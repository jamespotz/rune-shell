//! Notification daemon: serves `org.freedesktop.Notifications` and publishes
//! popups (shown now) and history (the notification center).
//!
//! The bus name is requested politely: if another daemon owns it (another
//! shell), Rune leaves it alone and notifications stay off.

use crate::icons::IconIndex;
use chrono::{DateTime, Local};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Notify, mpsc, watch};
use tracing::{info, warn};
use zbus::{
    Connection,
    fdo::{RequestNameFlags, RequestNameReply},
    object_server::SignalEmitter,
    zvariant::{OwnedValue, Structure},
};

const PATH: &str = "/org/freedesktop/Notifications";
const NAME: &str = "org.freedesktop.Notifications";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

/// Notification icon or image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Image {
    Path(PathBuf),
    /// Tightly packed RGBA8.
    Rgba {
        width: u32,
        height: u32,
        data: Arc<[u8]>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    /// Body with markup removed.
    pub body: String,
    pub image: Option<Image>,
    /// `(key, label)`; the `default` key is what clicking the notification does.
    pub actions: Vec<(String, String)>,
    pub urgency: Urgency,
    pub time: DateTime<Local>,
    /// Stays in popups after an action is invoked.
    pub resident: bool,
}

/// What the UI shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    /// Shown as popups now, newest first.
    pub popups: Vec<Notification>,
    /// Everything received, newest first (the notification center).
    pub history: Vec<Notification>,
    /// Do not disturb: notifications go straight to history.
    pub dnd: bool,
    /// Whether Rune owns the bus name (false when another daemon runs).
    pub active: bool,
}

/// Requests from the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Close a popup (it stays in history).
    Dismiss(u32),
    /// Run an action; closes the notification unless it is resident.
    Action(u32, String),
    /// Delete from history.
    Remove(u32),
    ClearHistory,
    SetDnd(bool),
    /// Pointer over the popups: expiry timers pause, like Sonner.
    Hover(bool),
    /// New popup timing and history size (the config changed).
    SetOptions(Options),
    /// Show a notification raised by the shell itself (event reminders); its
    /// popup stays until the user closes or dismisses it.
    Post(Notification),
}

/// Popup timing and history size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Used when the sender asks for the default (-1).
    pub timeout: Duration,
    pub history: usize,
}

struct Inner {
    state: State,
    next_id: u32,
    expiry: HashMap<u32, Instant>,
    paused_at: Option<Instant>,
    options: Options,
    icons: Arc<IconIndex>,
}

impl Inner {
    /// Adds or replaces a notification; returns its id.
    fn add(&mut self, mut n: Notification, replaces: u32, expire_ms: i32, now: Instant) -> u32 {
        n.id = if replaces != 0 && self.state.history.iter().any(|h| h.id == replaces) {
            replaces
        } else {
            self.next_id = self.next_id.wrapping_add(1).max(1);
            self.next_id
        };
        let id = n.id;
        self.state.history.retain(|h| h.id != id);
        self.state.popups.retain(|h| h.id != id);
        self.state.history.insert(0, n.clone());
        self.state.history.truncate(self.options.history);
        let timeout = match (expire_ms, n.urgency) {
            (_, Urgency::Critical) | (0, _) => None, // critical and "never" stay until dismissed
            (ms, _) if ms > 0 => Some(Duration::from_millis(ms.unsigned_abs().into())),
            _ => Some(self.options.timeout),
        };
        self.expiry.remove(&id);
        if !self.state.dnd || n.urgency == Urgency::Critical {
            self.state.popups.insert(0, n);
            if let Some(t) = timeout {
                self.expiry.insert(id, now + t);
            }
        }
        id
    }

    /// Adds a shell notification that never times out.
    fn post(&mut self, n: Notification, now: Instant) -> u32 {
        self.add(n, 0, 0, now)
    }

    /// Removes expired popups; returns their ids.
    fn expire(&mut self, now: Instant) -> Vec<u32> {
        if self.paused_at.is_some() {
            return Vec::new();
        }
        let gone: Vec<u32> = self
            .expiry
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(id, _)| *id)
            .collect();
        for id in &gone {
            self.expiry.remove(id);
            self.state.popups.retain(|p| p.id != *id);
        }
        gone
    }

    fn close_popup(&mut self, id: u32) -> bool {
        self.expiry.remove(&id);
        let before = self.state.popups.len();
        self.state.popups.retain(|p| p.id != id);
        before != self.state.popups.len()
    }

    fn hover(&mut self, on: bool, now: Instant) {
        match (on, self.paused_at) {
            (true, None) => self.paused_at = Some(now),
            (false, Some(since)) => {
                let paused = now - since;
                for at in self.expiry.values_mut() {
                    *at += paused;
                }
                self.paused_at = None;
            }
            _ => {}
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        if self.paused_at.is_some() {
            None
        } else {
            self.expiry.values().min().copied()
        }
    }
}

struct Server {
    inner: Arc<Mutex<Inner>>,
    tx: watch::Sender<State>,
    wake: Arc<Notify>,
}

impl Server {
    fn publish(&self, inner: &Inner) {
        self.tx.send_replace(inner.state.clone());
    }
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    #[expect(
        clippy::too_many_arguments,
        reason = "signature fixed by the freedesktop notification spec"
    )]
    async fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let Some(icons) = self.inner.lock().ok().map(|i| Arc::clone(&i.icons)) else {
            return 0;
        };
        let urgency = urgency(&hints);
        let resident = hints
            .get("resident")
            .and_then(|v| bool::try_from(v).ok())
            .unwrap_or(false);
        let image = tokio::task::spawn_blocking(move || {
            image_from_hints(&hints, &icons).or_else(|| icon_path(&app_icon, &icons))
        })
        .await
        .ok()
        .flatten();
        let n = Notification {
            id: 0,
            app_name,
            summary: strip_markup(&summary),
            body: strip_markup(&body),
            image,
            actions: actions
                .as_chunks::<2>()
                .0
                .iter()
                .map(|[k, v]| (k.clone(), v.clone()))
                .collect(),
            urgency,
            time: Local::now(),
            resident,
        };
        let Ok(mut inner) = self.inner.lock() else {
            return 0;
        };
        let id = inner.add(n, replaces_id, expire_timeout, Instant::now());
        self.publish(&inner);
        drop(inner);
        self.wake.notify_one();
        id
    }

    async fn close_notification(
        &self,
        id: u32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        let closed = self.inner.lock().map(|mut i| {
            let c = i.close_popup(id);
            self.publish(&i);
            c
        });
        if closed.unwrap_or(false) {
            let _ = Self::notification_closed(&emitter, id, 3).await; // 3 = closed by a call
        }
    }

    fn get_capabilities(&self) -> Vec<&'static str> {
        vec!["actions", "body", "icon-static", "persistence"]
    }

    fn get_server_information(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        ("Rune Shell", "Rune", env!("CARGO_PKG_VERSION"), "1.2")
    }

    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}

/// Starts the daemon on `conn`. Returns the state channel and a command sender;
/// `State::active` is false when another daemon already owns the name.
pub async fn spawn(
    conn: Connection,
    options: Options,
    dnd: bool,
    icons: Arc<IconIndex>,
) -> (watch::Receiver<State>, mpsc::UnboundedSender<Command>) {
    let state = State {
        dnd,
        ..State::default()
    };
    let (tx, rx) = watch::channel(state.clone());
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
    let inner = Arc::new(Mutex::new(Inner {
        state,
        next_id: 0,
        expiry: HashMap::new(),
        paused_at: None,
        options,
        icons,
    }));
    let wake = Arc::new(Notify::new());
    let server = Server {
        inner: inner.clone(),
        tx: tx.clone(),
        wake: wake.clone(),
    };
    if let Err(e) = conn.object_server().at(PATH, server).await {
        warn!("notifications: cannot export {PATH}: {e}");
        return (rx, cmd_tx);
    }
    // Never push another daemon aside; let a later one replace us if it asks.
    let flags = RequestNameFlags::DoNotQueue | RequestNameFlags::AllowReplacement;
    match conn.request_name_with_flags(NAME, flags).await {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
            info!("notification daemon running");
            // Set it on the service's own copy too, or the next update would clear it.
            if let Ok(mut i) = inner.lock() {
                i.state.active = true;
                tx.send_replace(i.state.clone());
            }
        }
        Ok(_) | Err(_) => {
            info!("another notification daemon is running; Rune's notifications are off");
            return (rx, cmd_tx);
        }
    }
    let emitter = SignalEmitter::new(&conn, PATH).map(|e| e.into_owned());
    let Ok(emitter) = emitter else {
        return (rx, cmd_tx);
    };
    tokio::spawn(async move {
        loop {
            let deadline = inner.lock().ok().and_then(|i| i.next_deadline());
            let sleep = async {
                match deadline {
                    Some(at) => tokio::time::sleep_until(at.into()).await,
                    None => std::future::pending().await,
                }
            };
            let mut closed = Vec::new();
            let mut invoked = None;
            tokio::select! {
                () = sleep => {
                    if let Ok(mut i) = inner.lock() {
                        closed = i.expire(Instant::now()).into_iter().map(|id| (id, 1)).collect(); // 1 = expired
                        tx.send_replace(i.state.clone());
                    }
                }
                () = wake.notified() => {}
                cmd = cmd_rx.recv() => {
                    let Some(cmd) = cmd else { return };
                    let Ok(mut i) = inner.lock() else { return };
                    match cmd {
                        Command::Dismiss(id) => if i.close_popup(id) { closed.push((id, 2)) }, // 2 = dismissed
                        Command::Action(id, key) => {
                            invoked = Some((id, key));
                            let resident = i.state.history.iter().any(|n| n.id == id && n.resident);
                            if !resident && i.close_popup(id) {
                                closed.push((id, 2));
                            }
                        }
                        Command::Remove(id) => {
                            i.state.history.retain(|n| n.id != id);
                            if i.close_popup(id) { closed.push((id, 2)) }
                        }
                        Command::ClearHistory => i.state.history.clear(),
                        Command::SetDnd(on) => i.state.dnd = on,
                        Command::Hover(on) => i.hover(on, Instant::now()),
                        Command::SetOptions(options) => {
                            i.options = options;
                            i.state.history.truncate(options.history);
                        }
                        Command::Post(n) => {
                            i.post(n, Instant::now());
                        }
                    }
                    tx.send_replace(i.state.clone());
                }
            }
            if let Some((id, key)) = invoked {
                let _ = Server::action_invoked(&emitter, id, &key).await;
            }
            for (id, reason) in closed {
                let _ = Server::notification_closed(&emitter, id, reason).await;
            }
        }
    });
    (rx, cmd_tx)
}

fn urgency(hints: &HashMap<String, OwnedValue>) -> Urgency {
    match hints.get("urgency").and_then(|v| u8::try_from(v).ok()) {
        Some(0) => Urgency::Low,
        Some(2) => Urgency::Critical,
        _ => Urgency::Normal,
    }
}

/// `image-data` (or the older names) as RGBA, else `image-path`.
fn image_from_hints(hints: &HashMap<String, OwnedValue>, icons: &IconIndex) -> Option<Image> {
    for key in ["image-data", "image_data", "icon_data"] {
        if let Some(img) = hints
            .get(key)
            .and_then(|v| Structure::try_from(v.try_clone().ok()?).ok())
            .and_then(raw_image)
        {
            return Some(img);
        }
    }
    let path = hints
        .get("image-path")
        .or_else(|| hints.get("image_path"))?;
    let path = String::try_from(path.try_clone().ok()?).ok()?;
    // libnotify sends `-i firefox` as an image-path holding an icon *name*.
    icon_path(&path, icons)
}

/// Decodes the spec's `(iiibiiay)`: width, height, rowstride, has_alpha, bits, channels, data.
fn raw_image(s: Structure<'_>) -> Option<Image> {
    let f = s.fields();
    let int = |i: usize| f.get(i).and_then(|v| i32::try_from(v).ok());
    let (w, h, stride) = (int(0)?, int(1)?, int(2)?);
    let alpha = f.get(3).and_then(|v| bool::try_from(v).ok())?;
    let channels = int(5)?;
    let data: Vec<u8> = f.get(6)?.try_clone().ok()?.try_into().ok()?;
    rgba_from_raw(w, h, stride, alpha, channels, &data)
}

fn rgba_from_raw(
    w: i32,
    h: i32,
    stride: i32,
    alpha: bool,
    channels: i32,
    data: &[u8],
) -> Option<Image> {
    let (w, h, stride, ch) = (
        usize::try_from(w).ok()?,
        usize::try_from(h).ok()?,
        usize::try_from(stride).ok()?,
        usize::try_from(channels).ok()?,
    );
    if w == 0 || h == 0 || w > 4096 || h > 4096 || ch < 3 || (alpha && ch < 4) {
        return None;
    }
    let mut out = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        let line = data.get(row * stride..row * stride + w * ch)?;
        for px in line.chunks_exact(ch) {
            out.extend_from_slice(&[px[0], px[1], px[2], if alpha { px[3] } else { 255 }]);
        }
    }
    // Notification pictures are displayed as 40px tiles. Bound retained history
    // pixels here as well as the renderer cache, while allowing HiDPI detail.
    let pixels = image::RgbaImage::from_raw(w as u32, h as u32, out)?;
    let pixels = if w > 128 || h > 128 {
        image::DynamicImage::ImageRgba8(pixels)
            .thumbnail(128, 128)
            .into_rgba8()
    } else {
        pixels
    };
    Some(Image::Rgba {
        width: pixels.width(),
        height: pixels.height(),
        data: pixels.into_raw().into(),
    })
}

fn icon_path(app_icon: &str, icons: &IconIndex) -> Option<Image> {
    let name = app_icon.strip_prefix("file://").unwrap_or(app_icon);
    icons.lookup(name).map(Image::Path)
}

/// Removes markup tags and decodes the common entities.
pub fn strip_markup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inner() -> Inner {
        Inner {
            state: State::default(),
            next_id: 0,
            expiry: HashMap::new(),
            paused_at: None,
            options: Options {
                timeout: Duration::from_secs(5),
                history: 3,
            },
            icons: Arc::new(IconIndex::default()),
        }
    }

    fn note(urgency: Urgency) -> Notification {
        Notification {
            id: 0,
            app_name: "app".into(),
            summary: "hi".into(),
            body: String::new(),
            image: None,
            actions: Vec::new(),
            urgency,
            time: Local::now(),
            resident: false,
        }
    }

    #[test]
    fn popups_expire_but_stay_in_history() {
        let mut i = inner();
        let t0 = Instant::now();
        let a = i.add(note(Urgency::Normal), 0, -1, t0);
        let b = i.add(note(Urgency::Critical), 0, -1, t0);
        assert_eq!(
            i.state.popups.iter().map(|n| n.id).collect::<Vec<_>>(),
            [b, a],
            "newest first"
        );
        assert_eq!(i.expire(t0 + Duration::from_secs(6)), [a]);
        assert_eq!(i.state.popups.len(), 1, "critical stays");
        assert_eq!(i.state.history.len(), 2);
    }

    #[test]
    fn posted_reminders_stay_until_dismissed() {
        let mut i = inner();
        let t = Instant::now();
        let id = i.post(note(Urgency::Normal), t);
        assert!(i.expire(t + Duration::from_secs(24 * 3600)).is_empty());
        assert!(i.state.popups.iter().any(|p| p.id == id));
    }

    #[test]
    fn replaces_and_caps_history() {
        let mut i = inner();
        let t = Instant::now();
        let a = i.add(note(Urgency::Normal), 0, 1000, t);
        assert_eq!(
            i.add(note(Urgency::Normal), a, 1000, t),
            a,
            "replaces_id keeps the id"
        );
        assert_eq!(i.state.history.len(), 1);
        for _ in 0..5 {
            i.add(note(Urgency::Low), 0, 0, t);
        }
        assert_eq!(i.state.history.len(), 3);
        assert_eq!(
            i.expire(t + Duration::from_secs(3600)),
            [a],
            "only the timed one expires; timeout 0 never does"
        );
    }

    #[test]
    fn hover_pauses_expiry() {
        let mut i = inner();
        let t = Instant::now();
        let a = i.add(note(Urgency::Normal), 0, 1000, t);
        i.hover(true, t + Duration::from_millis(500));
        assert!(i.expire(t + Duration::from_secs(10)).is_empty());
        i.hover(false, t + Duration::from_secs(10));
        assert!(
            i.expire(t + Duration::from_millis(10_400)).is_empty(),
            "the remaining 500 ms resume"
        );
        assert_eq!(i.expire(t + Duration::from_millis(10_600)), [a]);
    }

    #[test]
    fn dnd_sends_to_history_only() {
        let mut i = inner();
        i.state.dnd = true;
        i.add(note(Urgency::Normal), 0, -1, Instant::now());
        assert!(i.state.popups.is_empty());
        assert_eq!(i.state.history.len(), 1);
    }

    #[test]
    fn strips_markup() {
        assert_eq!(
            strip_markup("<b>Bold</b> &amp; <a href=\"x\">link</a> 1 &lt; 2"),
            "Bold & link 1 < 2"
        );
    }

    #[test]
    fn converts_raw_images() {
        // 2x1 RGB with a padded rowstride of 8.
        let data = [255, 0, 0, 0, 255, 0, 9, 9];
        let Some(Image::Rgba {
            width,
            height,
            data,
        }) = rgba_from_raw(2, 1, 8, false, 3, &data)
        else {
            panic!()
        };
        assert_eq!((width, height), (2, 1));
        assert_eq!(&*data, &[255, 0, 0, 255, 0, 255, 0, 255]);
        assert!(
            rgba_from_raw(2, 2, 8, false, 3, &[0; 8]).is_none(),
            "short data is rejected"
        );
    }
    #[test]
    fn notification_history_keeps_thumbnail_pixels_not_full_images() {
        let pixels = vec![180u8; 1024 * 512 * 4];
        let Image::Rgba {
            width,
            height,
            data,
        } = rgba_from_raw(1024, 512, 1024 * 4, true, 4, &pixels).unwrap()
        else {
            panic!("RGBA expected")
        };
        assert_eq!((width, height), (128, 64));
        assert_eq!(data.len(), 128 * 64 * 4);
        assert!(
            data.as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [180, 180, 180, 180])
        );
    }
}

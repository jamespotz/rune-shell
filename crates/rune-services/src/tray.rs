//! System tray (StatusNotifierItem). Rune hosts the tray icons; it also acts
//! as the `org.kde.StatusNotifierWatcher` registry, unless another shell already does.
//!
//! Like the other services, any relevant signal just triggers a re-read of
//! every item.

use crate::{icons::IconIndex, notifications::Image};
use futures_util::{StreamExt, stream::select_all};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{Notify, watch};
use tracing::{info, warn};
use zbus::{
    Connection, MessageStream,
    fdo::{RequestNameFlags, RequestNameReply},
    message::Header,
    object_server::SignalEmitter,
    zvariant::{OwnedObjectPath, OwnedValue, Structure, Value},
};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";

/// One tray icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayItem {
    /// `bus name` + `object path`; stable while the app runs.
    pub key: String,
    pub service: String,
    pub path: String,
    pub id: String,
    pub title: String,
    pub image: Option<Image>,
    /// dbusmenu object path, if the app exports a menu.
    pub menu: Option<String>,
    /// The app wants a menu (not Activate) on a primary click.
    pub item_is_menu: bool,
}

/// One entry of an app's tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub kind: MenuKind,
    pub children: Vec<MenuEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKind {
    Normal,
    Separator,
    Check(bool),
    Radio(bool),
}

#[zbus::proxy(interface = "org.kde.StatusNotifierItem")]
trait StatusNotifierItem {
    fn activate(&self, x: i32, y: i32) -> zbus::Result<()>;
    fn secondary_activate(&self, x: i32, y: i32) -> zbus::Result<()>;
    fn context_menu(&self, x: i32, y: i32) -> zbus::Result<()>;
    #[zbus(property)]
    fn id(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn title(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn status(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn icon_name(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn icon_theme_path(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn icon_pixmap(&self) -> zbus::Result<Vec<(i32, i32, Vec<u8>)>>;
    #[zbus(property)]
    fn menu(&self) -> zbus::Result<OwnedObjectPath>;
    #[zbus(property)]
    fn item_is_menu(&self) -> zbus::Result<bool>;
}

#[zbus::proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_service = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher"
)]
trait WatcherClient {
    fn register_status_notifier_host(&self, service: &str) -> zbus::Result<()>;
    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> zbus::Result<Vec<String>>;
}

type Layout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

#[zbus::proxy(interface = "com.canonical.dbusmenu")]
trait DBusMenu {
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: &[&str],
    ) -> zbus::Result<(u32, Layout)>;
    fn event(&self, id: i32, event_id: &str, data: &Value<'_>, timestamp: u32) -> zbus::Result<()>;
    fn about_to_show(&self, id: i32) -> zbus::Result<bool>;
}

/// Our registry, when no other watcher exists.
struct Watcher {
    items: Arc<Mutex<Vec<String>>>,
    changed: Arc<Notify>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        let address = item_address(service, &sender);
        let added = self.items.lock().is_ok_and(|mut items| {
            let fresh = !items.contains(&address);
            if fresh {
                items.push(address.clone());
            }
            fresh
        });
        if added {
            let _ = Self::status_notifier_item_registered(&emitter, &address).await;
            self.changed.notify_one();
        }
    }

    fn register_status_notifier_host(&self, _service: &str) {}

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().map(|i| i.clone()).unwrap_or_default()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;
}

/// `service` as sent to RegisterStatusNotifierItem: a bus name, or an object
/// path (then the item lives on the sender's connection).
fn item_address(service: &str, sender: &str) -> String {
    if service.starts_with('/') {
        format!("{sender}{service}")
    } else {
        format!("{service}/StatusNotifierItem")
    }
}

/// Splits `bus/path` at the first `/`.
fn split_address(address: &str) -> Option<(&str, String)> {
    let i = address.find('/')?;
    Some((&address[..i], address[i..].to_owned()))
}

/// Sends clicks and menu events to tray apps.
#[derive(Debug, Clone)]
pub struct TrayHandle {
    conn: Connection,
}

impl TrayHandle {
    async fn item<'a>(&self, item: &'a TrayItem) -> zbus::Result<StatusNotifierItemProxy<'a>> {
        StatusNotifierItemProxy::builder(&self.conn)
            .destination(item.service.as_str())?
            .path(item.path.as_str())?
            .build()
            .await
    }

    /// Primary (`Activate`) or middle (`SecondaryActivate`) click.
    pub async fn activate(&self, item: &TrayItem, secondary: bool) -> zbus::Result<()> {
        let p = self.item(item).await?;
        if secondary {
            p.secondary_activate(0, 0).await
        } else {
            p.activate(0, 0).await
        }
    }

    /// Asks the app to show its own menu (for apps without a dbusmenu).
    pub async fn context_menu(&self, item: &TrayItem) -> zbus::Result<()> {
        self.item(item).await?.context_menu(0, 0).await
    }

    async fn menu_proxy<'a>(&self, item: &'a TrayItem) -> zbus::Result<DBusMenuProxy<'a>> {
        let path = item
            .menu
            .as_deref()
            .ok_or_else(|| zbus::Error::Failure("no menu".into()))?;
        DBusMenuProxy::builder(&self.conn)
            .destination(item.service.as_str())?
            .path(path.to_owned())?
            .build()
            .await
    }

    /// The item's menu tree.
    pub async fn menu(&self, item: &TrayItem) -> zbus::Result<Vec<MenuEntry>> {
        let p = self.menu_proxy(item).await?;
        let _ = p.about_to_show(0).await; // lets apps refresh the menu; optional
        let (_, (_, _, children)) = p.get_layout(0, -1, &[]).await?;
        Ok(children.iter().filter_map(parse_entry).collect())
    }

    /// Clicks menu entry `id`.
    pub async fn click(&self, item: &TrayItem, id: i32) -> zbus::Result<()> {
        let now = u32::try_from(chrono::Utc::now().timestamp()).unwrap_or(0);
        self.menu_proxy(item)
            .await?
            .event(id, "clicked", &Value::I32(0), now)
            .await
    }
}

/// Parses one `(ia{sv}av)` menu node; `None` for invisible entries.
fn parse_entry(value: &OwnedValue) -> Option<MenuEntry> {
    let s = Structure::try_from(value.try_clone().ok()?).ok()?;
    let f = s.fields();
    let id = i32::try_from(f.first()?).ok()?;
    let props: HashMap<String, OwnedValue> = f.get(1)?.try_clone().ok()?.try_into().ok()?;
    let children: Vec<OwnedValue> = f.get(2)?.try_clone().ok()?.try_into().ok()?;
    let text = |k: &str| {
        props
            .get(k)
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
    };
    let flag = |k: &str| props.get(k).and_then(|v| bool::try_from(v).ok());
    if flag("visible") == Some(false) {
        return None;
    }
    let on = props
        .get("toggle-state")
        .and_then(|v| i32::try_from(v).ok())
        == Some(1);
    let kind = match (text("type").as_deref(), text("toggle-type").as_deref()) {
        (Some("separator"), _) => MenuKind::Separator,
        (_, Some("checkmark")) => MenuKind::Check(on),
        (_, Some("radio")) => MenuKind::Radio(on),
        _ => MenuKind::Normal,
    };
    Some(MenuEntry {
        id,
        label: strip_mnemonic(&text("label").unwrap_or_default()),
        enabled: flag("enabled").unwrap_or(true),
        kind,
        children: children.iter().filter_map(parse_entry).collect(),
    })
}

/// `_File` → `File`, `__` → `_`.
fn strip_mnemonic(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                out.push('_');
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Starts the tray on `conn`.
pub async fn spawn(
    conn: Connection,
    icons: Arc<IconIndex>,
) -> (watch::Receiver<Vec<TrayItem>>, TrayHandle) {
    let (tx, rx) = watch::channel(Vec::new());
    let handle = TrayHandle { conn: conn.clone() };
    tokio::spawn(async move {
        if let Err(e) = run(conn, icons, tx).await {
            warn!("tray stopped: {e}");
        }
    });
    (rx, handle)
}

async fn run(
    conn: Connection,
    icons: Arc<IconIndex>,
    tx: watch::Sender<Vec<TrayItem>>,
) -> zbus::Result<()> {
    let ours = Arc::new(Mutex::new(Vec::<String>::new()));
    let changed = Arc::new(Notify::new());
    let mut is_watcher = false;
    if matches!(
        conn.request_name_with_flags(WATCHER, RequestNameFlags::DoNotQueue.into())
            .await,
        Ok(RequestNameReply::PrimaryOwner)
    ) {
        conn.object_server()
            .at(
                WATCHER_PATH,
                Watcher {
                    items: ours.clone(),
                    changed: changed.clone(),
                },
            )
            .await?;
        is_watcher = true;
        info!("tray: acting as the StatusNotifierWatcher");
    } else {
        let host = format!("org.kde.StatusNotifierHost-{}-rune", std::process::id());
        conn.request_name(host.as_str()).await?;
        WatcherClientProxy::new(&conn)
            .await?
            .register_status_notifier_host(&host)
            .await?;
        info!("tray: registered as a host with the existing watcher");
    }
    let rules = [
        "type='signal',interface='org.kde.StatusNotifierItem'",
        "type='signal',interface='org.kde.StatusNotifierWatcher'",
        "type='signal',sender='org.freedesktop.DBus',member='NameOwnerChanged'",
    ];
    let mut streams = Vec::new();
    for rule in rules {
        streams.push(MessageStream::for_match_rule(rule, &conn, Some(256)).await?);
    }
    let mut events = select_all(streams);
    loop {
        let addresses = if is_watcher {
            ours.lock().map(|i| i.clone()).unwrap_or_default()
        } else {
            match WatcherClientProxy::builder(&conn)
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build()
                .await
            {
                Ok(w) => w
                    .registered_status_notifier_items()
                    .await
                    .unwrap_or_default(),
                Err(_) => Vec::new(),
            }
        };
        let mut items = Vec::new();
        for address in &addresses {
            if let Some(item) = read_item(&conn, address, &icons).await {
                items.push(item);
            }
        }
        tx.send_if_modified(|current| {
            let changed = *current != items;
            if changed {
                *current = items;
            }
            changed
        });
        tokio::select! {
            event = events.next() => {
                let Some(Ok(msg)) = event else { return Ok(()) };
                if is_watcher {
                    forget_vanished(&msg, &ours, &conn).await;
                }
            }
            () = changed.notified() => {}
        }
        // Apps fire several signals at once (icon, title, status); refresh once.
        while let Ok(Some(Ok(msg))) =
            tokio::time::timeout(Duration::from_millis(50), events.next()).await
        {
            if is_watcher {
                forget_vanished(&msg, &ours, &conn).await;
            }
        }
    }
}

/// When an app's bus name disappears, drops its items from our registry.
async fn forget_vanished(msg: &zbus::Message, ours: &Mutex<Vec<String>>, conn: &Connection) {
    if msg
        .header()
        .member()
        .is_none_or(|m| m.as_str() != "NameOwnerChanged")
    {
        return;
    }
    let Ok((name, _old, new)) = msg.body().deserialize::<(String, String, String)>() else {
        return;
    };
    if !new.is_empty() {
        return;
    }
    let gone: Vec<String> = match ours.lock() {
        Ok(mut items) => {
            let gone = items
                .iter()
                .filter(|a| split_address(a).is_some_and(|(bus, _)| bus == name))
                .cloned()
                .collect();
            items.retain(|a| split_address(a).is_none_or(|(bus, _)| bus != name));
            gone
        }
        Err(_) => return,
    };
    if let Ok(emitter) = SignalEmitter::new(conn, WATCHER_PATH) {
        for address in gone {
            let _ = Watcher::status_notifier_item_unregistered(&emitter, &address).await;
        }
    }
}

async fn read_item(conn: &Connection, address: &str, icons: &IconIndex) -> Option<TrayItem> {
    let (service, path) = split_address(address)?;
    let p = StatusNotifierItemProxy::builder(conn)
        .destination(service.to_owned())
        .ok()?
        .path(path.clone())
        .ok()?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .ok()?;
    // "Passive" items ask not to be shown.
    if p.status().await.is_ok_and(|s| s == "Passive") {
        return None;
    }
    let name = p.icon_name().await.unwrap_or_default();
    let theme_path = p.icon_theme_path().await.unwrap_or_default();
    // A themed icon looks better than the app's pixmap, so prefer it.
    let image = match icon_in_dir(&theme_path, &name).or_else(|| icons.lookup(&name)) {
        Some(path) => Some(Image::Path(path)),
        None => p.icon_pixmap().await.ok().and_then(best_pixmap),
    };
    Some(TrayItem {
        key: address.to_owned(),
        service: service.to_owned(),
        id: p.id().await.unwrap_or_default(),
        title: p.title().await.unwrap_or_default(),
        image,
        menu: p
            .menu()
            .await
            .ok()
            .map(|m| m.to_string())
            .filter(|m| m != "/"),
        item_is_menu: p.item_is_menu().await.unwrap_or(false),
        path,
    })
}

/// Apps may ship their own icon directory (`IconThemePath`).
fn icon_in_dir(dir: &str, name: &str) -> Option<PathBuf> {
    if dir.is_empty() || name.is_empty() {
        return None;
    }
    fn find(dir: &Path, name: &str, depth: u8) -> Option<PathBuf> {
        let entries = std::fs::read_dir(dir).ok()?;
        let mut subdirs = Vec::new();
        for path in entries.flatten().map(|e| e.path()) {
            if path.is_dir() {
                subdirs.push(path);
            } else if path.file_stem().is_some_and(|s| s == name) {
                return Some(path);
            }
        }
        if depth == 0 {
            return None;
        }
        subdirs.iter().find_map(|d| find(d, name, depth - 1))
    }
    find(Path::new(dir), name, 3)
}

/// Largest ARGB32 (network byte order) pixmap, as RGBA.
fn best_pixmap(pixmaps: Vec<(i32, i32, Vec<u8>)>) -> Option<Image> {
    let (w, h, argb) = pixmaps
        .into_iter()
        .filter(|(w, h, d)| *w > 0 && *h > 0 && d.len() >= (*w * *h * 4) as usize)
        .max_by_key(|(w, _, _)| *w)?;
    let rgba: Vec<u8> = argb
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|[a, r, g, b]| [*r, *g, *b, *a])
        .collect();
    Some(Image::Rgba {
        width: w.unsigned_abs(),
        height: h.unsigned_abs(),
        data: rgba.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert_eq!(
            item_address("org.kde.StatusNotifierItem-12-1", ":1.5"),
            "org.kde.StatusNotifierItem-12-1/StatusNotifierItem"
        );
        assert_eq!(
            item_address("/org/ayatana/NotificationItem/x", ":1.9"),
            ":1.9/org/ayatana/NotificationItem/x"
        );
        assert_eq!(
            split_address(":1.9/org/x"),
            Some((":1.9", "/org/x".to_owned()))
        );
        assert_eq!(split_address("nopath"), None);
    }

    #[test]
    fn strips_mnemonics() {
        assert_eq!(strip_mnemonic("_Quit"), "Quit");
        assert_eq!(strip_mnemonic("Save__As"), "Save_As");
    }

    #[test]
    fn converts_pixmaps() {
        let small = (1, 1, vec![255, 1, 2, 3]);
        let big = (2, 1, vec![128, 10, 20, 30, 255, 40, 50, 60]);
        let Some(Image::Rgba { width, data, .. }) = best_pixmap(vec![small, big]) else {
            panic!()
        };
        assert_eq!(width, 2);
        assert_eq!(&*data, &[10, 20, 30, 128, 40, 50, 60, 255]);
        assert!(
            best_pixmap(vec![(4, 4, vec![0; 3])]).is_none(),
            "truncated data is ignored"
        );
    }

    #[test]
    fn parses_menu_layout() {
        let node = |id: i32,
                    props: Vec<(&str, Value<'static>)>,
                    children: Vec<OwnedValue>|
         -> OwnedValue {
            let props: HashMap<String, OwnedValue> = props
                .into_iter()
                .map(|(k, v)| (k.to_owned(), OwnedValue::try_from(v).unwrap()))
                .collect();
            let s = zbus::zvariant::StructureBuilder::new()
                .add_field(id)
                .add_field(props)
                .add_field(children)
                .build()
                .unwrap();
            OwnedValue::try_from(Value::from(s)).unwrap()
        };
        let quit = node(2, vec![("label", Value::from("_Quit"))], vec![]);
        let sep = node(3, vec![("type", Value::from("separator"))], vec![]);
        let hidden = node(4, vec![("visible", Value::from(false))], vec![]);
        let check = node(
            5,
            vec![
                ("label", Value::from("Mute")),
                ("toggle-type", Value::from("checkmark")),
                ("toggle-state", Value::from(1)),
            ],
            vec![],
        );
        let sub = node(1, vec![("label", Value::from("More"))], vec![check]);
        let entries: Vec<MenuEntry> = [sub, sep, hidden, quit]
            .iter()
            .filter_map(parse_entry)
            .collect();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].children[0].kind, MenuKind::Check(true));
        assert_eq!(entries[1].kind, MenuKind::Separator);
        assert_eq!(
            (entries[2].id, entries[2].label.as_str(), entries[2].enabled),
            (2, "Quit", true)
        );
    }
}

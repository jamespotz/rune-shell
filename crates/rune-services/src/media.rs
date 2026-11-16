//! Now playing, via MPRIS on the session bus.
//!
//! D-Bus signals (players appearing, property changes, seeks) only trigger a
//! re-read of every player, like the Hyprland service. Playback position is not
//! signalled while playing, so [`Media::position_now`] extrapolates it.

use futures_util::{StreamExt, stream::select_all};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};
use zbus::{Connection, MessageStream, zvariant::OwnedValue};

const PREFIX: &str = "org.mpris.MediaPlayer2.";

#[zbus::proxy(
    interface = "org.mpris.MediaPlayer2.Player",
    default_path = "/org/mpris/MediaPlayer2"
)]
trait Player {
    fn play_pause(&self) -> zbus::Result<()>;
    fn next(&self) -> zbus::Result<()>;
    fn previous(&self) -> zbus::Result<()>;
    fn stop(&self) -> zbus::Result<()>;
    fn set_position(
        &self,
        track_id: &zbus::zvariant::ObjectPath<'_>,
        position: i64,
    ) -> zbus::Result<()>;
    #[zbus(property)]
    fn playback_status(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn metadata(&self) -> zbus::Result<HashMap<String, OwnedValue>>;
    #[zbus(property)]
    fn position(&self) -> zbus::Result<i64>;
}

#[zbus::proxy(
    interface = "org.mpris.MediaPlayer2",
    default_path = "/org/mpris/MediaPlayer2"
)]
trait MediaPlayer2 {
    #[zbus(property)]
    fn identity(&self) -> zbus::Result<String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Playing,
    Paused,
    Stopped,
}

/// The player shown in the shell.
#[derive(Debug, Clone, PartialEq)]
pub struct Media {
    /// Bus name, e.g. `org.mpris.MediaPlayer2.spotify`.
    pub player: String,
    /// Human name, e.g. `Spotify`.
    pub identity: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// Album art image bytes (PNG/JPEG...). Bytes rather than a path because
    /// some players write art to extension-less temp files.
    pub art: Option<Arc<[u8]>>,
    pub length: Option<Duration>,
    pub status: Status,
    /// Position when the state was read, at `read_at`.
    pub position: Duration,
    pub read_at: Instant,
    /// `mpris:trackid`, needed to seek.
    pub track_id: Option<String>,
}

impl Media {
    /// Current playback position, extrapolated while playing.
    pub fn position_now(&self) -> Duration {
        let pos = match self.status {
            Status::Playing => self.position + self.read_at.elapsed(),
            _ => self.position,
        };
        self.length.map_or(pos, |len| pos.min(len))
    }
}

/// Transport controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    PlayPause,
    Next,
    Previous,
    Stop,
    /// Jump to this position in the current track.
    Seek(Duration),
}

/// Handle for sending controls to the player currently shown.
#[derive(Debug, Clone)]
pub struct MediaHandle {
    conn: Connection,
    state: watch::Receiver<Option<Media>>,
}

impl MediaHandle {
    pub async fn control(&self, control: Control) -> zbus::Result<()> {
        let Some(player) = self.state.borrow().as_ref().map(|m| m.player.clone()) else {
            return Ok(());
        };
        let p = PlayerProxy::builder(&self.conn)
            .destination(player)?
            .build()
            .await?;
        match control {
            Control::PlayPause => p.play_pause().await,
            Control::Next => p.next().await,
            Control::Previous => p.previous().await,
            Control::Stop => p.stop().await,
            Control::Seek(to) => {
                // SetPosition needs the current track's id, so a stale seek can't move the next song.
                let track = self
                    .state
                    .borrow()
                    .as_ref()
                    .and_then(|m| m.track_id.clone());
                let Some(track) = track else { return Ok(()) };
                let micros = i64::try_from(to.as_micros()).unwrap_or(i64::MAX);
                p.set_position(
                    &zbus::zvariant::ObjectPath::try_from(track.as_str())?,
                    micros,
                )
                .await
            }
        }
    }
}

/// Connects to the session bus and spawns the watcher. `None` (logged) when
/// there is no session bus; the media widget then just stays hidden.
pub async fn spawn() -> Option<(watch::Receiver<Option<Media>>, MediaHandle)> {
    let conn = Connection::session()
        .await
        .inspect_err(|e| warn!("no session bus: {e}; media disabled"))
        .ok()?;
    let (tx, rx) = watch::channel(None);
    let handle = MediaHandle {
        conn: conn.clone(),
        state: rx.clone(),
    };
    tokio::spawn(async move {
        if let Err(e) = run(conn, tx).await {
            warn!("media watcher stopped: {e}");
        }
    });
    Some((rx, handle))
}

async fn run(conn: Connection, tx: watch::Sender<Option<Media>>) -> zbus::Result<()> {
    let rules = [
        "type='signal',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',path='/org/mpris/MediaPlayer2'",
        "type='signal',interface='org.mpris.MediaPlayer2.Player',member='Seeked',path='/org/mpris/MediaPlayer2'",
        "type='signal',sender='org.freedesktop.DBus',member='NameOwnerChanged',arg0namespace='org.mpris.MediaPlayer2'",
    ];
    let mut streams = Vec::new();
    for rule in rules {
        streams.push(MessageStream::for_match_rule(rule, &conn, Some(64)).await?);
    }
    let mut events = select_all(streams);
    // Finished art downloads also trigger a refresh.
    let (art_done, mut art_rx) = mpsc::unbounded_channel::<()>();
    loop {
        let current = tx.borrow().as_ref().map(|m| m.player.clone());
        let next = snapshot(&conn, current.as_deref(), &art_done).await;
        tx.send_if_modified(|m| {
            let changed = !same(m.as_ref(), next.as_ref());
            if changed {
                *m = next;
            }
            changed
        });
        tokio::select! {
            event = events.next() => if event.is_none() { return Ok(()) },
            _ = art_rx.recv() => {}
        }
        // Players emit bursts (metadata, status, length); refresh once per burst.
        while let Ok(Some(_)) = tokio::time::timeout(Duration::from_millis(30), events.next()).await
        {
        }
    }
}

/// Equal apart from the read timestamp, so an unchanged re-read doesn't redraw.
fn same(a: Option<&Media>, b: Option<&Media>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            let drift = a.position_now().abs_diff(b.position_now());
            Media {
                read_at: b.read_at,
                position: b.position,
                ..a.clone()
            } == *b
                && drift < Duration::from_secs(1)
        }
        (None, None) => true,
        _ => false,
    }
}

/// Reads every player and picks the one to show.
async fn snapshot(
    conn: &Connection,
    current: Option<&str>,
    art_done: &mpsc::UnboundedSender<()>,
) -> Option<Media> {
    let names = zbus::fdo::DBusProxy::new(conn)
        .await
        .ok()?
        .list_names()
        .await
        .ok()?;
    let mut players = Vec::new();
    for name in names
        .iter()
        .map(|n| n.as_str())
        .filter(|n| n.starts_with(PREFIX))
    {
        match read_player(conn, name, art_done).await {
            Ok(media) => players.push(media),
            Err(e) => info!("skipping {name}: {e}"),
        }
    }
    pick(players, current)
}

/// Playing beats paused beats stopped; on a tie the player already shown stays.
fn pick(mut players: Vec<Media>, current: Option<&str>) -> Option<Media> {
    let rank = |m: &Media| {
        let status = match m.status {
            Status::Playing => 0,
            Status::Paused => 2,
            Status::Stopped => 4,
        };
        status + usize::from(Some(m.player.as_str()) != current)
    };
    players.sort_by_key(rank);
    players.into_iter().next()
}

async fn read_player(
    conn: &Connection,
    name: &str,
    art_done: &mpsc::UnboundedSender<()>,
) -> zbus::Result<Media> {
    let p = PlayerProxy::builder(conn)
        .destination(name.to_owned())?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await?;
    let status = match p.playback_status().await?.as_str() {
        "Playing" => Status::Playing,
        "Paused" => Status::Paused,
        _ => Status::Stopped,
    };
    let meta = p.metadata().await.unwrap_or_default();
    // Not every player implements Position.
    let position = p.position().await.map_or(Duration::ZERO, micros);
    let identity = MediaPlayer2Proxy::builder(conn)
        .destination(name.to_owned())?
        .build()
        .await?
        .identity()
        .await
        .unwrap_or_else(|_| name.trim_start_matches(PREFIX).to_owned());
    let mut media = from_metadata(&meta, name, identity, status, position);
    media.art = string(&meta, "mpris:artUrl")
        .and_then(|url| crate::art::resolve(&url, art_done))
        .and_then(|path| read_art(&path));
    Ok(media)
}

/// Reads art, re-reading only when the file or its mtime changes.
fn read_art(path: &Path) -> Option<Arc<[u8]>> {
    type Cached = Option<(PathBuf, SystemTime, Arc<[u8]>)>;
    static LAST: Mutex<Cached> = Mutex::new(None);
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    let mut last = LAST.lock().ok()?;
    if let Some((p, m, data)) = last.as_ref()
        && p == path
        && *m == modified
    {
        return Some(data.clone());
    }
    // shortcut: a small blocking read on the runtime; move to spawn_blocking if art gets large.
    let data: Arc<[u8]> = std::fs::read(path).ok()?.into();
    *last = Some((path.to_path_buf(), modified, data.clone()));
    Some(data)
}

fn from_metadata(
    meta: &HashMap<String, OwnedValue>,
    player: &str,
    identity: String,
    status: Status,
    position: Duration,
) -> Media {
    let artist = meta
        .get("xesam:artist")
        .and_then(|v| Vec::<String>::try_from(v.try_clone().ok()?).ok())
        .map(|a| {
            a.into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let length = meta.get("mpris:length").and_then(|v| {
        // Spec says int64, but some players send uint64.
        i64::try_from(v)
            .ok()
            .or_else(|| u64::try_from(v).ok().and_then(|u| i64::try_from(u).ok()))
    });
    Media {
        player: player.to_owned(),
        identity,
        title: string(meta, "xesam:title").unwrap_or_default(),
        artist,
        album: string(meta, "xesam:album").unwrap_or_default(),
        art: None,
        length: length.filter(|l| *l > 0).map(micros),
        status,
        position,
        read_at: Instant::now(),
        track_id: meta
            .get("mpris:trackid")
            .and_then(|v| zbus::zvariant::OwnedObjectPath::try_from(v.try_clone().ok()?).ok())
            .map(|p| p.to_string()),
    }
}

fn string(meta: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    meta.get(key)
        .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
        .filter(|s| !s.is_empty())
}

fn micros(us: i64) -> Duration {
    Duration::from_micros(us.max(0).unsigned_abs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    fn meta() -> HashMap<String, OwnedValue> {
        let v = |v: Value<'static>| OwnedValue::try_from(v).unwrap();
        HashMap::from([
            ("xesam:title".to_owned(), v(Value::from("Song"))),
            (
                "xesam:artist".to_owned(),
                v(Value::from(vec!["A", "", "B"])),
            ),
            ("mpris:length".to_owned(), v(Value::from(1_734_821_000_i64))),
            (
                "mpris:trackid".to_owned(),
                v(Value::from(
                    zbus::zvariant::ObjectPath::try_from("/org/mpris/Track/7").unwrap(),
                )),
            ),
        ])
    }

    #[test]
    fn parses_metadata() {
        let m = from_metadata(
            &meta(),
            "org.mpris.MediaPlayer2.x",
            "X".into(),
            Status::Paused,
            Duration::from_secs(5),
        );
        assert_eq!(m.title, "Song");
        assert_eq!(m.artist, "A, B");
        assert_eq!(m.album, "");
        assert_eq!(
            m.track_id.as_deref(),
            Some("/org/mpris/Track/7"),
            "seeking needs the track id"
        );
        assert_eq!(m.length, Some(Duration::from_micros(1_734_821_000)));
        assert_eq!(m.position_now(), Duration::from_secs(5)); // paused: no drift
    }

    #[test]
    fn picks_playing_then_current() {
        let m = |name: &str, status| Media {
            player: name.into(),
            status,
            ..from_metadata(&HashMap::new(), name, String::new(), status, Duration::ZERO)
        };
        let players = || {
            vec![
                m("a", Status::Paused),
                m("b", Status::Playing),
                m("c", Status::Paused),
            ]
        };
        assert_eq!(pick(players(), None).unwrap().player, "b");
        let paused = vec![m("a", Status::Paused), m("c", Status::Paused)];
        assert_eq!(pick(paused.clone(), Some("c")).unwrap().player, "c");
        assert_eq!(pick(paused, None).unwrap().player, "a");
        assert!(pick(Vec::new(), None).is_none());
    }

    #[test]
    fn extrapolates_while_playing() {
        let mut m = from_metadata(
            &meta(),
            "p",
            String::new(),
            Status::Playing,
            Duration::from_secs(10),
        );
        m.read_at = Instant::now() - Duration::from_secs(3);
        let pos = m.position_now();
        assert!(pos >= Duration::from_secs(13) && pos < Duration::from_secs(14));
    }
}

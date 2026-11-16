//! Clipboard history through `cliphist`. If nothing is recording history yet,
//! Rune starts the usual `wl-paste --watch cliphist store` watchers.
//!
//! cliphist keeps neither times nor favorites, so Rune adds a side store in
//! `$XDG_DATA_HOME/rune-shell/clipboard/`: `state.json` plus one file per
//! favorite (its own copy, so favorites survive a wipe or cliphist's size limit).
//! Image previews are decoded once into `$XDG_CACHE_HOME/rune-shell/clipboard/`.
//!
//! The logic lives in [`Clipboard`], generic over a [`Tools`] adapter: the real
//! one runs `cliphist` and `wl-copy`; tests use an in-memory one.

use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::Mutex,
};
use tracing::{info, warn};

/// Which entry: a cliphist history id, or one of Rune's favorites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Id {
    History(u64),
    Favorite(u64),
}

/// One entry, ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: Id,
    /// One line: the text, or an image's size and format.
    pub title: String,
    /// When it was copied (unix seconds), as far as Rune knows.
    pub time: Option<i64>,
    /// The image itself, for image entries.
    pub image: Option<PathBuf>,
}

impl Entry {
    pub fn is_favorite(&self) -> bool {
        matches!(self.id, Id::Favorite(_))
    }
}

/// The clipboard tools Rune drives. One adapter runs the real programs;
/// tests use an in-memory one.
pub trait Tools {
    /// cliphist's history, newest first: (id, one-line preview).
    fn list(&self) -> impl Future<Output = Result<Vec<(u64, String)>, String>> + Send;
    /// The raw contents of history entry `id`.
    fn decode(&self, id: u64) -> impl Future<Output = Result<Vec<u8>, String>> + Send;
    fn wipe(&self) -> impl Future<Output = Result<(), String>> + Send;
    /// Puts `data` on the clipboard.
    fn set(&self, data: Vec<u8>) -> impl Future<Output = Result<(), String>> + Send;
}

/// `state.json`. Ids are kept as strings so older files still load.
#[derive(Default, Serialize, Deserialize)]
struct Store {
    /// When Rune first saw each cliphist id.
    #[serde(default)]
    seen: HashMap<String, i64>,
    /// Newest first.
    #[serde(default)]
    favorites: Vec<Favorite>,
}

#[derive(Serialize, Deserialize)]
struct Favorite {
    /// File name of the saved copy (before any image extension).
    key: String,
    /// The cliphist id it came from (hidden from history while favorited).
    source: String,
    preview: String,
    time: Option<i64>,
}

impl Favorite {
    fn file(&self) -> String {
        match image_ext(&self.preview) {
            Some(ext) => format!("{}.{ext}", self.key),
            None => self.key.clone(),
        }
    }
}

/// Clipboard history with times and favorites.
pub struct Clipboard<T> {
    tools: T,
    /// `state.json` and favorite files.
    data: PathBuf,
    /// Decoded images.
    cache: PathBuf,
    /// Serializes read-modify-write of `state.json`.
    lock: Mutex<()>,
}

impl<T: Tools> Clipboard<T> {
    pub fn new(tools: T, data: PathBuf, cache: PathBuf) -> Self {
        Self {
            tools,
            data,
            cache,
            lock: Mutex::new(()),
        }
    }

    /// Notes the time of new history entries and forgets vanished ones.
    pub async fn sync(&self) -> Result<(), String> {
        let history = self.tools.list().await?;
        let _guard = self.lock.lock().await;
        self.stamp(&history)?;
        Ok(())
    }

    /// Favorites first, then history newest first.
    pub async fn list(&self) -> Result<Vec<Entry>, String> {
        let history = self.tools.list().await?;
        let store = {
            let _guard = self.lock.lock().await;
            self.stamp(&history)?
        };
        let ids: HashSet<String> = history.iter().map(|(id, _)| id.to_string()).collect();
        prune_cache(&self.cache, &ids);

        let mut entries: Vec<Entry> = store
            .favorites
            .iter()
            .filter_map(|f| {
                Some(Entry {
                    id: Id::Favorite(f.key.parse().ok()?),
                    title: title(&f.preview),
                    time: f.time,
                    image: image_ext(&f.preview).map(|_| self.data.join(f.file())),
                })
            })
            .collect();
        let favorited: HashSet<&str> = store.favorites.iter().map(|f| f.source.as_str()).collect();
        for (id, preview) in &history {
            let key = id.to_string();
            if favorited.contains(key.as_str()) {
                continue;
            }
            let image = match image_ext(preview) {
                Some(ext) => self.thumbnail(*id, ext).await,
                None => None,
            };
            entries.push(Entry {
                id: Id::History(*id),
                title: title(preview),
                time: store.seen.get(&key).copied(),
                image,
            });
        }
        Ok(entries)
    }

    /// Puts an entry back on the clipboard.
    pub async fn copy(&self, id: Id) -> Result<(), String> {
        let data = match id {
            Id::History(id) => self.tools.decode(id).await?,
            Id::Favorite(key) => {
                let store = self.load()?;
                let fav = find(&store, key).ok_or("that favorite is gone")?;
                std::fs::read(self.data.join(fav.file())).map_err(|e| e.to_string())?
            }
        };
        self.tools.set(data).await
    }

    /// Favorites a history entry, or unfavorites a favorite.
    pub async fn toggle_favorite(&self, id: Id) -> Result<(), String> {
        match id {
            Id::Favorite(key) => {
                let _guard = self.lock.lock().await;
                let mut store = self.load()?;
                let Some(at) = store
                    .favorites
                    .iter()
                    .position(|f| f.key == key.to_string())
                else {
                    return Ok(()); // already gone
                };
                let fav = store.favorites.remove(at);
                self.save(&store)?;
                let _ = std::fs::remove_file(self.data.join(fav.file()));
            }
            Id::History(source) => {
                // Its preview comes from cliphist, never from the caller.
                let history = self.tools.list().await?;
                let preview = history
                    .into_iter()
                    .find(|(id, _)| *id == source)
                    .map(|(_, p)| p)
                    .ok_or("that entry left the history")?;
                let bytes = self.tools.decode(source).await?;
                let _guard = self.lock.lock().await;
                let mut store = self.load()?;
                let key = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis())
                    .to_string();
                let fav = Favorite {
                    key,
                    time: store.seen.get(&source.to_string()).copied(),
                    source: source.to_string(),
                    preview,
                };
                std::fs::create_dir_all(&self.data).map_err(|e| e.to_string())?;
                std::fs::write(self.data.join(fav.file()), bytes).map_err(|e| e.to_string())?;
                store.favorites.insert(0, fav);
                self.save(&store)?;
            }
        }
        Ok(())
    }

    /// Wipes the history. Favorites stay.
    pub async fn clear(&self) -> Result<(), String> {
        self.tools.wipe().await?;
        let _guard = self.lock.lock().await;
        let mut store = self.load()?;
        store.seen.clear();
        self.save(&store)?;
        let _ = std::fs::remove_dir_all(&self.cache);
        Ok(())
    }

    /// Stamps new ids with now and drops vanished ones. Caller holds the lock.
    fn stamp(&self, history: &[(u64, String)]) -> Result<Store, String> {
        let mut store = self.load()?;
        let ids: HashSet<String> = history.iter().map(|(id, _)| id.to_string()).collect();
        let before = store.seen.len();
        store.seen.retain(|id, _| ids.contains(id));
        let now = now();
        let mut changed = store.seen.len() != before;
        for id in ids {
            if let std::collections::hash_map::Entry::Vacant(slot) = store.seen.entry(id) {
                slot.insert(now);
                changed = true;
            }
        }
        if changed {
            self.save(&store)?;
        }
        Ok(store)
    }

    fn load(&self) -> Result<Store, String> {
        match std::fs::read_to_string(self.data.join("state.json")) {
            Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_else(|e| {
                warn!("clipboard state is corrupt, starting over: {e}");
                Store::default()
            })),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn save(&self, store: &Store) -> Result<(), String> {
        std::fs::create_dir_all(&self.data).map_err(|e| e.to_string())?;
        let json = serde_json::to_string(store).map_err(|e| e.to_string())?;
        // Write then rename, so a crash never leaves half a file (and lost favorites).
        let tmp = self.data.join("state.json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(tmp, self.data.join("state.json")).map_err(|e| e.to_string())
    }

    /// The decoded image for history entry `id`, cached on disk.
    async fn thumbnail(&self, id: u64, ext: &str) -> Option<PathBuf> {
        let path = self.cache.join(format!("{id}.{ext}"));
        if path.exists() {
            return Some(path);
        }
        let bytes = self.tools.decode(id).await.ok()?;
        std::fs::create_dir_all(&self.cache).ok()?;
        std::fs::write(&path, bytes).ok()?;
        Some(path)
    }
}

fn find(store: &Store, key: u64) -> Option<&Favorite> {
    store.favorites.iter().find(|f| f.key == key.to_string())
}

/// Drops cached images whose entries left the history.
fn prune_cache(cache: &Path, ids: &HashSet<String>) {
    let Ok(dir) = std::fs::read_dir(cache) else {
        return;
    };
    for file in dir.flatten() {
        let id = file
            .path()
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_owned);
        if !id.is_some_and(|id| ids.contains(&id)) {
            let _ = std::fs::remove_file(file.path());
        }
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// File extension for an image entry we can show, from cliphist's preview
/// (`[[ binary data 5 KiB png 128x128 ]]`; older versions say `image/png`).
/// iced picks the decoder by extension and panics on images it failed to
/// decode, so only formats it surely handles count.
fn image_ext(preview: &str) -> Option<&str> {
    preview
        .strip_prefix("[[ binary data ")?
        .split_whitespace()
        .map(|t| t.strip_prefix("image/").unwrap_or(t))
        .find(|t| ["png", "jpeg", "webp", "gif", "bmp", "tiff"].contains(t))
}

/// One display line: binary entries lose cliphist's `[[ binary data … ]]` wrapper.
fn title(preview: &str) -> String {
    match preview.strip_prefix("[[ binary data ") {
        Some(rest) => rest.trim_end_matches(" ]]").to_owned(),
        None => preview.chars().take(90).collect(),
    }
}

/// The real tools: `cliphist` and `wl-copy`.
pub struct Cliphist;

impl Tools for Cliphist {
    async fn list(&self) -> Result<Vec<(u64, String)>, String> {
        let out = Command::new("cliphist")
            .arg("list")
            .output()
            .await
            .map_err(|e| format!("cannot run cliphist: {e}"))?;
        Ok(parse(&String::from_utf8_lossy(&out.stdout)))
    }

    async fn decode(&self, id: u64) -> Result<Vec<u8>, String> {
        let mut child = Command::new("cliphist")
            .arg("decode")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run cliphist: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(id.to_string().as_bytes())
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(child
            .wait_with_output()
            .await
            .map_err(|e| e.to_string())?
            .stdout)
    }

    async fn wipe(&self) -> Result<(), String> {
        let status = Command::new("cliphist")
            .arg("wipe")
            .status()
            .await
            .map_err(|e| format!("cannot run cliphist: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("cliphist wipe failed ({status})"))
        }
    }

    async fn set(&self, data: Vec<u8>) -> Result<(), String> {
        let mut wl = Command::new("wl-copy")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run wl-copy: {e}"))?;
        if let Some(mut stdin) = wl.stdin.take() {
            stdin.write_all(&data).await.map_err(|e| e.to_string())?;
        }
        wl.wait().await.map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn parse(text: &str) -> Vec<(u64, String)> {
    text.lines()
        .filter_map(|l| l.split_once('\t'))
        .filter_map(|(id, preview)| Some((id.parse().ok()?, preview.trim().to_owned())))
        .collect()
}

fn xdg_dir(var: &str, fallback: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)))
        .map(|d| d.join("rune-shell/clipboard"))
}

impl Clipboard<Cliphist> {
    /// The real clipboard with the XDG data and cache folders.
    pub fn system() -> Result<Self, String> {
        let data = xdg_dir("XDG_DATA_HOME", ".local/share").ok_or("HOME is not set")?;
        let cache = xdg_dir("XDG_CACHE_HOME", ".cache").ok_or("HOME is not set")?;
        Ok(Self::new(Cliphist, data, cache))
    }
}

/// Starts history recording unless something already does it.
pub async fn ensure_watcher() {
    let running = Command::new("pgrep")
        .args(["-f", "wl-paste .*--watch cliphist"])
        .stdout(Stdio::null())
        .status()
        .await
        .is_ok_and(|s| s.success());
    if running {
        return;
    }
    for kind in ["text", "image"] {
        let spawned = Command::new("wl-paste")
            .args(["--type", kind, "--watch", "cliphist", "store"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match spawned {
            // Detached on purpose: history keeps recording if the shell restarts.
            Ok(child) => drop(child),
            Err(e) => {
                return warn!("cannot start clipboard history ({e}); is wl-clipboard installed?");
            }
        }
    }
    info!("started clipboard history recording");
}

const MARKER: &str = "rune-clip-stamp";

/// Notes the time of every new copy. Runs for the life of the shell.
pub async fn track(clipboard: std::sync::Arc<Clipboard<Cliphist>>) {
    // A crashed shell leaves its watcher behind; don't stack them up.
    let _ = Command::new("pkill").args(["-f", MARKER]).status().await;
    let spawned = Command::new("wl-paste")
        .args(["--watch", "sh", "-c", "cat >/dev/null; echo", MARKER])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => return warn!("cannot watch the clipboard ({e}); times will be approximate"),
    };
    let Some(stdout) = child.stdout.take() else {
        return;
    };
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(_)) = lines.next_line().await {
        // Give `cliphist store` a moment to add it first.
        tokio::time::sleep(Duration::from_millis(500)).await;
        if let Err(e) = clipboard.sync().await {
            warn!("clipboard: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// In-memory tools: a history list, entry contents, and what was set.
    #[derive(Default)]
    struct Fake {
        history: StdMutex<Vec<(u64, String, Vec<u8>)>>,
        set: StdMutex<Vec<Vec<u8>>>,
    }

    impl Fake {
        fn push(&self, id: u64, preview: &str, data: &[u8]) {
            let mut h = self.history.lock().unwrap();
            h.insert(0, (id, preview.into(), data.to_vec()));
        }
    }

    impl Tools for &Fake {
        async fn list(&self) -> Result<Vec<(u64, String)>, String> {
            Ok(self
                .history
                .lock()
                .unwrap()
                .iter()
                .map(|(i, p, _)| (*i, p.clone()))
                .collect())
        }
        async fn decode(&self, id: u64) -> Result<Vec<u8>, String> {
            let h = self.history.lock().unwrap();
            h.iter()
                .find(|e| e.0 == id)
                .map(|e| e.2.clone())
                .ok_or("no such id".into())
        }
        async fn wipe(&self) -> Result<(), String> {
            self.history.lock().unwrap().clear();
            Ok(())
        }
        async fn set(&self, data: Vec<u8>) -> Result<(), String> {
            self.set.lock().unwrap().push(data);
            Ok(())
        }
    }

    /// A fresh folder under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "rune-clip-test-{name}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn clipboard<'a>(fake: &'a Fake, dir: &TempDir) -> Clipboard<&'a Fake> {
        Clipboard::new(fake, dir.0.join("data"), dir.0.join("cache"))
    }

    fn run<F: Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn lists_with_times_titles_and_images() {
        let (fake, dir) = (Fake::default(), TempDir::new("list"));
        fake.push(1, "hello", b"hello");
        fake.push(2, "[[ binary data 5 KiB png 1x1 ]]", b"PNG");
        let c = clipboard(&fake, &dir);
        let entries = run(c.list()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, Id::History(2));
        assert_eq!(entries[0].title, "5 KiB png 1x1");
        let image = entries[0].image.clone().unwrap();
        assert_eq!(image.extension().unwrap(), "png");
        assert_eq!(std::fs::read(image).unwrap(), b"PNG");
        assert_eq!(entries[1].title, "hello");
        assert!(entries.iter().all(|e| e.time.is_some()));
        // Times stick on later listings.
        let again = run(c.list()).unwrap();
        assert_eq!(again[1].time, entries[1].time);
    }

    #[test]
    fn favorites_survive_a_wipe_and_hide_their_source() {
        let (fake, dir) = (Fake::default(), TempDir::new("fav"));
        fake.push(7, "keep me", b"keep me");
        fake.push(8, "other", b"other");
        let c = clipboard(&fake, &dir);
        run(c.list()).unwrap();
        run(c.toggle_favorite(Id::History(7))).unwrap();

        let entries = run(c.list()).unwrap();
        assert!(entries[0].is_favorite());
        assert_eq!(entries[0].title, "keep me");
        assert!(entries[0].time.is_some(), "keeps the original copy time");
        assert!(
            !entries.iter().any(|e| e.id == Id::History(7)),
            "source hidden"
        );

        run(c.clear()).unwrap();
        let entries = run(c.list()).unwrap();
        assert_eq!(entries.len(), 1, "only the favorite is left");
        run(c.copy(entries[0].id)).unwrap();
        assert_eq!(fake.set.lock().unwrap().last().unwrap(), b"keep me");

        run(c.toggle_favorite(entries[0].id)).unwrap();
        assert!(run(c.list()).unwrap().is_empty());
    }

    #[test]
    fn unfavoriting_brings_the_history_entry_back() {
        let (fake, dir) = (Fake::default(), TempDir::new("unfav"));
        fake.push(3, "x", b"x");
        let c = clipboard(&fake, &dir);
        run(c.toggle_favorite(Id::History(3))).unwrap();
        let fav = run(c.list()).unwrap()[0].id;
        run(c.toggle_favorite(fav)).unwrap();
        let entries = run(c.list()).unwrap();
        assert_eq!(
            entries.iter().map(|e| e.id).collect::<Vec<_>>(),
            [Id::History(3)]
        );
    }

    #[test]
    fn favoriting_a_vanished_entry_fails_cleanly() {
        let (fake, dir) = (Fake::default(), TempDir::new("gone"));
        let c = clipboard(&fake, &dir);
        assert!(run(c.toggle_favorite(Id::History(99))).is_err());
        assert!(run(c.copy(Id::Favorite(1))).is_err());
    }

    #[test]
    fn corrupt_state_starts_over() {
        let (fake, dir) = (Fake::default(), TempDir::new("corrupt"));
        fake.push(1, "a", b"a");
        std::fs::create_dir_all(dir.0.join("data")).unwrap();
        std::fs::write(dir.0.join("data/state.json"), "{not json").unwrap();
        let c = clipboard(&fake, &dir);
        assert_eq!(run(c.list()).unwrap().len(), 1);
    }

    #[test]
    fn parses_cliphist_list() {
        let entries =
            parse("12\thello world\n11\t[[ binary data image/png 3 KiB 64x64 ]]\nbroken\nx\ty\n");
        assert_eq!(
            entries,
            [
                (12, "hello world".into()),
                (11, "[[ binary data image/png 3 KiB 64x64 ]]".into())
            ]
        );
        assert_eq!(image_ext(&entries[1].1), Some("png"));
        assert_eq!(
            image_ext("[[ binary data 5 KiB png 128x128 ]]"),
            Some("png")
        );
        assert_eq!(image_ext("[[ binary data image/x-xpm 1 KiB ]]"), None);
        assert_eq!(image_ext("hello"), None);
    }
}

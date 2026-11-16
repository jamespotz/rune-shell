//! Clipboard history through `cliphist`. If nothing is recording history yet,
//! Rune starts the usual `wl-paste --watch cliphist store` watchers.
//!
//! cliphist keeps neither times nor favorites, so Rune adds a side store in
//! `$XDG_DATA_HOME/rune-shell/clipboard/`: `state.json` plus one file per
//! favorite (its own copy, so favorites survive a wipe or cliphist's size limit).
//! Image previews are decoded once into `$XDG_CACHE_HOME/rune-shell/clipboard/`.

use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::Mutex,
};
use tracing::{info, warn};

/// One history entry. Favorites have ids starting with `fav:`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    /// cliphist's one-line preview.
    pub preview: String,
    /// When it was copied (unix seconds), as far as Rune knows.
    pub time: Option<i64>,
    pub favorite: bool,
    /// The image itself, for image entries.
    pub image: Option<PathBuf>,
}

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
    /// File name of the saved copy.
    key: String,
    /// The cliphist id it came from (hidden from history while favorited).
    source: String,
    preview: String,
    time: Option<i64>,
}

/// Serializes read-modify-write of `state.json`.
static LOCK: Mutex<()> = Mutex::const_new(());
const MARKER: &str = "rune-clip-stamp";

fn home_dir(var: &str, fallback: &str) -> Result<PathBuf, String> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)))
        .map(|d| d.join("rune-shell/clipboard"))
        .ok_or_else(|| "HOME is not set".into())
}

fn data_dir() -> Result<PathBuf, String> {
    home_dir("XDG_DATA_HOME", ".local/share")
}

fn cache_dir() -> Result<PathBuf, String> {
    home_dir("XDG_CACHE_HOME", ".cache")
}

fn load() -> Result<Store, String> {
    match std::fs::read_to_string(data_dir()?.join("state.json")) {
        Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_else(|e| {
            warn!("clipboard state is corrupt, starting over: {e}");
            Store::default()
        })),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
        Err(e) => Err(e.to_string()),
    }
}

fn save(store: &Store) -> Result<(), String> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string(store).map_err(|e| e.to_string())?;
    // Write then rename, so a crash never leaves half a file (and lost favorites).
    let tmp = dir.join("state.json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, dir.join("state.json")).map_err(|e| e.to_string())
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

/// A favorite's file: images keep their extension so they can be shown.
fn fav_file(key: &str, preview: &str) -> String {
    match image_ext(preview) {
        Some(ext) => format!("{key}.{ext}"),
        None => key.to_owned(),
    }
}

/// Ids reach us from the UI and end up in file paths; they are always digits.
fn valid(id: &str) -> Result<&str, String> {
    if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) {
        Ok(id)
    } else {
        Err(format!("bad clipboard id {id:?}"))
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

/// Notes the time of every new copy. Runs for the life of the shell.
pub async fn track() {
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
        if let Err(e) = list().await {
            warn!("clipboard: {e}");
        }
    }
}

/// Favorites first, then history newest first.
pub async fn list() -> Result<Vec<Entry>, String> {
    let out = Command::new("cliphist")
        .arg("list")
        .output()
        .await
        .map_err(|e| format!("cannot run cliphist: {e}"))?;
    let history = parse(&String::from_utf8_lossy(&out.stdout));

    let _guard = LOCK.lock().await;
    let mut store = load()?;
    let ids: HashSet<&str> = history.iter().map(|(id, _)| id.as_str()).collect();
    store.seen.retain(|id, _| ids.contains(id.as_str()));
    let now = now();
    for (id, _) in &history {
        store.seen.entry(id.clone()).or_insert(now);
    }
    save(&store)?;

    let data = data_dir()?;
    let mut entries: Vec<Entry> = store
        .favorites
        .iter()
        .map(|f| Entry {
            id: format!("fav:{}", f.key),
            preview: f.preview.clone(),
            time: f.time,
            favorite: true,
            image: image_ext(&f.preview).map(|_| data.join(fav_file(&f.key, &f.preview))),
        })
        .collect();
    let favorited: HashSet<&str> = store.favorites.iter().map(|f| f.source.as_str()).collect();
    let cache = cache_dir()?;
    prune_cache(&cache, &ids);
    for (id, preview) in &history {
        if favorited.contains(id.as_str()) {
            continue;
        }
        let image = match image_ext(preview) {
            Some(ext) => thumbnail(&cache, id, ext).await,
            None => None,
        };
        entries.push(Entry {
            id: id.clone(),
            preview: preview.clone(),
            time: store.seen.get(id).copied(),
            favorite: false,
            image,
        });
    }
    Ok(entries)
}

fn parse(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(id, preview)| (id.to_owned(), preview.trim().to_owned()))
        .collect()
}

/// The decoded image for history entry `id`, cached on disk.
async fn thumbnail(cache: &std::path::Path, id: &str, ext: &str) -> Option<PathBuf> {
    let path = cache.join(format!("{id}.{ext}"));
    if path.exists() {
        return Some(path);
    }
    let bytes = decode(id).await.ok()?;
    std::fs::create_dir_all(cache).ok()?;
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

/// Drops cached images whose entries left the history.
fn prune_cache(cache: &std::path::Path, ids: &HashSet<&str>) {
    let Ok(dir) = std::fs::read_dir(cache) else {
        return;
    };
    for file in dir.flatten() {
        let id = file
            .path()
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_owned);
        if !id.is_some_and(|id| ids.contains(id.as_str())) {
            let _ = std::fs::remove_file(file.path());
        }
    }
}

/// The raw contents of history entry `id`.
async fn decode(id: &str) -> Result<Vec<u8>, String> {
    let mut decode = Command::new("cliphist")
        .arg("decode")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run cliphist: {e}"))?;
    if let Some(mut stdin) = decode.stdin.take() {
        stdin
            .write_all(valid(id)?.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(decode
        .wait_with_output()
        .await
        .map_err(|e| e.to_string())?
        .stdout)
}

/// Puts entry `id` (history or favorite) back on the clipboard.
pub async fn copy(id: &str) -> Result<(), String> {
    let data = match id.strip_prefix("fav:") {
        Some(key) => {
            let key = valid(key)?;
            let store = load()?;
            let fav = store
                .favorites
                .iter()
                .find(|f| f.key == key)
                .ok_or("that favorite is gone")?;
            std::fs::read(data_dir()?.join(fav_file(key, &fav.preview)))
                .map_err(|e| e.to_string())?
        }
        None => decode(id).await?,
    };
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

/// Favorites a history entry, or unfavorites a favorite.
pub async fn toggle_favorite(entry: &Entry) -> Result<(), String> {
    let dir = data_dir()?;
    let bytes = match entry.favorite {
        true => None,
        false => Some(decode(&entry.id).await?),
    };
    let _guard = LOCK.lock().await;
    let mut store = load()?;
    match bytes {
        None => {
            let key = valid(entry.id.strip_prefix("fav:").unwrap_or_default())?;
            store.favorites.retain(|f| f.key != key);
            save(&store)?;
            let _ = std::fs::remove_file(dir.join(fav_file(key, &entry.preview)));
        }
        Some(bytes) => {
            let millis = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis());
            let key = millis.to_string();
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::write(dir.join(fav_file(&key, &entry.preview)), bytes)
                .map_err(|e| e.to_string())?;
            store.favorites.insert(
                0,
                Favorite {
                    key,
                    source: entry.id.clone(),
                    preview: entry.preview.clone(),
                    time: entry.time,
                },
            );
            save(&store)?;
        }
    }
    Ok(())
}

/// Wipes the history. Favorites stay.
pub async fn clear() -> Result<(), String> {
    let status = Command::new("cliphist")
        .arg("wipe")
        .status()
        .await
        .map_err(|e| format!("cannot run cliphist: {e}"))?;
    if !status.success() {
        return Err(format!("cliphist wipe failed ({status})"));
    }
    let _guard = LOCK.lock().await;
    let mut store = load()?;
    store.seen.clear();
    save(&store)?;
    let _ = std::fs::remove_dir_all(cache_dir()?);
    Ok(())
}

/// Deletes entry `id` from history.
pub async fn delete(id: &str, preview: &str) -> Result<(), String> {
    let mut child = Command::new("cliphist")
        .arg("delete")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run cliphist: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(format!("{id}\t{preview}\n").as_bytes())
            .await
            .map_err(|e| e.to_string())?;
    }
    child.wait().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_cliphist_list() {
        let entries = super::parse(
            "12\thello world\n11\t[[ binary data image/png 3 KiB 64x64 ]]\nbroken line\n",
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], ("12".into(), "hello world".into()));
        assert_eq!(super::image_ext(&entries[1].1), Some("png"));
        assert_eq!(super::image_ext("[[ binary data 5 KiB png 128x128 ]]"), Some("png"));
        assert_eq!(
            super::image_ext("[[ binary data image/x-xpm 1 KiB ]]"),
            None
        );
        assert_eq!(super::image_ext("hello"), None);
    }

    #[test]
    fn ids_must_be_digits() {
        assert!(super::valid("123").is_ok());
        assert!(super::valid("../state.json").is_err());
        assert!(super::valid("").is_err());
    }
}

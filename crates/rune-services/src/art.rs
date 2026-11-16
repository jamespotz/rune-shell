//! Album art: local `file://` URLs are used directly; `http(s)://` art is
//! downloaded once with `curl` into `$XDG_CACHE_HOME/rune-shell/art/`.
//! (curl keeps TLS out of our binary, in line with "no linked C libraries".)

use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hash, Hasher},
    path::PathBuf,
    sync::Mutex,
};
use tokio::sync::mpsc;
use tracing::warn;

/// Local file for `url`, or `None` while a download is in flight (then `done`
/// gets a message when it finishes).
pub fn resolve(url: &str, done: &mpsc::UnboundedSender<()>) -> Option<PathBuf> {
    if let Some(path) = url.strip_prefix("file://") {
        let path = PathBuf::from(percent_decode(path));
        return path.exists().then_some(path);
    }
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return None;
    }
    let file = cache_dir()?.join(format!("{:016x}", hash(url)));
    if file.exists() {
        return Some(file);
    }
    static IN_FLIGHT: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    let fresh = IN_FLIGHT
        .lock()
        .is_ok_and(|mut set| set.get_or_insert_default().insert(url.to_owned()));
    if fresh {
        let (url, done) = (url.to_owned(), done.clone());
        tokio::spawn(async move {
            if download(&url, &file).await {
                let _ = done.send(());
            }
            // Leave failed URLs marked so a dead link isn't retried on every refresh.
        });
    }
    None
}

async fn download(url: &str, file: &std::path::Path) -> bool {
    let Some(dir) = file.parent() else {
        return false;
    };
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let tmp = file.with_extension("part");
    let status = tokio::process::Command::new("curl")
        .args([
            "--silent",
            "--fail",
            "--location",
            "--max-time",
            "15",
            "--max-filesize",
            "10000000",
            "--output",
        ])
        .arg(&tmp)
        .arg(url)
        .status()
        .await;
    match status {
        Ok(s) if s.success() => std::fs::rename(&tmp, file).is_ok(),
        Ok(_) => false,
        Err(e) => {
            warn!("cannot run curl for album art: {e}");
            false
        }
    }
}

fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("rune-shell/art"))
}

fn hash(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Decodes `%XX` escapes (file URLs encode spaces and non-ASCII this way).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_file_urls() {
        assert_eq!(
            percent_decode("/music/My%20Song%E2%99%AA.jpg"),
            "/music/My Song♪.jpg"
        );
        assert_eq!(percent_decode("/bad%zz%2"), "/bad%zz%2");
    }

    #[tokio::test]
    async fn resolves_local_files_and_ignores_unknown_schemes() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let file = std::env::temp_dir().join(format!("rune art {}.jpg", std::process::id()));
        std::fs::write(&file, "").unwrap();
        let url = format!("file://{}", file.display().to_string().replace(' ', "%20"));
        assert_eq!(resolve(&url, &tx), Some(file.clone()));
        assert_eq!(resolve("data:image/png;base64,AAAA", &tx), None);
        std::fs::remove_file(file).unwrap();
    }
}

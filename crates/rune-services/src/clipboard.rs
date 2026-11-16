//! Clipboard history through `cliphist`. If nothing is recording history yet,
//! Rune starts the usual `wl-paste --watch cliphist store` watchers.

use std::process::Stdio;
use tokio::{io::AsyncWriteExt, process::Command};
use tracing::{info, warn};

/// One history entry: cliphist's id and a one-line preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub preview: String,
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

/// Newest first.
pub async fn list() -> Result<Vec<Entry>, String> {
    let out = Command::new("cliphist")
        .arg("list")
        .output()
        .await
        .map_err(|e| format!("cannot run cliphist: {e}"))?;
    Ok(parse(&String::from_utf8_lossy(&out.stdout)))
}

fn parse(text: &str) -> Vec<Entry> {
    text.lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(id, preview)| Entry {
            id: id.to_owned(),
            preview: preview.trim().to_owned(),
        })
        .collect()
}

/// Puts entry `id` back on the clipboard.
pub async fn copy(id: &str) -> Result<(), String> {
    let mut decode = Command::new("cliphist")
        .arg("decode")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run cliphist: {e}"))?;
    if let Some(mut stdin) = decode.stdin.take() {
        stdin
            .write_all(id.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
    }
    let data = decode
        .wait_with_output()
        .await
        .map_err(|e| e.to_string())?
        .stdout;
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
        assert_eq!(
            (entries[0].id.as_str(), entries[0].preview.as_str()),
            ("12", "hello world")
        );
    }
}

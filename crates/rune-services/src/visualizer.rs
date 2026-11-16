//! Audio visualizer bars from `cava`, run only while music is playing.

use std::{path::PathBuf, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::watch,
};
use tracing::warn;

/// Number of bars cava produces; widgets downsample as needed.
pub const BARS: usize = 24;

const CONFIG: &str = "\
[general]
bars = 24
framerate = 30
[output]
method = raw
raw_target = /dev/stdout
data_format = ascii
ascii_max_range = 100
bar_delimiter = 59
frame_delimiter = 10
";

/// Spawns the visualizer. Bars are 0..=100; the list is empty while stopped.
pub fn spawn(mut playing: watch::Receiver<bool>) -> watch::Receiver<Vec<u8>> {
    let (tx, rx) = watch::channel(Vec::new());
    tokio::spawn(async move {
        let Some(config) = write_config() else { return };
        loop {
            if playing.wait_for(|p| *p).await.is_err() {
                return;
            }
            let child = Command::new("cava")
                .arg("-p")
                .arg(&config)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .spawn();
            let mut child = match child {
                Ok(c) => c,
                Err(e) => return warn!("cannot run cava ({e}); visualizer disabled"),
            };
            let Some(stdout) = child.stdout.take() else {
                return;
            };
            let mut lines = BufReader::new(stdout).lines();
            loop {
                tokio::select! {
                    line = lines.next_line() => match line {
                        Ok(Some(line)) => { let _ = tx.send(parse(&line)); }
                        _ => break, // cava exited
                    },
                    stopped = playing.wait_for(|p| !*p) => {
                        if stopped.is_err() { return; }
                        break;
                    }
                }
            }
            drop(child); // kill_on_drop stops cava
            let _ = tx.send(Vec::new());
            // Don't spin if cava keeps dying while still "playing".
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    rx
}

fn parse(line: &str) -> Vec<u8> {
    line.split(';')
        .filter_map(|v| v.trim().parse().ok())
        .collect()
}

fn write_config() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    let path = base.join("rune-shell/cava.conf");
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::write(&path, CONFIG)
        .inspect_err(|e| warn!("cannot write cava config: {e}"))
        .ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_cava_frames() {
        assert_eq!(super::parse("0;12;100;7;"), [0, 12, 100, 7]);
        assert!(super::parse("").is_empty());
    }
}

//! Screenshots (`grim`, `slurp`), screen recording (`gpu-screen-recorder`)
//! and text extraction (`tesseract`). Results are announced through whatever
//! notification daemon is running, and copied with `wl-copy`.

use crate::hyprland::Hyprland;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    time::Instant,
};
use tokio::{
    io::AsyncWriteExt,
    process::{Child, Command},
    sync::{mpsc, watch},
};
use tracing::warn;
use zbus::zvariant::Value;

/// What to capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The focused monitor.
    Screen,
    /// Every monitor (screenshots only).
    All,
    /// The focused window.
    Window,
    /// A region picked with the mouse.
    Selection,
}

impl Target {
    pub fn from_word(word: &str) -> Option<Self> {
        Some(match word {
            "screen" | "full" => Target::Screen,
            "all" => Target::All,
            "window" => Target::Window,
            "selection" | "region" | "area" => Target::Selection,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub screenshot_dir: PathBuf,
    pub copy: bool,
    pub record_dir: PathBuf,
    pub record_audio: bool,
    pub fps: u32,
    /// Tesseract language(s), e.g. `eng` or `eng+deu`.
    pub ocr_lang: String,
}

/// A recording in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    pub path: PathBuf,
    pub started: Instant,
}

/// Where a capture goes on screen, in the form each tool wants.
enum Area {
    Output(String),
    Everything,
    /// x, y, width, height
    Rect(i32, i32, i32, i32),
}

async fn area(target: Target, hypr: Option<&Hyprland>) -> Result<Area, String> {
    let err = |e: crate::hyprland::Error| e.to_string();
    match target {
        Target::All => Ok(Area::Everything),
        Target::Screen => match hypr {
            Some(h) => Ok(h
                .focused_monitor()
                .await
                .map_err(err)?
                .map_or(Area::Everything, Area::Output)),
            None => Ok(Area::Everything),
        },
        Target::Window => {
            let h = hypr.ok_or("window capture needs Hyprland")?;
            let (x, y, w, hgt) = h
                .active_window()
                .await
                .map_err(err)?
                .ok_or("no focused window")?;
            Ok(Area::Rect(x, y, w, hgt))
        }
        Target::Selection => {
            let out = Command::new("slurp")
                .args(["-f", "%x %y %w %h"])
                .output()
                .await
                .map_err(|e| format!("cannot run slurp: {e}"))?;
            if !out.status.success() {
                return Err("selection cancelled".into());
            }
            parse_rect(&String::from_utf8_lossy(&out.stdout))
                .ok_or_else(|| "unexpected slurp output".to_owned())
        }
    }
}

fn parse_rect(s: &str) -> Option<Area> {
    let v: Vec<i32> = s
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    match v[..] {
        [x, y, w, h] if w > 0 && h > 0 => Some(Area::Rect(x, y, w, h)),
        _ => None,
    }
}

fn grim_args(area: &Area) -> Vec<String> {
    match area {
        Area::Output(name) => vec!["-o".into(), name.clone()],
        Area::Everything => Vec::new(),
        Area::Rect(x, y, w, h) => vec!["-g".into(), format!("{x},{y} {w}x{h}")],
    }
}

fn file_name(dir: &Path, prefix: &str, ext: &str) -> PathBuf {
    dir.join(format!(
        "{prefix}_{}.{ext}",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    ))
}

/// Takes a screenshot; returns the saved file.
pub async fn screenshot(
    target: Target,
    opts: &Options,
    hypr: Option<&Hyprland>,
    conn: Option<&zbus::Connection>,
) -> Result<PathBuf, String> {
    let area = area(target, hypr).await?;
    tokio::fs::create_dir_all(&opts.screenshot_dir)
        .await
        .map_err(|e| format!("cannot create {}: {e}", opts.screenshot_dir.display()))?;
    let path = file_name(&opts.screenshot_dir, "Screenshot", "png");
    let status = Command::new("grim")
        .args(grim_args(&area))
        .arg(&path)
        .status()
        .await
        .map_err(|e| format!("cannot run grim: {e}"))?;
    if !status.success() {
        return Err("grim failed".into());
    }
    if opts.copy {
        let png = tokio::fs::read(&path)
            .await
            .map_err(|e| format!("{}: {e}", path.display()))?;
        copy(&png, "image/png").await;
    }
    let body = if opts.copy {
        format!("{} (copied)", path.display())
    } else {
        path.display().to_string()
    };
    notify(conn, "Screenshot saved", &body, Some(&path)).await;
    Ok(path)
}

/// Reads the text in a selected region and copies it.
pub async fn ocr(opts: &Options, conn: Option<&zbus::Connection>) -> Result<String, String> {
    let area = area(Target::Selection, None).await?;
    let grim = Command::new("grim")
        .args(grim_args(&area))
        .arg("-")
        .output()
        .await
        .map_err(|e| format!("cannot run grim: {e}"))?;
    if !grim.status.success() {
        return Err("grim failed".into());
    }
    let mut tess = Command::new("tesseract")
        .args(["stdin", "stdout", "-l", &opts.ocr_lang])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot run tesseract: {e}"))?;
    if let Some(mut stdin) = tess.stdin.take() {
        stdin
            .write_all(&grim.stdout)
            .await
            .map_err(|e| e.to_string())?;
    }
    let out = tess.wait_with_output().await.map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if text.is_empty() {
        notify(conn, "No text found", "Nothing readable in that area", None).await;
        return Ok(text);
    }
    copy(text.as_bytes(), "text/plain;charset=utf-8").await;
    let preview: String = text.chars().take(120).collect();
    notify(conn, "Text copied", &preview, None).await;
    Ok(text)
}

/// Recording control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordCommand {
    Start(Target, Options),
    Stop,
    /// Start if idle, stop if recording.
    Toggle(Target, Options),
}

/// Runs the recorder; at most one recording at a time.
pub fn spawn_recorder(
    hypr: Option<Hyprland>,
    conn: Option<zbus::Connection>,
) -> (
    watch::Receiver<Option<Recording>>,
    mpsc::UnboundedSender<RecordCommand>,
) {
    let (tx, rx) = watch::channel(None);
    let (cmd_tx, mut cmds) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut child: Option<Child> = None;
        loop {
            tokio::select! {
                cmd = cmds.recv() => {
                    let Some(cmd) = cmd else { return };
                    let recording = child.is_some();
                    match cmd {
                        RecordCommand::Stop | RecordCommand::Toggle(..) if recording => {
                            if let Some(c) = child.take() {
                                finish(c, &tx, conn.as_ref()).await;
                            }
                        }
                        RecordCommand::Start(target, opts) | RecordCommand::Toggle(target, opts) if !recording => {
                            match start(target, &opts, hypr.as_ref()).await {
                                Ok((c, path)) => {
                                    child = Some(c);
                                    tx.send_replace(Some(Recording { path, started: Instant::now() }));
                                }
                                Err(e) => notify(conn.as_ref(), "Recording failed", &e, None).await,
                            }
                        }
                        _ => {}
                    }
                }
                // The recorder exiting on its own means it failed.
                status = async {
                    match child.as_mut() {
                        Some(c) => c.wait().await,
                        None => std::future::pending().await,
                    }
                }, if child.is_some() => {
                    child = None;
                    tx.send_replace(None);
                    warn!("gpu-screen-recorder exited: {status:?}");
                    notify(conn.as_ref(), "Recording stopped", "The recorder exited unexpectedly", None).await;
                }
            }
        }
    });
    (rx, cmd_tx)
}

async fn start(
    target: Target,
    opts: &Options,
    hypr: Option<&Hyprland>,
) -> Result<(Child, PathBuf), String> {
    let area = area(
        if target == Target::All {
            Target::Screen
        } else {
            target
        },
        hypr,
    )
    .await?;
    tokio::fs::create_dir_all(&opts.record_dir)
        .await
        .map_err(|e| format!("cannot create {}: {e}", opts.record_dir.display()))?;
    let path = file_name(&opts.record_dir, "Recording", "mp4");
    let mut cmd = Command::new("gpu-screen-recorder");
    match area {
        Area::Output(name) => cmd.args(["-w", &name]),
        Area::Everything => cmd.args(["-w", "screen"]),
        Area::Rect(x, y, w, h) => {
            cmd.args(["-w", "region", "-region", &format!("{w}x{h}+{x}+{y}")])
        }
    };
    cmd.args(["-f", &opts.fps.to_string(), "-c", "mp4", "-o"])
        .arg(&path);
    if opts.record_audio {
        cmd.args(["-a", "default_output"]);
    }
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("cannot run gpu-screen-recorder: {e}"))?;
    Ok((child, path))
}

/// Stops with SIGINT (gpu-screen-recorder finalises the file on it) and reports.
async fn finish(
    mut child: Child,
    tx: &watch::Sender<Option<Recording>>,
    conn: Option<&zbus::Connection>,
) {
    let path = tx.borrow().as_ref().map(|r| r.path.clone());
    if let Some(pid) = child.id() {
        let _ = Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status()
            .await;
    }
    let _ = child.wait().await;
    tx.send_replace(None);
    if let Some(path) = path {
        notify(conn, "Recording saved", &path.display().to_string(), None).await;
    }
}

async fn copy(data: &[u8], mime: &str) {
    let child = Command::new("wl-copy")
        .args(["--type", mime])
        .stdin(Stdio::piped())
        .spawn();
    match child {
        Ok(mut c) => {
            if let Some(mut stdin) = c.stdin.take() {
                let _ = stdin.write_all(data).await;
            }
            let _ = c.wait().await; // wl-copy forks to keep serving the clipboard
        }
        Err(e) => warn!("cannot run wl-copy: {e}"),
    }
}

/// Sends a notification through whichever daemon is running (Rune's or another shell's).
async fn notify(conn: Option<&zbus::Connection>, summary: &str, body: &str, image: Option<&Path>) {
    let Some(conn) = conn else { return };
    let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
    let image = image.map(|p| p.display().to_string());
    if let Some(p) = &image {
        hints.insert("image-path", Value::from(p.as_str()));
    }
    let actions: Vec<&str> = Vec::new();
    let result = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &("Rune Shell", 0u32, "", summary, body, actions, hints, -1i32),
        )
        .await;
    if let Err(e) = result {
        warn!("cannot send notification: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_targets_and_regions() {
        assert_eq!(Target::from_word("window"), Some(Target::Window));
        assert_eq!(Target::from_word("area"), Some(Target::Selection));
        assert_eq!(Target::from_word("nope"), None);
        assert!(matches!(
            parse_rect("10 20 300 200\n"),
            Some(Area::Rect(10, 20, 300, 200))
        ));
        assert!(parse_rect("10 20 0 200").is_none());
        assert_eq!(grim_args(&Area::Rect(1, 2, 3, 4)), ["-g", "1,2 3x4"]);
        assert_eq!(grim_args(&Area::Output("DP-1".into())), ["-o", "DP-1"]);
    }

    #[test]
    fn names_files_by_time() {
        let p = file_name(Path::new("/s"), "Screenshot", "png");
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with("Screenshot_20") && name.ends_with(".png"),
            "{name}"
        );
    }
}

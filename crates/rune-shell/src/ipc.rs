//! Control socket at `$XDG_RUNTIME_DIR/rune-shell.sock`: one line in, one line out.

use anyhow::{Context, Result, bail};
use rune_services::{
    audio,
    capture::{RecordCommand, Target},
    lock, media, system,
};
use rune_ui::{Action, Panel, UiEvent};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{mpsc::UnboundedSender, watch},
};
use tracing::warn;

fn socket_path() -> Result<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    Ok(PathBuf::from(dir).join("rune-shell.sock"))
}

const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// Client side: sends `cmd` and returns the reply. `quit` returns once the daemon has exited.
pub fn send(cmd: &str) -> Result<String> {
    let mut stream = std::os::unix::net::UnixStream::connect(socket_path()?)
        .context("rune-shell is not running")?;
    stream.set_read_timeout(Some(REPLY_TIMEOUT))?;
    writeln!(stream, "{cmd}")?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut reply = String::new();
    stream
        .read_to_string(&mut reply)
        .with_context(|| format!("rune-shell did not finish `{cmd}` in time"))?;
    Ok(reply.trim_end().to_owned())
}

/// Daemon side: claims the socket, refusing if another shell answers on it.
pub fn bind() -> Result<UnixListener> {
    let path = socket_path()?;
    if std::os::unix::net::UnixStream::connect(&path).is_ok() {
        bail!("rune-shell is already running");
    }
    let _ = std::fs::remove_file(&path); // stale socket left by a crash
    UnixListener::bind(&path).with_context(|| format!("cannot listen on {}", path.display()))
}

pub fn remove_socket() {
    if let Ok(path) = socket_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// Service state the lock screen polls through `lock-media` and `lock-status`.
#[derive(Debug, Clone, Default)]
pub struct Feeds {
    pub media: Option<watch::Receiver<Option<media::Media>>>,
    pub network: Option<watch::Receiver<Option<system::Network>>>,
    pub volume: Option<watch::Receiver<Option<audio::Volume>>>,
}

/// Longest now-playing line hyprlock gets, in characters.
const MAX_MEDIA_CHARS: usize = 80;

impl Feeds {
    /// "Title · Artist" as Pango markup, or nothing when no track is playing or paused.
    fn now_playing(&self) -> String {
        let Some(media) = &self.media else {
            return String::new();
        };
        let media = media.borrow();
        let Some(m) = media
            .as_ref()
            .filter(|m| m.status != media::Status::Stopped)
        else {
            return String::new();
        };
        let line = match (m.title.trim(), m.artist.trim()) {
            ("", _) => return String::new(),
            (title, "") => title.to_owned(),
            (title, artist) => format!("{title} · {artist}"),
        };
        let line: String = line.chars().take(MAX_MEDIA_CHARS).collect();
        lock::markup(&line)
    }

    /// Material Symbols names for the network and volume, space-separated.
    fn status_icons(&self) -> String {
        let network = self.network.as_ref().map(|n| match n.borrow().as_ref() {
            Some(n) if n.connected && n.kind == system::NetKind::Wifi => {
                rune_ui::wifi_icon(n.strength.unwrap_or(0))
            }
            Some(n) if n.connected => "lan",
            _ => "signal_disconnected",
        });
        let volume =
            self.volume
                .as_ref()
                .and_then(|v| *v.borrow())
                .map(|v| match (v.muted, v.percent) {
                    (true, _) | (false, 0) => "volume_off",
                    (false, 1..50) => "volume_down",
                    _ => "volume_up",
                });
        [network, volume]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Answers commands until the process exits.
pub async fn serve(
    listener: UnixListener,
    config: PathBuf,
    ui: UnboundedSender<UiEvent>,
    actions: UnboundedSender<Action>,
    feeds: Feeds,
) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let (config, ui, actions, feeds) =
                    (config.clone(), ui.clone(), actions.clone(), feeds.clone());
                tokio::spawn(async move {
                    if let Err(e) = handle(stream, &config, &ui, &actions, &feeds).await {
                        warn!("control socket: {e}");
                    }
                });
            }
            Err(e) => {
                warn!("control socket accept: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await; // e.g. out of fds
            }
        }
    }
}

async fn handle(
    stream: UnixStream,
    config: &Path,
    ui: &UnboundedSender<UiEvent>,
    actions: &UnboundedSender<Action>,
    feeds: &Feeds,
) -> std::io::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut line = String::new();
    BufReader::new(read.take(4096)).read_line(&mut line).await?;
    if line.trim().is_empty() {
        return Ok(()); // a liveness probe from `bind`, not a command
    }
    let quitting = line.trim() == "quit";
    let reply = command(line.trim(), config, ui, actions, feeds);
    write.write_all(reply.as_bytes()).await?;
    if quitting && reply == "ok" {
        std::mem::forget(write);
    }
    Ok(())
}

fn command(
    cmd: &str,
    config: &Path,
    ui: &UnboundedSender<UiEvent>,
    actions: &UnboundedSender<Action>,
    feeds: &Feeds,
) -> String {
    let words: Vec<&str> = cmd.split_whitespace().collect();
    let sent = |event| match ui.send(event) {
        Ok(()) => "ok".to_owned(),
        Err(_) => "error: the shell is shutting down".to_owned(),
    };
    let send_action = |a| match actions.send(a) {
        Ok(()) => "ok".to_owned(),
        Err(_) => "error: the shell is shutting down".to_owned(),
    };
    // Captures read the current config file, so settings changes apply immediately.
    let act =
        |make: &dyn Fn(rune_services::capture::Options) -> Action| match crate::load_config(config)
        {
            Ok(c) => match actions.send(make(crate::capture_options(&c))) {
                Ok(()) => "ok".to_owned(),
                Err(_) => "error: the shell is shutting down".to_owned(),
            },
            Err(e) => format!("error: {e:#}"),
        };
    match words.as_slice() {
        ["ping"] => "pong".into(),
        ["quit"] => sent(UiEvent::Quit),
        ["reload"] => match crate::load_config(config) {
            Ok(c) => sent(UiEvent::Config(Box::new(c))),
            Err(e) => format!("error: {e:#}"),
        },
        ["toggle", "settings"] => sent(UiEvent::ToggleSettings),
        ["lock"] => match crate::load_config(config) {
            Ok(c) => send_action(Action::System(system::Command::Power {
                action: system::PowerAction::Lock,
                lock_command: c.lock_command(),
            })),
            Err(e) => format!("error: {e:#}"),
        },
        ["lock-media"] => feeds.now_playing(),
        ["lock-status"] => feeds.status_icons(),
        ["open", "bluetooth"] => sent(UiEvent::OpenBluetooth),
        ["screenshot"] => act(&|o| Action::Screenshot(Target::Screen, o)),
        ["screenshot", t] => match Target::from_word(t) {
            Some(t) => act(&|o| Action::Screenshot(t, o)),
            None => format!("error: unknown target {t:?} (try: screen, all, window, selection)"),
        },
        ["ocr"] => act(&Action::Ocr),
        ["record", "stop"] => match actions.send(Action::Record(RecordCommand::Stop)) {
            Ok(()) => "ok".into(),
            Err(_) => "error: the shell is shutting down".into(),
        },
        ["record"] => act(&|o| Action::Record(RecordCommand::Toggle(Target::Screen, o))),
        ["record", t] => match Target::from_word(t).filter(|t| *t != Target::All) {
            Some(t) => act(&|o| Action::Record(RecordCommand::Toggle(t, o))),
            None => format!("error: unknown target {t:?} (try: screen, window, selection, stop)"),
        },
        ["volume", rest @ ..] => match audio::Change::parse(rest) {
            Some(c) => send_action(Action::Audio(audio::Command::Volume(c))),
            None => {
                "error: try volume up|down|mute|set <0-100> (up/down take an optional step)".into()
            }
        },
        ["brightness", rest @ ..] => {
            match audio::Change::parse(rest).filter(|c| *c != audio::Change::ToggleMute) {
                Some(c) => send_action(Action::Audio(audio::Command::Brightness(c))),
                None => "error: try brightness up|down|set <0-100>".into(),
            }
        }
        ["game-mode"] | ["game-mode", "toggle"] => sent(UiEvent::GameMode(None)),
        ["game-mode", "on"] => sent(UiEvent::GameMode(Some(true))),
        ["game-mode", "off"] => sent(UiEvent::GameMode(Some(false))),
        ["toggle", name] => match Panel::from_name(name) {
            Some(panel) => sent(UiEvent::Toggle(panel)),
            None => {
                let names: Vec<_> = Panel::names().chain(["settings"]).collect();
                format!("error: unknown panel {name:?} (try: {})", names.join(", "))
            }
        },
        _ => format!("error: unknown command {cmd:?}, see rune-shell --help"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_commands() {
        let (ui, _rx) = tokio::sync::mpsc::unbounded_channel();
        let (actions, mut action_rx) = tokio::sync::mpsc::unbounded_channel();
        let feeds = Feeds::default();
        let dir = std::env::temp_dir().join(format!("rune-ipc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.toml");
        let bad = dir.join("bad.toml");
        std::fs::write(&good, "[bar]\nheight = 40\n").unwrap();
        std::fs::write(&bad, "[bar]\nheight = \"tall\"\n").unwrap();

        assert_eq!(command("ping", &good, &ui, &actions, &feeds), "pong");
        assert_eq!(command("reload", &good, &ui, &actions, &feeds), "ok");
        assert_eq!(
            command("reload", &dir.join("missing.toml"), &ui, &actions, &feeds),
            "ok"
        );
        assert!(command("reload", &bad, &ui, &actions, &feeds).starts_with("error: invalid"));
        assert_eq!(
            command("toggle settings", &good, &ui, &actions, &feeds),
            "ok"
        );
        assert_eq!(
            command("toggle launcher", &good, &ui, &actions, &feeds),
            "ok"
        );
        assert!(command("toggle dock", &good, &ui, &actions, &feeds).starts_with("error"));
        assert!(
            command("dance", &good, &ui, &actions, &feeds).starts_with("error: unknown command")
        );
        assert_eq!(
            command("screenshot window", &good, &ui, &actions, &feeds),
            "ok"
        );
        assert!(matches!(
            action_rx.try_recv(),
            Ok(Action::Screenshot(Target::Window, _))
        ));
        assert_eq!(command("record", &good, &ui, &actions, &feeds), "ok");
        assert!(matches!(
            action_rx.try_recv(),
            Ok(Action::Record(RecordCommand::Toggle(Target::Screen, _)))
        ));
        assert!(command("record all", &good, &ui, &actions, &feeds).starts_with("error"));
        assert_eq!(command("game-mode on", &good, &ui, &actions, &feeds), "ok");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lock_commands_report_media_and_status_safely() {
        let (ui, _rx) = tokio::sync::mpsc::unbounded_channel();
        let (actions, _action_rx) = tokio::sync::mpsc::unbounded_channel();
        let config = Path::new("/nonexistent/config.toml");
        let mut feeds = Feeds::default();
        assert_eq!(command("lock-media", config, &ui, &actions, &feeds), "");
        assert_eq!(command("lock-status", config, &ui, &actions, &feeds), "");

        let track = media::Media {
            player: "org.mpris.MediaPlayer2.test".into(),
            identity: "Test".into(),
            title: format!("<b>Evil</b> & \n{}", "x".repeat(500)),
            artist: "A".into(),
            album: String::new(),
            art: None,
            length: None,
            status: media::Status::Playing,
            position: Duration::ZERO,
            read_at: std::time::Instant::now(),
            track_id: None,
        };
        let (media_tx, media_rx) = watch::channel(Some(track));
        feeds.media = Some(media_rx);
        let line = command("lock-media", config, &ui, &actions, &feeds);
        assert!(line.starts_with("&lt;b&gt;Evil&lt;/b&gt; &amp; xxx"));
        assert!(!line.contains('\n') && !line.contains('<'));
        assert!(line.chars().count() < 2 * MAX_MEDIA_CHARS);
        media_tx.send_modify(|m| {
            if let Some(m) = m {
                m.status = media::Status::Stopped;
            }
        });
        assert_eq!(command("lock-media", config, &ui, &actions, &feeds), "");

        let (_net_tx, net_rx) = watch::channel(Some(system::Network {
            connected: true,
            online: true,
            kind: system::NetKind::Wifi,
            name: "Home".into(),
            strength: Some(90),
            wifi_enabled: true,
        }));
        let (_vol_tx, vol_rx) = watch::channel(Some(audio::Volume {
            percent: 20,
            muted: false,
        }));
        feeds.network = Some(net_rx);
        feeds.volume = Some(vol_rx);
        assert_eq!(
            command("lock-status", config, &ui, &actions, &feeds),
            "signal_wifi_4_bar volume_down"
        );
    }
}

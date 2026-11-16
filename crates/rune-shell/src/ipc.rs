//! Control socket at `$XDG_RUNTIME_DIR/rune-shell.sock`: one line in, one line out.

use anyhow::{Context, Result, bail};
use rune_services::capture::{RecordCommand, Target};
use rune_ui::{Action, UiEvent};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::mpsc::UnboundedSender,
};
use tracing::warn;

fn socket_path() -> Result<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    Ok(PathBuf::from(dir).join("rune-shell.sock"))
}

/// Client side: sends `cmd` and returns the reply.
pub fn send(cmd: &str) -> Result<String> {
    let mut stream = std::os::unix::net::UnixStream::connect(socket_path()?)
        .context("rune-shell is not running")?;
    writeln!(stream, "{cmd}")?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
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

/// Answers commands until the process exits.
pub async fn serve(
    listener: UnixListener,
    config: PathBuf,
    ui: UnboundedSender<UiEvent>,
    actions: UnboundedSender<Action>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let (config, ui, actions) = (config.clone(), ui.clone(), actions.clone());
                tokio::spawn(async move {
                    if let Err(e) = handle(stream, &config, &ui, &actions).await {
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
) -> std::io::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut line = String::new();
    BufReader::new(read.take(4096)).read_line(&mut line).await?;
    if line.trim().is_empty() {
        return Ok(()); // a liveness probe from `bind`, not a command
    }
    let reply = command(line.trim(), config, ui, actions);
    write.write_all(reply.as_bytes()).await
}

fn command(
    cmd: &str,
    config: &Path,
    ui: &UnboundedSender<UiEvent>,
    actions: &UnboundedSender<Action>,
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
        ["toggle", "launcher"] => sent(UiEvent::ToggleLauncher),
        ["toggle", "keybinds"] => sent(UiEvent::ToggleKeybinds),
        ["toggle", "media"] => sent(UiEvent::ToggleMedia),
        ["toggle", "notifications"] => sent(UiEvent::ToggleNotifications),
        ["toggle", "weather"] => sent(UiEvent::ToggleWeather),
        ["toggle", "calendar"] => sent(UiEvent::ToggleCalendar),
        ["toggle", "quick-settings"] => sent(UiEvent::ToggleQuick),
        ["toggle", "power"] => sent(UiEvent::TogglePower),
        ["toggle", "clipboard"] => sent(UiEvent::ToggleClipboard),
        ["lock"] => send_action(Action::Power(rune_services::system::PowerAction::Lock)),
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
        ["volume", rest @ ..] => match rune_services::audio::Change::parse(rest) {
            Some(c) => send_action(Action::Volume(c)),
            None => {
                "error: try volume up|down|mute|set <0-100> (up/down take an optional step)".into()
            }
        },
        ["brightness", rest @ ..] => match rune_services::audio::Change::parse(rest)
            .filter(|c| *c != rune_services::audio::Change::ToggleMute)
        {
            Some(c) => send_action(Action::Brightness(c)),
            None => "error: try brightness up|down|set <0-100>".into(),
        },
        ["game-mode"] | ["game-mode", "toggle"] => sent(UiEvent::GameMode(None)),
        ["game-mode", "on"] => sent(UiEvent::GameMode(Some(true))),
        ["game-mode", "off"] => sent(UiEvent::GameMode(Some(false))),
        ["toggle", panel] => {
            format!(
                "error: unknown panel {panel:?} (try: launcher, keybinds, media, notifications, weather, calendar, settings)"
            )
        }
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
        let dir = std::env::temp_dir().join(format!("rune-ipc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.toml");
        let bad = dir.join("bad.toml");
        std::fs::write(&good, "[bar]\nheight = 40\n").unwrap();
        std::fs::write(&bad, "[bar]\nheight = \"tall\"\n").unwrap();

        assert_eq!(command("ping", &good, &ui, &actions), "pong");
        assert_eq!(command("reload", &good, &ui, &actions), "ok");
        assert_eq!(
            command("reload", &dir.join("missing.toml"), &ui, &actions),
            "ok"
        );
        assert!(command("reload", &bad, &ui, &actions).starts_with("error: invalid"));
        assert_eq!(command("toggle settings", &good, &ui, &actions), "ok");
        assert_eq!(command("toggle launcher", &good, &ui, &actions), "ok");
        assert!(command("toggle dock", &good, &ui, &actions).starts_with("error"));
        assert!(command("dance", &good, &ui, &actions).starts_with("error: unknown command"));
        assert_eq!(command("screenshot window", &good, &ui, &actions), "ok");
        assert!(matches!(
            action_rx.try_recv(),
            Ok(Action::Screenshot(Target::Window, _))
        ));
        assert_eq!(command("record", &good, &ui, &actions), "ok");
        assert!(matches!(
            action_rx.try_recv(),
            Ok(Action::Record(RecordCommand::Toggle(Target::Screen, _)))
        ));
        assert!(command("record all", &good, &ui, &actions).starts_with("error"));
        assert_eq!(command("game-mode on", &good, &ui, &actions), "ok");
        std::fs::remove_dir_all(dir).unwrap();
    }
}

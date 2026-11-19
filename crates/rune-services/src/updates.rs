//! Pending package updates (dnf, Flatpak, firmware) and `topgrade` runs.

use std::{
    path::Path,
    process::{ExitStatus, Stdio},
    time::{Duration, SystemTime},
};
use tokio::{
    io::AsyncReadExt,
    sync::{mpsc, watch},
};
use tracing::warn;

const FIRST_CHECK: Duration = Duration::from_secs(30);
const INTERVAL: Duration = Duration::from_secs(60 * 60);
const TIMEOUT: Duration = Duration::from_secs(3 * 60);
const OUTPUT_LIMIT: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Check,
    /// Open `topgrade` in this terminal (`<terminal> -e topgrade --keep`).
    Run {
        terminal: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Updates {
    /// Only sources whose tool is installed, after the first check.
    pub sources: Vec<Source>,
    pub checked_at: Option<SystemTime>,
    pub checking: bool,
    pub run: Run,
    pub topgrade_installed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub kind: SourceKind,
    /// Pending updates, or why the check failed.
    pub pending: Result<u32, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Dnf,
    Flatpak,
    Firmware,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Run {
    #[default]
    Idle,
    Running,
    Failed(String),
}

pub fn spawn() -> (watch::Receiver<Updates>, mpsc::UnboundedSender<Command>) {
    let (state, rx) = watch::channel(Updates {
        topgrade_installed: on_path("topgrade"),
        ..Updates::default()
    });
    let (cmds, mut cmd_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut ticks =
            tokio::time::interval_at(tokio::time::Instant::now() + FIRST_CHECK, INTERVAL);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = ticks.tick() => start_check(&state),
                cmd = cmd_rx.recv() => match cmd {
                    Some(Command::Check) => start_check(&state),
                    Some(Command::Run { terminal }) => {
                        let mut words = terminal.split_whitespace();
                        let Some(program) = words.next() else {
                            fail_run(&state, "no terminal is configured (launcher.terminal)".into());
                            continue;
                        };
                        let args: Vec<&str> =
                            words.chain(["-e", "topgrade", "--keep"]).collect();
                        start_run(&state, program, &args);
                    }
                    None => {
                        warn!("updates: command channel closed; stopping");
                        return;
                    }
                },
            }
        }
    });
    (rx, cmds)
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

fn fail_run(state: &watch::Sender<Updates>, message: String) {
    warn!("updates: {message}");
    state.send_modify(|u| u.run = Run::Failed(message));
}

/// Starts a check unless one is already running.
fn start_check(state: &watch::Sender<Updates>) {
    let started = state.send_if_modified(|u| !std::mem::replace(&mut u.checking, true));
    if !started {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        let (dnf, flatpak, firmware) = tokio::join!(
            check(SourceKind::Dnf),
            check(SourceKind::Flatpak),
            check(SourceKind::Firmware),
        );
        state.send_modify(|u| {
            u.sources = [dnf, flatpak, firmware].into_iter().flatten().collect();
            u.checked_at = Some(SystemTime::now());
            u.checking = false;
        });
    });
}

/// `None` when the source's tool is not installed.
async fn check(kind: SourceKind) -> Option<Source> {
    let (program, args): (&str, &[&str]) = match kind {
        SourceKind::Dnf => ("dnf", &["check-update", "-q"]),
        SourceKind::Flatpak => (
            "flatpak",
            &["remote-ls", "--updates", "--columns=application"],
        ),
        SourceKind::Firmware => ("fwupdmgr", &["get-updates", "--json"]),
    };
    let pending = match capture(program, args, OUTPUT_LIMIT, TIMEOUT).await {
        Err(Capture::NotFound) => return None,
        Err(Capture::Failed(message)) => Err(message),
        Ok((status, stdout)) => match (kind, status.code()) {
            (SourceKind::Dnf, Some(0)) => Ok(0),
            (SourceKind::Dnf, Some(100)) => Ok(dnf_pending(&stdout)),
            (SourceKind::Flatpak, Some(0)) => Ok(flatpak_pending(&stdout)),
            // fwupdmgr exits 2 with "no updates" and may print no JSON then.
            (SourceKind::Firmware, Some(0 | 2)) if stdout.trim().is_empty() => Ok(0),
            (SourceKind::Firmware, Some(0 | 2)) => fwupd_pending(&stdout),
            _ => Err(format!("{program} {} exited with {status}", args.join(" "))),
        },
    };
    if let Err(message) = &pending {
        warn!("updates: {message}");
    }
    Some(Source { kind, pending })
}

#[derive(Debug, PartialEq, Eq)]
enum Capture {
    NotFound,
    Failed(String),
}

/// Runs `program`, keeping at most `limit` bytes of stdout and killing it after `timeout`.
async fn capture(
    program: &str,
    args: &[&str],
    limit: u64,
    timeout: Duration,
) -> Result<(ExitStatus, String), Capture> {
    let command = format!("{program} {}", args.join(" "));
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Capture::NotFound,
            _ => Capture::Failed(format!("{command}: {e}")),
        })?;
    let Some(stdout) = child.stdout.take() else {
        return Err(Capture::Failed(format!("{command}: no stdout pipe")));
    };
    let run = async {
        let mut bytes = Vec::new();
        stdout
            .take(limit)
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| Capture::Failed(format!("{command}: reading output: {e}")))?;
        // Past the cap, stop it rather than wait for it to finish writing.
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) >= limit {
            let _ = child.start_kill();
        }
        let status = child
            .wait()
            .await
            .map_err(|e| Capture::Failed(format!("{command}: {e}")))?;
        Ok((status, String::from_utf8_lossy(&bytes).into_owned()))
    };
    tokio::time::timeout(timeout, run)
        .await
        .map_err(|_| Capture::Failed(format!("{command} timed out after {}s", timeout.as_secs())))?
}

/// Spawns `program args` unless a run is already going; a check follows when it exits.
fn start_run(state: &watch::Sender<Updates>, program: &str, args: &[&str]) {
    let started = state.send_if_modified(|u| {
        let idle = u.run != Run::Running;
        if idle {
            u.run = Run::Running;
        }
        idle
    });
    if !started {
        return;
    }
    let command = format!("{program} {}", args.join(" "));
    let child = tokio::process::Command::new(Path::new(program))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => return fail_run(state, format!("could not start {command}: {e}")),
    };
    let state = state.clone();
    tokio::spawn(async move {
        let run = match child.wait().await {
            Ok(status) if status.success() => Run::Idle,
            Ok(status) => Run::Failed(match status.code() {
                Some(code) => format!("topgrade exited with status {code}"),
                None => format!("{command} was stopped by a signal"),
            }),
            Err(e) => Run::Failed(format!("waiting for {command}: {e}")),
        };
        if let Run::Failed(message) = &run {
            warn!("updates: {message}");
        }
        state.send_modify(|u| u.run = run);
        start_check(&state);
    });
}

fn count(items: impl Iterator) -> u32 {
    u32::try_from(items.count()).unwrap_or(u32::MAX)
}

/// dnf5 colors its output even when piped and ignores `NO_COLOR`.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // CSI: ESC [ params, ending in a letter.
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `dnf check-update` lines shaped `name.arch version repo`; the
/// "Obsoleting packages" section that may follow repeats packages.
fn dnf_pending(stdout: &str) -> u32 {
    count(
        stdout
            .lines()
            .map(strip_ansi)
            .take_while(|line| !line.starts_with("Obsoleting"))
            .filter(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                matches!(fields.as_slice(), [name, _, _] if name.contains('.') && !line.starts_with(' '))
            }),
    )
}

fn flatpak_pending(stdout: &str) -> u32 {
    count(stdout.lines().filter(|line| !line.trim().is_empty()))
}

#[derive(serde::Deserialize)]
struct Fwupd {
    #[serde(rename = "Devices")]
    devices: Vec<FwupdDevice>,
}

#[derive(serde::Deserialize)]
struct FwupdDevice {
    #[serde(rename = "Releases", default)]
    releases: Vec<serde_json::Value>,
}

fn fwupd_pending(json: &str) -> Result<u32, String> {
    let parsed: Fwupd = serde_json::from_str(json)
        .map_err(|e| format!("fwupdmgr get-updates printed unexpected JSON: {e}"))?;
    Ok(count(
        parsed.devices.iter().filter(|d| !d.releases.is_empty()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DNF: &str = "Upgrades (available for \x1b[1m\x1b[32mreinstall\x1b[0m, available for \x1b[1m\x1b[34mupgrade\x1b[0m)\n\
\x1b[1m\x1b[34mImageMagick.x86_64             1:7.1.2.36-1.fc45 updates\x1b[0m\n\
\x1b[1m\x1b[34mffmpeg.x86_64                  9.0.2-4.fc45      rpmfusion-free-updates\x1b[0m\n\
stress-ng.x86_64               0.22.02-1.fc45    updates\n\
\n\
Obsoleting packages\n\
grub2-tools.x86_64             1:2.12-9.fc45     updates\n\
    grub2-tools-efi.x86_64     1:2.12-8.fc45     @System\n";

    #[test]
    fn dnf_counts_package_lines_before_obsoletes() {
        assert_eq!(dnf_pending(DNF), 3);
    }

    #[test]
    fn dnf_ignores_empty_and_garbage() {
        assert_eq!(dnf_pending(""), 0);
        assert_eq!(
            dnf_pending("Last metadata expiration check: 0:10:00 ago\n\x1b[\x1b[\n"),
            0
        );
    }

    #[test]
    fn flatpak_counts_non_empty_lines() {
        assert_eq!(
            flatpak_pending("app.zen_browser.zen\n  \norg.gimp.GIMP\n"),
            2
        );
        assert_eq!(flatpak_pending(""), 0);
    }

    #[test]
    fn fwupd_counts_devices_with_releases() {
        assert_eq!(fwupd_pending(r#"{"Devices":[]}"#), Ok(0));
        let json = r#"{"Devices":[{"Name":"UEFI","Releases":[{"Version":"2"}]},{"Name":"SSD","Releases":[]},{"Name":"Pad"}]}"#;
        assert_eq!(fwupd_pending(json), Ok(1));
    }

    #[tokio::test]
    async fn capture_caps_endless_output() {
        let (_, out) = capture("sh", &["-c", "yes"], 1024, Duration::from_secs(5))
            .await
            .expect("capped output");
        assert!(out.len() <= 1024);
    }

    #[tokio::test]
    async fn capture_times_out_hung_commands() {
        let err = capture("sleep", &["5"], 1024, Duration::from_millis(100)).await;
        assert!(matches!(err, Err(Capture::Failed(m)) if m.contains("timed out")));
    }

    #[tokio::test]
    async fn capture_reports_missing_tools() {
        let err = capture("rune-missing-tool", &[], 1024, Duration::from_secs(1)).await;
        assert_eq!(err.unwrap_err(), Capture::NotFound);
    }

    #[tokio::test]
    async fn run_with_missing_terminal_fails_with_its_name() {
        let (state, rx) = watch::channel(Updates::default());
        start_run(&state, "rune-missing-terminal", &["-e", "topgrade"]);
        assert!(matches!(&rx.borrow().run, Run::Failed(m) if m.contains("rune-missing-terminal")));
    }

    #[tokio::test]
    async fn run_is_ignored_while_running() {
        let (state, rx) = watch::channel(Updates {
            run: Run::Running,
            ..Updates::default()
        });
        start_run(&state, "rune-missing-terminal", &[]);
        assert_eq!(rx.borrow().run, Run::Running);
    }

    #[tokio::test]
    async fn run_reports_exit_status() {
        let (state, mut rx) = watch::channel(Updates::default());
        start_run(&state, "sh", &["-c", "exit 3"]);
        let failed = rx
            .wait_for(|u| matches!(u.run, Run::Failed(_)))
            .await
            .expect("run finishes")
            .run
            .clone();
        assert_eq!(failed, Run::Failed("topgrade exited with status 3".into()));
    }

    #[test]
    fn fwupd_rejects_garbage() {
        assert!(fwupd_pending("No updatable devices").is_err());
        assert!(fwupd_pending(r#"{"Devices":5}"#).is_err());
    }
}

//! Thin async client for Hyprland's IPC sockets.
//!
//! `.socket.sock` takes one request per connection (`j/workspaces`, `dispatch ...`).
//! `.socket2.sock` streams `EVENT>>DATA` lines. Events are only used as a "something
//! changed" trigger; the actual state always comes from the JSON queries, so a
//! renamed or newly added event in a future Hyprland release cannot desync us.

use serde::{Deserialize, de::DeserializeOwned};
use std::{collections::HashSet, path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::watch,
    time::{sleep, timeout},
};
use tracing::{info, warn};

/// Errors from talking to Hyprland.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Hyprland is not running (HYPRLAND_INSTANCE_SIGNATURE or XDG_RUNTIME_DIR unset)")]
    NotRunning,
    #[error("Hyprland IPC: {0}")]
    Io(#[from] std::io::Error),
    #[error("unexpected Hyprland reply: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Hyprland rejected the request: {0}")]
    Rejected(String),
}

/// One entry of `hyprctl -j workspaces`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Workspace {
    pub id: i32,
    pub name: String,
    pub monitor: String,
    pub windows: u32,
    #[serde(rename = "lastwindowtitle")]
    pub last_window_title: String,
}

/// Workspace reference embedded in monitors and clients.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkspaceRef {
    pub id: i32,
    pub name: String,
}

/// One entry of `hyprctl -j monitors`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Monitor {
    pub name: String,
    pub focused: bool,
    #[serde(rename = "activeWorkspace")]
    pub active_workspace: WorkspaceRef,
}

#[derive(Debug, Deserialize)]
struct Client {
    address: String,
    workspace: WorkspaceRef,
}

/// Everything the bar needs from Hyprland.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HyprState {
    pub monitors: Vec<Monitor>,
    /// Sorted by id.
    pub workspaces: Vec<Workspace>,
    /// Workspace ids holding a window that asked for attention.
    pub urgent: HashSet<i32>,
}

impl HyprState {
    /// Monitor by connector name (`DP-1`), as reported by `wl_output.name`.
    pub fn monitor(&self, name: &str) -> Option<&Monitor> {
        self.monitors.iter().find(|m| m.name == name)
    }
}

/// Handle to one Hyprland instance. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Hyprland {
    dir: PathBuf,
}

impl Hyprland {
    /// Locates the sockets of the Hyprland instance this process runs under.
    pub fn from_env() -> Result<Self, Error> {
        let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").ok_or(Error::NotRunning)?;
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or(Error::NotRunning)?;
        Ok(Self::with_dir(
            PathBuf::from(runtime).join("hypr").join(sig),
        ))
    }

    /// Uses the sockets in `dir` (tests point this at a mock).
    pub fn with_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Sends one raw request to `.socket.sock` and returns the full reply.
    pub async fn request(&self, cmd: &str) -> Result<String, Error> {
        let mut stream = UnixStream::connect(self.dir.join(".socket.sock")).await?;
        stream.write_all(cmd.as_bytes()).await?;
        let mut reply = String::new();
        stream.read_to_string(&mut reply).await?;
        Ok(reply)
    }

    async fn query<T: DeserializeOwned>(&self, what: &str) -> Result<T, Error> {
        Ok(serde_json::from_str(
            &self.request(&format!("j/{what}")).await?,
        )?)
    }

    /// Runs a dispatcher. Since Hyprland 0.56 the payload is a Lua expression,
    /// e.g. `hl.dsp.focus({ workspace = 3 })`.
    pub async fn dispatch(&self, lua: &str) -> Result<(), Error> {
        let reply = self.request(&format!("dispatch {lua}")).await?;
        if reply.starts_with("error") {
            return Err(Error::Rejected(reply));
        }
        Ok(())
    }

    /// Switches the focused monitor to workspace `id`.
    pub async fn focus_workspace(&self, id: i32) -> Result<(), Error> {
        self.dispatch(&format!("hl.dsp.focus({{ workspace = {id} }})"))
            .await
    }

    /// Starts `command` through Hyprland, so it opens on the current workspace
    /// and outlives the shell.
    pub async fn exec(&self, command: &str) -> Result<(), Error> {
        self.dispatch(&format!("hl.dsp.exec_cmd({})", lua_string(command)))
            .await
    }

    /// Connector name of the focused monitor, e.g. `DP-1`.
    pub async fn focused_monitor(&self) -> Result<Option<String>, Error> {
        let monitors: Vec<Monitor> = self.query("monitors").await?;
        Ok(monitors.into_iter().find(|m| m.focused).map(|m| m.name))
    }

    /// Focused window as `(x, y, width, height)` in layout coordinates.
    pub async fn active_window(&self) -> Result<Option<(i32, i32, i32, i32)>, Error> {
        #[derive(Deserialize)]
        struct Active {
            at: Option<[i32; 2]>,
            size: Option<[i32; 2]>,
        }
        // With no focused window Hyprland answers `{}`.
        let w: Active = self.query("activewindow").await?;
        Ok(w.at
            .zip(w.size)
            .map(|([x, y], [width, height])| (x, y, width, height)))
    }

    /// Game mode: no animations, blur, shadows, gaps, rounding or transparency.
    /// Turning it off reloads the user's config, which restores all of them exactly.
    pub async fn game_mode(&self, on: bool) -> Result<(), Error> {
        if !on {
            return self.request("reload").await.map(drop);
        }
        self.eval(
            "hl.config({ animations = { enabled = false }, \
             decoration = { rounding = 0, active_opacity = 1.0, inactive_opacity = 1.0, blur = { enabled = false }, shadow = { enabled = false } }, \
             general = { gaps_in = 0, gaps_out = 0 } })",
        )
        .await
    }

    /// Runs a Lua snippet in Hyprland (e.g. a runtime `hl.layer_rule`).
    pub async fn eval(&self, lua: &str) -> Result<(), Error> {
        let reply = self.request(&format!("eval {lua}")).await?;
        if reply.starts_with("error") {
            return Err(Error::Rejected(reply));
        }
        Ok(())
    }

    /// Every keybind, with the description set in the Hyprland config.
    pub async fn binds(&self) -> Result<Vec<Bind>, Error> {
        let raw: Vec<RawBind> = self.query("binds").await?;
        Ok(raw.into_iter().map(Bind::from).collect())
    }

    /// Spawns the event-stream task and returns its state channel. It starts
    /// empty and reconnects every 2 s if Hyprland goes away or restarts.
    pub fn watch(self) -> watch::Receiver<HyprState> {
        let (tx, rx) = watch::channel(HyprState::default());
        tokio::spawn(async move {
            loop {
                match self.follow(&tx).await {
                    Ok(()) => info!("Hyprland closed the event socket"),
                    Err(e) => warn!("Hyprland event stream: {e}"),
                }
                if tx.is_closed() {
                    return;
                }
                sleep(Duration::from_secs(2)).await;
            }
        });
        rx
    }

    async fn follow(&self, tx: &watch::Sender<HyprState>) -> Result<(), Error> {
        let stream = UnixStream::connect(self.dir.join(".socket2.sock")).await?;
        let mut lines = BufReader::new(stream).lines();
        let mut urgent = HashSet::new();
        self.publish(tx, &mut urgent).await?;
        while let Some(line) = lines.next_line().await? {
            self.apply(&line, &mut urgent).await?;
            // One focus change emits several events; refresh once per burst.
            while let Ok(next) = timeout(Duration::from_millis(5), lines.next_line()).await {
                match next? {
                    Some(line) => self.apply(&line, &mut urgent).await?,
                    None => return Ok(()),
                }
            }
            self.publish(tx, &mut urgent).await?;
        }
        Ok(())
    }

    async fn apply(&self, line: &str, urgent: &mut HashSet<i32>) -> Result<(), Error> {
        if let Some(("urgent", addr)) = parse_event(line) {
            let clients: Vec<Client> = self.query("clients").await?;
            let addr = addr.trim_start_matches("0x");
            if let Some(c) = clients
                .iter()
                .find(|c| c.address.trim_start_matches("0x") == addr)
            {
                urgent.insert(c.workspace.id);
            }
        }
        Ok(())
    }

    async fn publish(
        &self,
        tx: &watch::Sender<HyprState>,
        urgent: &mut HashSet<i32>,
    ) -> Result<(), Error> {
        let (monitors, mut workspaces) = tokio::try_join!(
            self.query::<Vec<Monitor>>("monitors"),
            self.query::<Vec<Workspace>>("workspaces"),
        )?;
        workspaces.sort_by_key(|w| w.id);
        // Looking at an urgent workspace counts as handling it.
        urgent.retain(|id| !monitors.iter().any(|m| m.active_workspace.id == *id));
        let state = HyprState {
            monitors,
            workspaces,
            urgent: urgent.clone(),
        };
        tx.send_if_modified(|current| {
            let changed = *current != state;
            if changed {
                *current = state;
            }
            changed
        });
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct RawBind {
    modmask: u32,
    key: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    dispatcher: String,
    #[serde(default)]
    submap: String,
}

/// A keybind for the cheat sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bind {
    /// e.g. `SUPER + SHIFT + Q`.
    pub keys: String,
    /// The bind's description, or its dispatcher when it has none.
    pub description: String,
    /// Empty for the global keymap.
    pub submap: String,
}

impl From<RawBind> for Bind {
    fn from(b: RawBind) -> Self {
        // Bit order of Hyprland's modmask (wlr keyboard modifiers).
        const MODS: [(u32, &str); 6] = [
            (64, "SUPER"),
            (4, "CTRL"),
            (8, "ALT"),
            (1, "SHIFT"),
            (32, "MOD3"),
            (128, "MOD5"),
        ];
        let mut keys: Vec<&str> = MODS
            .iter()
            .filter(|(bit, _)| b.modmask & bit != 0)
            .map(|(_, n)| *n)
            .collect();
        keys.push(&b.key);
        let description = if b.description.is_empty() {
            // `HL.Dispatcher(exec_cmd)` -> `exec_cmd`
            b.dispatcher
                .rsplit('(')
                .next()
                .unwrap_or_default()
                .trim_end_matches(')')
                .to_owned()
        } else {
            b.description
        };
        Self {
            keys: keys.join(" + "),
            description,
            submap: b.submap,
        }
    }
}

/// Quotes `s` as a Lua long string (`[==[...]==]`) that `s` cannot terminate.
pub fn lua_string(s: &str) -> String {
    let mut eq = String::new();
    while s.contains(&format!("]{eq}]")) {
        eq.push('=');
    }
    format!("[{eq}[{s}]{eq}]")
}

/// Splits a `.socket2.sock` line into `(event, data)`.
pub fn parse_event(line: &str) -> Option<(&str, &str)> {
    line.split_once(">>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UnixListener;

    const MONITORS: &str =
        r#"[{"id":0,"name":"DP-1","focused":true,"activeWorkspace":{"id":2,"name":"2"}}]"#;
    const WORKSPACES: &str = r#"[
        {"id":2,"name":"2","monitor":"DP-1","windows":1,"lastwindowtitle":"Warframe","hasfullscreen":false},
        {"id":1,"name":"1","monitor":"DP-1","windows":4,"lastwindowtitle":"kitty"}]"#;
    const CLIENTS: &str = r#"[{"address":"0x55b1","workspace":{"id":1,"name":"1"}}]"#;

    #[test]
    fn parses_events() {
        assert_eq!(
            parse_event("workspacev2>>3,3"),
            Some(("workspacev2", "3,3"))
        );
        assert_eq!(
            parse_event("activewindow>>kitty,a>>b"),
            Some(("activewindow", "kitty,a>>b"))
        );
        assert_eq!(parse_event("garbage"), None);
    }

    #[test]
    fn quotes_lua_strings() {
        assert_eq!(lua_string("kitty -e btop"), "[[kitty -e btop]]");
        assert_eq!(lua_string("echo ]] x"), "[=[echo ]] x]=]");
        assert_eq!(lua_string("a ]] b ]=] c"), "[==[a ]] b ]=] c]==]");
    }

    #[test]
    fn formats_binds() {
        let raw: Vec<RawBind> = serde_json::from_str(
            r#"[{"modmask":72,"key":"r","description":"Reload Hyprland","dispatcher":"HL.Dispatcher(exec_cmd)","submap":""},
                {"modmask":65,"key":"Up","description":"","dispatcher":"HL.Dispatcher(move)","submap":"resize"}]"#,
        )
        .unwrap();
        let binds: Vec<Bind> = raw.into_iter().map(Bind::from).collect();
        assert_eq!(binds[0].keys, "SUPER + ALT + r");
        assert_eq!(binds[0].description, "Reload Hyprland");
        assert_eq!(binds[1].keys, "SUPER + SHIFT + Up");
        assert_eq!(binds[1].description, "move");
        assert_eq!(binds[1].submap, "resize");
    }

    #[test]
    fn parses_real_json() {
        let m: Vec<Monitor> = serde_json::from_str(MONITORS).unwrap();
        assert_eq!(m[0].active_workspace.id, 2);
        let w: Vec<Workspace> = serde_json::from_str(WORKSPACES).unwrap();
        assert_eq!(w[0].last_window_title, "Warframe");
    }

    /// Fake Hyprland: answers queries on `.socket.sock`, sends `events` on `.socket2.sock`.
    fn mock(events: &'static str) -> Hyprland {
        let dir = std::env::temp_dir().join(format!("rune-test-{}-{events:p}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let req = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let ev = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = req.accept().await.unwrap();
                let mut buf = [0u8; 256];
                let n = s.read(&mut buf).await.unwrap();
                let reply = match &buf[..n] {
                    b"j/monitors" => MONITORS,
                    b"j/workspaces" => WORKSPACES,
                    b"j/clients" => CLIENTS,
                    b"dispatch hl.dsp.focus({ workspace = 1 })" => "ok",
                    _ => "error: unknown",
                };
                s.write_all(reply.as_bytes()).await.unwrap();
            }
        });
        tokio::spawn(async move {
            let (mut s, _) = ev.accept().await.unwrap();
            s.write_all(events.as_bytes()).await.unwrap();
            sleep(Duration::from_secs(60)).await; // keep the stream open
        });
        Hyprland::with_dir(dir)
    }

    #[tokio::test]
    async fn dispatch_uses_lua_syntax() {
        let h = mock("");
        h.focus_workspace(1).await.unwrap();
        assert!(matches!(h.dispatch("bogus").await, Err(Error::Rejected(_))));
    }

    #[tokio::test]
    async fn watch_publishes_state_and_urgency() {
        let mut rx = mock("urgent>>55b1\n").watch();
        let state = rx.wait_for(|s| !s.urgent.is_empty()).await.unwrap().clone();
        assert_eq!(
            state.workspaces.iter().map(|w| w.id).collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(state.monitor("DP-1").unwrap().active_workspace.id, 2);
        assert!(state.urgent.contains(&1));
    }
}

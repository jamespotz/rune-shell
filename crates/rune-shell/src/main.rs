//! `rune-shell`: with no arguments starts the shell; with a command, sends it
//! to the running shell (for Hyprland keybinds) and prints the reply.

mod ipc;

use anyhow::{Context, Result};
use rune_services::{
    apps, audio, bluetooth, calendar, capture, clipboard, clock, equalizer,
    hyprland::Hyprland,
    icons::IconIndex,
    media, notifications, stats, system,
    theme::{self, Variant, backup::Backup, export},
    tray, visualizer, wallpaper, weather,
};
use rune_ui::config::{Colors, Rgba, ThemeSource};
use rune_ui::{Action, Config, Surface, UiEvent};
use std::{
    fmt::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{
    mpsc::{self, UnboundedSender},
    watch,
};
use tracing::{info, warn};

const USAGE: &str = "usage: rune-shell [COMMAND]

Without a command, starts the shell. Commands for the running shell:
  reload           re-read config.toml (also happens automatically on save)
  toggle PANEL     open or close a panel: {panels}
  open bluetooth   open settings on the Bluetooth page
  screenshot [screen|all|window|selection]
  record [screen|window|selection]   start, or stop if recording
  record stop
  ocr              copy the text in a selected area
  game-mode [on|off|toggle]
  lock             lock the screen
  volume up|down|mute|set N      (shows the OSD)
  brightness up|down|set N
  toggle settings  show or hide the settings window
  ping             check that the shell is running
  quit             stop the shell

Works without a running shell:
  restore-themes   put back every app theme file Rune changed";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => daemon(),
        // Works without a running shell: the escape hatch if theming ever goes wrong.
        Some("restore-themes") => {
            println!("{}", restore_themes());
            Ok(())
        }
        Some("-h" | "--help" | "help") => {
            let panels: Vec<_> = rune_ui::Panel::names().collect();
            println!("{}", USAGE.replace("{panels}", &panels.join(", ")));
            Ok(())
        }
        Some(_) => {
            let reply = ipc::send(&args.join(" "))?;
            println!("{reply}");
            if reply.starts_with("error") {
                std::process::exit(1);
            }
            Ok(())
        }
    }
}

#[cfg(target_env = "gnu")]
fn limit_malloc_arenas() {
    // SAFETY: mallopt takes plain integers, touches no Rust-owned memory, and is locked internally by glibc.
    if unsafe { libc::mallopt(libc::M_ARENA_MAX, 2) } == 0 {
        warn!("could not limit malloc arenas; memory use may be higher");
    }
}

#[cfg(not(target_env = "gnu"))]
fn limit_malloc_arenas() {}

fn daemon() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    limit_malloc_arenas();

    let path = config_path()?;
    let config = load_config(&path).unwrap_or_else(|e| {
        warn!("{e:#}; using the default config");
        Config::default()
    });

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let _rt = rt.enter(); // lets tokio::spawn work from this (the UI) thread
    let listener = ipc::bind()?;
    let (ui, ui_events) = mpsc::unbounded_channel();
    let (actions, mut action_rx) = mpsc::unbounded_channel();

    let hypr = Hyprland::from_env()
        .inspect_err(|e| warn!("{e}; workspaces disabled"))
        .ok();
    if let Some(h) = &hypr {
        forward(h.clone().watch(), ui.clone(), UiEvent::Hypr);
    }
    forward(clock::spawn(), ui.clone(), UiEvent::Clock);
    forward(stats::spawn(), ui.clone(), UiEvent::Stats);
    let media = rt.block_on(media::spawn()).map(|(state, handle)| {
        // The visualizer (cava) only runs while something is playing.
        let (playing_tx, playing) = watch::channel(false);
        let mut watch_state = state.clone();
        tokio::spawn(async move {
            loop {
                let playing = watch_state
                    .borrow_and_update()
                    .as_ref()
                    .is_some_and(|m| m.status == media::Status::Playing);
                playing_tx.send_if_modified(|p| std::mem::replace(p, playing) != playing);
                if watch_state.changed().await.is_err() {
                    return;
                }
            }
        });
        forward(visualizer::spawn(playing), ui.clone(), UiEvent::Bars);
        forward(state, ui.clone(), UiEvent::Media);
        handle
    });
    let save = save_config(path.clone());
    let hyprland = hypr.is_some();
    // Shared by the launcher, notifications and the tray (~0.3 s to build).
    let icons: Arc<IconIndex> = Arc::new(
        rt.block_on(tokio::task::spawn_blocking(IconIndex::build))
            .unwrap_or_default(),
    );
    let (notif_cmds, tray_handle, tray_items) = match rt.block_on(zbus::Connection::session()) {
        Ok(conn) => {
            let options = notifications::Options {
                timeout: Duration::from_millis(config.notifications.timeout_ms),
                history: config.notifications.history,
            };
            let (state, cmds) = rt.block_on(notifications::spawn(
                conn.clone(),
                options,
                config.notifications.dnd,
                icons.clone(),
            ));
            forward(state, ui.clone(), UiEvent::Notifications);
            let (items, handle) = rt.block_on(tray::spawn(conn, icons.clone()));
            forward(items.clone(), ui.clone(), UiEvent::Tray);
            (Some(cmds), Some(handle), Some(items))
        }
        Err(e) => {
            warn!("no session bus ({e}); notifications and tray disabled");
            (None, None, None)
        }
    };
    let theme_jobs = theme_worker(ui.clone());
    if config.bar.blur
        && let Some(h) = hypr.clone()
    {
        tokio::spawn(async move { h.eval(&blur_rule(true)).await });
    }
    let (weather_query, weather_rx) = watch::channel(None);
    let weather_refresh = Arc::new(tokio::sync::Notify::new());
    forward(
        weather::spawn(weather_rx, weather_refresh.clone()),
        ui.clone(),
        UiEvent::Weather,
    );
    let (bt_state, bt_cmds) = rt.block_on(bluetooth::spawn());
    forward(bt_state, ui.clone(), UiEvent::Bluetooth);
    let (calendar_urls, calendar_rx) = watch::channel(Vec::new());
    let calendar_refresh = Arc::new(tokio::sync::Notify::new());
    let calendar_state = calendar::spawn(calendar_rx, calendar_refresh.clone());
    let (remind_minutes, remind_rx) = watch::channel(config.calendar.remind_minutes);
    if let Some(cmds) = notif_cmds.clone() {
        calendar::spawn_reminders(calendar_state.clone(), remind_rx, move |n| {
            cmds.send(notifications::Command::Post(n)).is_ok()
        });
    }
    forward(calendar_state, ui.clone(), UiEvent::Calendar);
    let capture_conn = rt.block_on(zbus::Connection::session()).ok();
    let equalizer = Arc::new(equalizer::spawn(config.equalizer.bands));
    if config.equalizer.enabled {
        equalizer.send(equalizer::Command::Enable);
    }
    let (volume, audio_devices) = audio::spawn_volume(equalizer.output());
    forward(volume.clone(), ui.clone(), UiEvent::Volume);
    forward(audio_devices, ui.clone(), UiEvent::AudioDevices);
    let system_conn = rt.block_on(zbus::Connection::system()).ok();
    if let Some(system) = &system_conn {
        forward(
            system::spawn_network(system.clone()),
            ui.clone(),
            UiEvent::Network,
        );
        forward(
            system::spawn_battery(system.clone()),
            ui.clone(),
            UiEvent::Battery,
        );
        forward(
            system::spawn_power_profile(system.clone()),
            ui.clone(),
            UiEvent::PowerProfile,
        );
    }
    {
        let ui = ui.clone();
        tokio::spawn(async move {
            let _ = ui.send(UiEvent::Brightness(audio::read_brightness().await));
        });
    }
    tokio::spawn(clipboard::ensure_watcher());
    let clips = match clipboard::Clipboard::system() {
        Ok(c) => {
            let c = std::sync::Arc::new(c);
            tokio::spawn(clipboard::track(c.clone()));
            Some(c)
        }
        Err(e) => {
            warn!("clipboard history is off: {e}");
            None
        }
    };
    let system_controls = system::Controls::new(system_conn.clone(), hypr.clone());
    let audio_controls = audio::Controls::new(volume);
    let (recording, record_cmds) = capture::spawn_recorder(hypr.clone(), capture_conn.clone());
    forward(recording, ui.clone(), UiEvent::Recording);
    let actions_ui = ui.clone();
    let media_handle = media;
    let eq = equalizer.clone();
    tokio::spawn(async move {
        while let Some(action) = action_rx.recv().await {
            // Each action runs on its own task so a slow app scan never delays a click.
            let (hypr, icons, ui, media) = (
                hypr.clone(),
                icons.clone(),
                actions_ui.clone(),
                media_handle.clone(),
            );
            match action {
                Action::SaveConfig(config) => {
                    let _ = save.send(Some(*config)); // writer lives as long as the runtime
                }
                Action::Hypr(cmd) => {
                    let Some(h) = hypr else { continue };
                    spawn_handled("hyprland", ui, async move |reply| {
                        h.handle(cmd, reply).await
                    });
                }
                Action::System(cmd) => {
                    let c = system_controls.clone();
                    spawn_handled("system", ui, async move |reply| c.handle(cmd, reply).await);
                }
                Action::Audio(cmd) => {
                    let active = eq.output().borrow().is_some();
                    if let Some(output) = equalizer::retarget_for(&cmd, active) {
                        eq.send(equalizer::Command::Retarget(output.to_owned()));
                        continue;
                    }
                    let c = audio_controls.clone();
                    spawn_handled("audio", ui, async move |reply| c.handle(cmd, reply).await);
                }
                Action::Equalizer(cmd) => eq.send(cmd),
                Action::Clipboard(cmd) => {
                    let Some(c) = clips.clone() else { continue };
                    spawn_handled("clipboard", ui, async move |reply| {
                        c.handle(cmd, reply).await
                    });
                }
                Action::Launch(command) => {
                    tokio::spawn(launch(hypr, command));
                }
                Action::Weather(query) => {
                    weather_query.send_if_modified(|q| std::mem::replace(q, query) != query);
                }
                Action::RefreshWeather => weather_refresh.notify_one(),
                Action::Geocode(name) => {
                    tokio::spawn(async move {
                        let _ = ui.send(UiEvent::Geocoded(weather::geocode(&name).await));
                    });
                }
                Action::Bluetooth(cmd) => {
                    let _ = bt_cmds.send(cmd); // the service lives as long as the runtime
                }
                Action::Screenshot(target, opts) => {
                    let conn = capture_conn.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            capture::screenshot(target, &opts, hypr.as_ref(), conn.as_ref()).await
                        {
                            warn!("screenshot: {e}"); // includes a cancelled selection; not worth a popup
                        }
                    });
                }
                Action::Ocr(opts) => {
                    let conn = capture_conn.clone();
                    tokio::spawn(async move {
                        if let Err(e) = capture::ocr(&opts, conn.as_ref()).await {
                            warn!("text extraction: {e}");
                        }
                    });
                }
                Action::Calendar(calendar) => {
                    let urls = calendar.urls;
                    calendar_urls.send_if_modified(|u| {
                        let changed = *u != urls;
                        if changed {
                            *u = urls;
                        }
                        changed
                    });
                    remind_minutes.send_if_modified(|m| {
                        std::mem::replace(m, calendar.remind_minutes) != calendar.remind_minutes
                    });
                }
                Action::RefreshCalendar => calendar_refresh.notify_one(),
                Action::Record(cmd) => {
                    let _ = record_cmds.send(cmd); // the recorder lives as long as the runtime
                }
                Action::Notif(cmd) => {
                    if let Some(cmds) = &notif_cmds {
                        let _ = cmds.send(cmd); // the service lives as long as the runtime
                    }
                }
                Action::Tray(request) => {
                    let (Some(handle), Some(items)) = (tray_handle.clone(), tray_items.clone())
                    else {
                        continue;
                    };
                    tokio::spawn(tray_request(handle, items, request, ui));
                }
                Action::LoadApps => {
                    tokio::spawn(async move {
                        if let Ok(list) =
                            tokio::task::spawn_blocking(move || apps::scan(&icons)).await
                        {
                            let _ = ui.send(UiEvent::Apps(list));
                        }
                    });
                }
                Action::Media(control) => {
                    tokio::spawn(async move {
                        let Some(media) = media else { return };
                        if let Err(e) = media.control(control).await {
                            warn!("media {control:?}: {e}");
                        }
                    });
                }
                Action::ApplyTheme(config) => {
                    let _ = theme_jobs.send(Some(*config)); // worker lives as long as the runtime
                }
                Action::RestoreThemes => {
                    tokio::spawn(async move {
                        let status = tokio::task::spawn_blocking(restore_themes)
                            .await
                            .unwrap_or_default();
                        let _ = ui.send(UiEvent::Status(status));
                    });
                }
                Action::LoadWallpapers(dir) => {
                    tokio::spawn(load_wallpapers(dir, ui));
                }
                Action::SetWallpaper(path) => {
                    tokio::spawn(async move {
                        let result =
                            tokio::task::spawn_blocking(move || wallpaper::set(&path)).await;
                        if let Ok(Err(e)) = result {
                            warn!("wallpaper: {e}");
                            let _ = ui.send(UiEvent::Status(e));
                        }
                    });
                }
                Action::Blur(on) => {
                    tokio::spawn(async move {
                        let Some(h) = hypr else { return };
                        if let Err(e) = h.eval(&blur_rule(on)).await {
                            warn!("blur: {e}");
                        }
                    });
                }
            }
        }
    });
    tokio::spawn(ipc::serve(
        listener,
        path.clone(),
        ui.clone(),
        actions.clone(),
    ));
    let _watcher = watch_config(&path, ui)
        .inspect_err(|e| warn!("{e:#}; config hot reload disabled"))
        .ok();

    let result = rune_ui::run(config, ui_events, actions, hyprland);
    // Puts the default output back before the daemon is gone, so a restart starts clean.
    if rt
        .block_on(tokio::time::timeout(
            Duration::from_secs(3),
            equalizer.shutdown(),
        ))
        .is_err()
    {
        warn!("equalizer: shutdown timed out; the default output may need resetting");
    }
    ipc::remove_socket();
    drop(_rt);
    rt.shutdown_timeout(std::time::Duration::from_secs(1));
    Ok(result?)
}

/// Runs a service command on its own task, so a slow one never delays the next
/// action. Its replies go to the UI; a failure is logged under `what`.
fn spawn_handled<R, E, F>(
    what: &'static str,
    ui: UnboundedSender<UiEvent>,
    work: impl FnOnce(Box<dyn Fn(R) + Send + Sync>) -> F,
) where
    R: Into<UiEvent>,
    E: std::fmt::Display,
    F: Future<Output = std::result::Result<(), E>> + Send + 'static,
{
    let reply = Box::new(move |r: R| drop(ui.send(r.into())));
    let task = work(reply);
    tokio::spawn(async move {
        if let Err(e) = task.await {
            warn!("{what}: {e}");
        }
    });
}

/// Pushes every value of a service channel into the UI loop.
fn forward<T: Clone + Send + Sync + 'static>(
    mut rx: watch::Receiver<T>,
    ui: UnboundedSender<UiEvent>,
    wrap: fn(T) -> UiEvent,
) {
    tokio::spawn(async move {
        loop {
            let value = rx.borrow_and_update().clone();
            if ui.send(wrap(value)).is_err() || rx.changed().await.is_err() {
                return;
            }
        }
    });
}

/// `$XDG_CONFIG_HOME/rune-shell/config.toml`.
fn config_path() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?).join(".config"),
    };
    Ok(base.join("rune-shell").join("config.toml"))
}

/// Reads the config; a missing file means defaults.
pub(crate) fn load_config(path: &Path) -> Result<Config> {
    match std::fs::read_to_string(path) {
        Ok(text) => Config::parse(&text).with_context(|| format!("invalid {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            info!("no {}, using defaults", path.display());
            Ok(Config::default())
        }
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

/// Reloads the config whenever it is saved. Watches the directory because
/// editors save by replacing the file, which would end a watch on the file.
fn watch_config(path: &Path, ui: UnboundedSender<UiEvent>) -> Result<notify::RecommendedWatcher> {
    use notify::Watcher;
    let dir = path.parent().context("config path has no parent")?;
    std::fs::create_dir_all(dir)?;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let file = path.to_owned();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|e| e.paths.contains(&file)) {
            let _ = tx.send(());
        }
    })?;
    watcher.watch(dir, notify::RecursiveMode::NonRecursive)?;
    let path = path.to_owned();
    tokio::spawn(async move {
        while rx.recv().await.is_some() {
            // One save produces several events; settle, then reload once.
            tokio::time::sleep(Duration::from_millis(100)).await;
            while rx.try_recv().is_ok() {}
            match load_config(&path) {
                Ok(config) => {
                    info!("config reloaded");
                    if ui.send(UiEvent::Config(Box::new(config))).is_err() {
                        return;
                    }
                }
                Err(e) => warn!("{e:#}; keeping the previous config"),
            }
        }
    });
    Ok(watcher)
}

/// Writes configs from the settings window. Slider drags send many configs in
/// a row; only the latest is written, once things settle.
fn save_config(path: PathBuf) -> watch::Sender<Option<Config>> {
    let (tx, mut rx) = watch::channel(None::<Config>);
    tokio::spawn(async move {
        while rx.changed().await.is_ok() {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let Some(config) = rx.borrow_and_update().clone() else {
                continue;
            };
            let path = path.clone();
            let written =
                tokio::task::spawn_blocking(move || write_atomic(&path, &config.to_toml())).await;
            match written {
                Ok(Ok(())) => info!("config saved"),
                Ok(Err(e)) => warn!("cannot save config: {e:#}"),
                Err(e) => warn!("cannot save config: {e}"),
            }
        }
    });
    tx
}

/// Writes via a temp file + rename, so a crash or the file watcher never sees
/// a half-written config.
fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("cannot replace {}", path.display()))
}

/// Starts `command` detached: through Hyprland when available (so it lands on the
/// current workspace), otherwise as its own process group.
async fn launch(hypr: Option<Hyprland>, command: String) {
    info!("launch: {command}");
    if let Some(h) = hypr {
        if let Err(e) = h.exec(&command).await {
            warn!("launch {command:?}: {e}");
        }
        return;
    }
    use std::os::unix::process::CommandExt;
    let child = std::process::Command::new("sh")
        .args(["-c", &command])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn();
    match child {
        // Reap it when it exits so it doesn't linger as a zombie.
        Ok(mut child) => drop(tokio::task::spawn_blocking(move || child.wait())),
        Err(e) => warn!("launch {command:?}: {e}"),
    }
}

/// Applies themes one at a time; rapid clicks collapse to the latest choice.
fn theme_worker(ui: UnboundedSender<UiEvent>) -> watch::Sender<Option<Config>> {
    let (tx, mut rx) = watch::channel(None::<Config>);
    tokio::spawn(async move {
        while rx.changed().await.is_ok() {
            let Some(config) = rx.borrow_and_update().clone() else {
                continue;
            };
            match tokio::task::spawn_blocking(move || apply_theme(config)).await {
                Ok(Ok((config, status))) => {
                    info!("{status}");
                    let _ = ui.send(UiEvent::ThemeColors(config.colors));
                    let _ = ui.send(UiEvent::Status(status));
                }
                Ok(Err(e)) => {
                    warn!("theme: {e}");
                    let _ = ui.send(UiEvent::Status(e));
                }
                Err(e) => warn!("theme worker: {e}"),
            }
        }
    });
    tx
}

/// Builds the palette for `config.theme`, recolors the shell, themes the enabled apps.
fn apply_theme(mut config: Config) -> std::result::Result<(Config, String), String> {
    let t = &config.theme;
    let palette = match t.source {
        ThemeSource::Preset => theme::preset_for(&t.preset, theme::Appearance::from_dark(t.dark))
            .ok_or_else(|| format!("Unknown preset {:?}", t.preset))?,
        ThemeSource::Wallpaper => {
            let path = Some(PathBuf::from(&config.wallpaper.current))
                .filter(|p| !p.as_os_str().is_empty())
                .or_else(wallpaper::current)
                .ok_or("No wallpaper to take colors from; pick one first")?;
            let seed = theme::seed_from_image(&path)?;
            theme::from_seed(
                seed,
                Variant::from_id(&t.scheme).unwrap_or(Variant::TonalSpot),
                theme::Appearance::from_dark(t.dark),
            )
        }
    };
    let apps: Vec<export::App> = t
        .apps
        .iter()
        .filter_map(|a| export::App::from_id(a))
        .collect();
    let rgba = |c: theme::Rgb, a: u8| Rgba([c.0[0], c.0[1], c.0[2], a]);
    // Keep the notch's transparency so blur keeps working.
    let alpha = config.colors.background.0[3];
    config.colors = Colors {
        background: rgba(palette.bg_dim, alpha),
        foreground: rgba(palette.fg, 255),
        accent: rgba(palette.primary, 255),
        on_accent: rgba(palette.on_primary, 255),
        muted: rgba(palette.outline, 255),
        urgent: rgba(palette.error, 255),
    };
    let mut status = format!("Applied {}", palette.name);
    if !apps.is_empty() {
        let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set")?);
        let mut backup =
            Backup::open_default().map_err(|e| format!("Cannot open the theme backup: {e}"))?;
        let report = export::apply(&apps, &palette, &home, &mut backup);
        let (mut failed, mut notes) = (Vec::new(), Vec::new());
        for outcome in &report {
            match outcome {
                export::Outcome::Failed(message) => failed.push(message.as_str()),
                export::Outcome::Note(message) => notes.push(message.as_str()),
                export::Outcome::Done => {}
            }
        }
        if failed.is_empty() {
            let _ = write!(status, " to the shell and {} app(s)", apps.len());
        } else {
            let _ = write!(status, "; problems: {}", failed.join("; "));
        }
        if !notes.is_empty() {
            let _ = write!(status, "; {}", notes.join("; "));
        }
    }
    Ok((config, status))
}

fn restore_themes() -> String {
    match Backup::open_default() {
        Err(e) => format!("Cannot open the theme backup: {e}"),
        Ok(b) if b.is_empty() => {
            "Nothing to restore: Rune hasn't changed any app theme files".into()
        }
        Ok(b) => {
            let (done, failed) = b.restore();
            export::reload_running();
            if failed.is_empty() {
                format!(
                    "Restored {} file(s) and settings to how they were",
                    done.len()
                )
            } else {
                format!("Restored {}, but failed: {}", done.len(), failed.join("; "))
            }
        }
    }
}

/// Sends the folder listing right away, then again as thumbnails get made.
async fn load_wallpapers(dir: PathBuf, ui: UnboundedSender<UiEvent>) {
    let Ok(mut list) = tokio::task::spawn_blocking(move || wallpaper::list(&dir)).await else {
        return;
    };
    if ui.send(UiEvent::Wallpapers(list.clone())).is_err() {
        return;
    }
    let mut fresh = 0;
    for i in 0..list.len() {
        if list[i].thumb.is_some() {
            continue;
        }
        let path = list[i].path.clone();
        list[i].thumb = tokio::task::spawn_blocking(move || wallpaper::thumbnail(&path))
            .await
            .ok()
            .flatten();
        fresh += 1;
        if fresh % 3 == 0 && ui.send(UiEvent::Wallpapers(list.clone())).is_err() {
            return;
        }
    }
    if fresh > 0 {
        let _ = ui.send(UiEvent::Wallpapers(list));
    }
}

/// Hyprland layer rule (named, so re-sending it updates rather than duplicates).
fn blur_rule(on: bool) -> String {
    format!(
        "hl.layer_rule({{ name = \"rune-blur\", match = {{ namespace = \"^({})$\" }}, blur = {on}, ignore_alpha = 0.3 }})",
        [
            Surface::Bar(String::new()),
            Surface::Settings,
            Surface::Panel,
            Surface::Popups,
            Surface::Osd,
        ]
        .map(|surface| surface.namespace())
        .join("|"),
    )
}

/// Runs a tray click or menu request against the current item with that key.
async fn tray_request(
    handle: tray::TrayHandle,
    items: watch::Receiver<Vec<tray::TrayItem>>,
    request: rune_ui::TrayRequest,
    ui: UnboundedSender<UiEvent>,
) {
    use rune_ui::TrayRequest as R;
    let key = match &request {
        R::Activate { key, .. } | R::ContextMenu(key) | R::LoadMenu(key) | R::Click(key, _) => {
            key.clone()
        }
    };
    let Some(item) = items.borrow().iter().find(|i| i.key == key).cloned() else {
        return;
    };
    let result = match request {
        R::Activate { secondary, .. } => handle.activate(&item, secondary).await,
        R::ContextMenu(_) => handle.context_menu(&item).await,
        R::Click(_, id) => handle.click(&item, id).await,
        R::LoadMenu(_) => match handle.menu(&item).await {
            Ok(entries) => {
                let _ = ui.send(UiEvent::TrayMenu(key.clone(), entries));
                Ok(())
            }
            Err(e) => {
                let _ = ui.send(UiEvent::TrayMenu(key.clone(), Vec::new()));
                Err(e)
            }
        },
    };
    if let Err(e) = result {
        warn!("tray {key}: {e}");
    }
}

pub(crate) use rune_ui::capture_options;

#[cfg(test)]
mod tests {
    #[test]
    fn blur_rule_covers_every_frosted_surface() {
        let rule = super::blur_rule(true);
        for namespace in [
            "rune-bar",
            "rune-settings",
            "rune-panel",
            "rune-notifications",
            "rune-osd",
        ] {
            assert!(rule.contains(namespace), "{namespace}");
        }
    }
}

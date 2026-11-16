//! `rune-shell`: with no arguments starts the shell; with a command, sends it
//! to the running shell (for Hyprland keybinds) and prints the reply.

mod ipc;

use anyhow::{Context, Result};
use rune_services::{
    apps, audio, bluetooth, calendar, capture, clipboard, clock,
    hyprland::Hyprland,
    icons::IconIndex,
    media, notifications, system,
    theme::{self, Variant, backup::Backup, export},
    tray, visualizer, wallpaper, weather,
};
use rune_ui::config::{Colors, Rgba, ThemeSource};
use rune_ui::{Action, Config, UiEvent};
use std::{
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
  toggle launcher  open or close the app launcher
  toggle keybinds  open or close the keybind cheat sheet
  toggle media     open or close the music player
  toggle notifications  open or close the notification center
  toggle weather   open or close the forecast
  toggle calendar  open or close the calendar
  open bluetooth   open settings on the Bluetooth page
  screenshot [screen|all|window|selection]
  record [screen|window|selection]   start, or stop if recording
  record stop
  ocr              copy the text in a selected area
  game-mode [on|off|toggle]
  toggle quick-settings | toggle power | toggle clipboard
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
            println!("{USAGE}");
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

fn daemon() -> Result<()> {
    if std::env::var_os("ICED_BACKEND").is_none() {
        // Software rendering: ~15 MB and no idle CPU, against ~190 MB for the GPU path.
        // SAFETY: nothing else runs yet (no threads), so no one reads the environment concurrently.
        unsafe { std::env::set_var("ICED_BACKEND", "tiny-skia") };
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

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
    let theme_jobs = theme_worker(ui.clone(), save.clone());
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
    forward(
        calendar::spawn(calendar_rx, calendar_refresh.clone()),
        ui.clone(),
        UiEvent::Calendar,
    );
    let capture_conn = rt.block_on(zbus::Connection::session()).ok();
    let volume = audio::spawn_volume();
    forward(volume.clone(), ui.clone(), UiEvent::Volume);
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
    }
    {
        let ui = ui.clone();
        tokio::spawn(async move {
            let _ = ui.send(UiEvent::Brightness(audio::read_brightness().await));
        });
    }
    tokio::spawn(clipboard::ensure_watcher());
    let lock_command = config.power.lock_command.clone();
    let (recording, record_cmds) = capture::spawn_recorder(hypr.clone(), capture_conn.clone());
    forward(recording, ui.clone(), UiEvent::Recording);
    let actions_ui = ui.clone();
    let media_handle = media;
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
                Action::FocusWorkspace(id) => {
                    tokio::spawn(async move {
                        let Some(h) = hypr else { return };
                        if let Err(e) = h.focus_workspace(id).await {
                            warn!("focus workspace {id}: {e}");
                        }
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
                Action::Calendar(urls) => {
                    calendar_urls.send_if_modified(|u| std::mem::replace(u, urls.clone()) != urls);
                }
                Action::RefreshCalendar => calendar_refresh.notify_one(),
                Action::Wifi(on) => {
                    let Some(sys) = system_conn.clone() else {
                        continue;
                    };
                    tokio::spawn(async move {
                        if let Err(e) = system::set_wifi(&sys, on).await {
                            warn!("wi-fi: {e}");
                        }
                    });
                }
                Action::Power(action) => {
                    tokio::spawn(power(action, hypr, lock_command.clone()));
                }
                Action::LoadClipboard => {
                    tokio::spawn(async move {
                        match clipboard::list().await {
                            Ok(entries) => {
                                let _ = ui.send(UiEvent::Clipboard(entries));
                            }
                            Err(e) => warn!("clipboard: {e}"),
                        }
                    });
                }
                Action::ClipboardCopy(id) => {
                    tokio::spawn(async move {
                        if let Err(e) = clipboard::copy(&id).await {
                            warn!("clipboard: {e}");
                        }
                    });
                }
                Action::Volume(change) => {
                    let current = *volume.borrow();
                    tokio::spawn(async move {
                        if let Err(e) = audio::change_volume(change, current).await {
                            warn!("volume: {e}");
                        }
                    });
                }
                Action::Brightness(change) => {
                    tokio::spawn(async move {
                        match audio::change_brightness(change).await {
                            Ok(level) => {
                                let _ = ui.send(UiEvent::Brightness(level));
                            }
                            Err(e) => warn!("brightness: {e}"),
                        }
                    });
                }
                Action::Record(cmd) => {
                    let _ = record_cmds.send(cmd); // the recorder lives as long as the runtime
                }
                Action::GameMode(on) => {
                    tokio::spawn(async move {
                        let Some(h) = hypr else { return };
                        if let Err(e) = h.game_mode(on).await {
                            warn!("game mode: {e}");
                        }
                    });
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
                Action::LoadBinds => {
                    tokio::spawn(async move {
                        let Some(h) = hypr else { return };
                        match h.binds().await {
                            Ok(binds) => {
                                let _ = ui.send(UiEvent::Binds(binds));
                            }
                            Err(e) => warn!("keybinds: {e}"),
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
    ipc::remove_socket();
    Ok(result?)
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
fn theme_worker(
    ui: UnboundedSender<UiEvent>,
    save: watch::Sender<Option<Config>>,
) -> watch::Sender<Option<Config>> {
    let (tx, mut rx) = watch::channel(None::<Config>);
    tokio::spawn(async move {
        while rx.changed().await.is_ok() {
            let Some(config) = rx.borrow_and_update().clone() else {
                continue;
            };
            match tokio::task::spawn_blocking(move || apply_theme(config)).await {
                Ok(Ok((config, status))) => {
                    info!("{status}");
                    let _ = save.send(Some(config.clone()));
                    let _ = ui.send(UiEvent::Config(Box::new(config)));
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
        ThemeSource::Preset => theme::preset_named(&t.preset)
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
                t.dark,
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
        let failed: Vec<&String> = report.iter().filter(|r| !r.ends_with(": done")).collect();
        status += &if failed.is_empty() {
            format!(" to the shell and {} app(s)", apps.len())
        } else {
            format!(
                "; problems: {}",
                failed
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        };
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
        "hl.layer_rule({{ name = \"rune-blur\", match = {{ namespace = \"^rune-(bar|settings)$\" }}, blur = {on}, ignore_alpha = 0.3 }})"
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

/// Runs a power-menu action.
async fn power(action: system::PowerAction, hypr: Option<Hyprland>, lock_command: String) {
    use system::PowerAction as P;
    let run = |program: &str, args: &[&str]| {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args);
        cmd
    };
    let result = match action {
        P::Lock => run("sh", &["-c", &lock_command])
            .status()
            .await
            .map(drop)
            .map_err(|e| e.to_string()),
        P::Logout => match hypr {
            Some(h) => h.dispatch("hl.dsp.exit()").await.map_err(|e| e.to_string()),
            None => Err("logging out needs Hyprland".into()),
        },
        P::Suspend => run("systemctl", &["suspend"])
            .status()
            .await
            .map(drop)
            .map_err(|e| e.to_string()),
        P::Reboot => run("systemctl", &["reboot"])
            .status()
            .await
            .map(drop)
            .map_err(|e| e.to_string()),
        P::Shutdown => run("systemctl", &["poweroff"])
            .status()
            .await
            .map(drop)
            .map_err(|e| e.to_string()),
    };
    if let Err(e) = result {
        warn!("power {action:?}: {e}");
    }
}

//! Launchable things: `.desktop` applications and installed Steam games.

use crate::icons::IconIndex;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

/// One launcher entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// Desktop file id (`org.mozilla.firefox.desktop`) or `steam:<appid>`.
    pub id: String,
    pub name: String,
    pub comment: String,
    pub keywords: Vec<String>,
    /// Shell command with desktop field codes removed.
    pub command: String,
    pub terminal: bool,
    pub icon: Option<PathBuf>,
    pub steam: bool,
}

/// Parsed `[Desktop Entry]` group, before icon resolution.
#[derive(Debug, Default, PartialEq, Eq)]
struct Entry {
    name: String,
    comment: String,
    keywords: Vec<String>,
    exec: String,
    try_exec: String,
    icon: String,
    terminal: bool,
}

/// Every visible application and installed Steam game, sorted by name.
/// Reads the disk; call it from a blocking thread.
pub fn scan(icons: &IconIndex) -> Vec<App> {
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in crate::icons::data_dirs()
        .iter()
        .map(|d| d.join("applications"))
    {
        for (id, path) in desktop_files(&dir) {
            // Earlier data dirs override later ones, also to hide an entry.
            if !seen.insert(id.clone()) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some(e) = parse_desktop(&text) else {
                continue;
            };
            if !e.try_exec.is_empty() && which(&e.try_exec).is_none() {
                continue;
            }
            apps.push(App {
                id,
                command: clean_exec(&e.exec),
                icon: icons.lookup(&e.icon),
                name: e.name,
                comment: e.comment,
                keywords: e.keywords,
                terminal: e.terminal,
                steam: false,
            });
        }
    }
    for game in steam_games() {
        let marker = format!("steam://rungameid/{}", game.id.trim_start_matches("steam:"));
        if !apps.iter().any(|a| a.command.contains(&marker)) {
            apps.push(game);
        }
    }
    apps.sort_by_cached_key(|a| a.name.to_lowercase());
    apps
}

/// `(desktop id, path)` for every `.desktop` file under `dir`; ids use `-` for subdirectories.
fn desktop_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                stack.push((path, format!("{prefix}{name}-")));
            } else if name.ends_with(".desktop") {
                out.push((format!("{prefix}{name}"), path));
            }
        }
    }
    out
}

/// Parses a desktop entry; `None` for hidden or non-application entries.
fn parse_desktop(text: &str) -> Option<Entry> {
    let mut e = Entry::default();
    let mut in_main = false;
    let mut kind = "";
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| in_main) else {
            continue;
        };
        let value = unescape(value.trim());
        match key.trim() {
            "Type" => {
                kind = if value == "Application" {
                    "app"
                } else {
                    "other"
                }
            }
            "Name" => e.name = value,
            "Comment" => e.comment = value,
            "Keywords" => {
                e.keywords = value
                    .split(';')
                    .filter(|k| !k.is_empty())
                    .map(str::to_owned)
                    .collect()
            }
            "Exec" => e.exec = value,
            "TryExec" => e.try_exec = value,
            "Icon" => e.icon = value,
            "Terminal" => e.terminal = value == "true",
            "NoDisplay" | "Hidden" if value == "true" => return None,
            _ => {}
        }
    }
    (kind == "app" && !e.name.is_empty() && !e.exec.is_empty()).then_some(e)
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Removes desktop-spec field codes (`%u`, `%F`, ...) and turns `%%` into `%`.
pub fn clean_exec(exec: &str) -> String {
    let mut out = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            // `%%` is a literal percent; any other field code is dropped.
            if chars.next() == Some('%') {
                out.push('%');
            }
        } else {
            out.push(c);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Finds `program` (a name or a path) on `$PATH`.
fn which(program: &str) -> Option<PathBuf> {
    let p = Path::new(program);
    if p.is_absolute() {
        return p.exists().then(|| p.to_path_buf());
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(program))
        .find(|f| f.exists())
}

/// Installed games from every Steam library (native and Flatpak installs).
fn steam_games() -> Vec<App> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    let installs = [
        (home.join(".local/share/Steam"), "steam"),
        (
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
            "flatpak run com.valvesoftware.Steam",
        ),
    ];
    let mut games = Vec::new();
    for (root, launcher) in installs {
        let Ok(vdf) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) else {
            continue;
        };
        for library in vdf_values(&vdf, "path") {
            let Ok(entries) = std::fs::read_dir(Path::new(&library).join("steamapps")) else {
                continue;
            };
            for entry in entries.flatten() {
                let file = entry.file_name().to_string_lossy().into_owned();
                if !(file.starts_with("appmanifest_") && file.ends_with(".acf")) {
                    continue;
                }
                let Ok(acf) = std::fs::read_to_string(entry.path()) else {
                    continue;
                };
                if let Some(game) = steam_game(&acf, &root, launcher) {
                    games.push(game);
                }
            }
        }
    }
    games
}

fn steam_game(acf: &str, root: &Path, launcher: &str) -> Option<App> {
    let id = vdf_values(acf, "appid").into_iter().next()?;
    let name = vdf_values(acf, "name").into_iter().next()?;
    // shortcut: tools are filtered by name; switch to appinfo.vdf types if one slips through.
    let tools = ["Proton", "Steam Linux Runtime", "Steamworks Common"];
    if tools.iter().any(|t| name.starts_with(t)) {
        return None;
    }
    // Steam caches a 32x32 client icon as `librarycache/<appid>/<hash>.jpg`.
    let icon = std::fs::read_dir(root.join("appcache/librarycache").join(&id))
        .ok()
        .and_then(|dir| {
            dir.flatten()
                .map(|e| e.path())
                // Only the hash-named icon: art like `library_600x900.jpg` is often PNG
                // data, which iced fails to decode as JPEG (and then panics on redraw).
                .find(|p| {
                    p.extension().is_some_and(|x| x == "jpg")
                        && p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| {
                            s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
                        })
                })
        });
    Some(App {
        command: format!("{launcher} steam://rungameid/{id}"),
        id: format!("steam:{id}"),
        name,
        comment: "Steam game".into(),
        keywords: vec!["game".into(), "steam".into()],
        terminal: false,
        icon,
        steam: true,
    })
}

/// Values of every `"key"  "value"` line in Valve's KeyValues text format.
fn vdf_values(text: &str, key: &str) -> Vec<String> {
    let quoted = format!("\"{key}\"");
    text.lines()
        .filter_map(|l| l.trim().strip_prefix(&quoted))
        .map(|rest| rest.trim().trim_matches('"').replace("\\\\", "\\"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_desktop_entry() {
        let e = parse_desktop(
            "[Desktop Entry]\nType=Application\nName=Firefox\nName[de]=Feuerfuchs\nComment=Browse\\sthe web\n\
             Keywords=web;browser;\nExec=firefox %u\nIcon=firefox\n[Desktop Action new]\nName=New Window\n",
        )
        .unwrap();
        assert_eq!(e.name, "Firefox");
        assert_eq!(e.comment, "Browse the web");
        assert_eq!(e.keywords, ["web", "browser"]);
        assert_eq!(clean_exec(&e.exec), "firefox");
    }

    #[test]
    fn skips_hidden_and_non_apps() {
        assert!(
            parse_desktop("[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true")
                .is_none()
        );
        assert!(parse_desktop("[Desktop Entry]\nType=Link\nName=X\nURL=http://x").is_none());
        assert!(parse_desktop("[Desktop Entry]\nType=Application\nName=X").is_none());
    }

    #[test]
    fn cleans_field_codes() {
        assert_eq!(clean_exec("app --flag %F"), "app --flag");
        assert_eq!(clean_exec("printf 100%% %i"), "printf 100%");
        assert_eq!(clean_exec("sh -c 'echo hi' %U"), "sh -c 'echo hi'");
    }

    #[test]
    fn reads_steam_manifests() {
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"230410\"\n\t\"name\"\t\t\"Warframe\"\n}";
        let game = steam_game(acf, Path::new("/nonexistent"), "steam").unwrap();
        assert_eq!(game.command, "steam steam://rungameid/230410");
        assert_eq!(game.icon, None); // no cached art is fine
        assert_eq!(vdf_values(acf, "name"), ["Warframe"]);
        let tool = "\"appid\" \"1493710\"\n\"name\" \"Proton Experimental\"";
        assert!(steam_game(tool, Path::new("/"), "steam").is_none());
        let lib = "\"0\"\n{\n\t\"path\"\t\t\"/mnt/games\\\\lib\"\n}";
        assert_eq!(vdf_values(lib, "path"), ["/mnt/games\\lib"]);
    }
}

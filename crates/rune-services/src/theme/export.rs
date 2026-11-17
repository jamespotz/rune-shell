//! Per-app theme exporters. Each one is a pure function: palette + the current
//! contents of the files it edits → the new contents. Writing (with backups)
//! happens in [`apply`]. Our own files are named `rune*`; user config files only
//! get the one line or key needed to load them.

use super::{Palette, Rgb, backup::Backup};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum App {
    Kitty,
    Ghostty,
    Btop,
    Yazi,
    Neovim,
    Zed,
    VsCode,
    Kiro,
    Gtk3,
    Gtk4,
    Qt,
    Kde,
    Firefox,
    Chrome,
    Vivaldi,
}

impl App {
    pub const ALL: [App; 15] = [
        App::Kitty,
        App::Ghostty,
        App::Btop,
        App::Yazi,
        App::Neovim,
        App::Zed,
        App::VsCode,
        App::Kiro,
        App::Gtk3,
        App::Gtk4,
        App::Qt,
        App::Kde,
        App::Firefox,
        App::Chrome,
        App::Vivaldi,
    ];

    /// Config name.
    pub fn id(self) -> &'static str {
        match self {
            App::Kitty => "kitty",
            App::Ghostty => "ghostty",
            App::Btop => "btop",
            App::Yazi => "yazi",
            App::Neovim => "nvim",
            App::Zed => "zed",
            App::VsCode => "vscode",
            App::Kiro => "kiro",
            App::Gtk3 => "gtk3",
            App::Gtk4 => "gtk4",
            App::Qt => "qt",
            App::Kde => "kde",
            App::Firefox => "firefox",
            App::Chrome => "chrome",
            App::Vivaldi => "vivaldi",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            App::Kitty => "Kitty",
            App::Ghostty => "Ghostty",
            App::Btop => "btop",
            App::Yazi => "Yazi",
            App::Neovim => "Neovim (:colorscheme rune)",
            App::Zed => "Zed",
            App::VsCode => "VS Code",
            App::Kiro => "Kiro",
            App::Gtk3 => "GTK 3 apps",
            App::Gtk4 => "GTK 4 / libadwaita apps",
            App::Qt => "Qt apps (qt5ct / qt6ct)",
            App::Kde => "KDE apps",
            App::Firefox => "Firefox (applies on restart)",
            App::Chrome => "Chrome (load the theme once, see README)",
            App::Vivaldi => "Vivaldi (point Custom UI Modifications at ~/.config/vivaldi/rune)",
        }
    }
}

/// What applying one app's theme means.
#[derive(Debug, Default)]
pub struct Plan {
    pub files: Vec<(PathBuf, String)>,
    /// Commands that make running instances pick the theme up.
    pub reload: Vec<Vec<String>>,
}

/// Builds `app`'s plan. `read` returns a file's current contents ("" if missing).
pub fn plan(app: App, p: &Palette, home: &Path, read: &dyn Fn(&Path) -> String) -> Plan {
    let cfg = home.join(".config");
    let file = |rel: &str, text: String| (cfg.join(rel), text);
    let cmd = |args: &[&str]| args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    match app {
        App::Kitty => {
            let conf = cfg.join("kitty/kitty.conf");
            Plan {
                files: vec![
                    file("kitty/themes/rune.conf", kitty(p)),
                    (
                        conf.clone(),
                        ensure_line(&read(&conf), "include themes/rune.conf", |l| {
                            l == "include themes/rune.conf"
                        }),
                    ),
                ],
                // kitty reloads its config on SIGUSR1.
                reload: vec![cmd(&["pkill", "-USR1", "-x", "kitty"])],
            }
        }
        App::Ghostty => {
            let conf = cfg.join("ghostty/config");
            Plan {
                files: vec![
                    file("ghostty/themes/rune", ghostty(p)),
                    (
                        conf.clone(),
                        set_line(&read(&conf), "theme = rune", |l| key_is(l, "theme", '=')),
                    ),
                ],
                reload: vec![cmd(&["pkill", "-USR2", "-x", "ghostty"])],
            }
        }
        App::Btop => {
            let conf = cfg.join("btop/btop.conf");
            Plan {
                files: vec![
                    file("btop/themes/rune.theme", btop(p)),
                    (
                        conf.clone(),
                        set_line(&read(&conf), "color_theme = \"rune\"", |l| {
                            key_is(l, "color_theme", '=')
                        }),
                    ),
                ],
                reload: Vec::new(),
            }
        }
        App::Yazi => Plan {
            files: vec![file("yazi/theme.toml", yazi(p))],
            reload: Vec::new(),
        },
        App::Neovim => Plan {
            files: vec![file("nvim/colors/rune.lua", nvim(p))],
            reload: Vec::new(),
        },
        App::Zed => {
            let settings = cfg.join("zed/settings.json");
            Plan {
                files: vec![
                    file("zed/themes/rune.json", zed(p)),
                    (
                        settings.clone(),
                        jsonc_set(&read(&settings), "theme", "\"Rune\""),
                    ),
                ],
                reload: Vec::new(), // Zed watches both files
            }
        }
        App::VsCode | App::Kiro => {
            let dir = if app == App::VsCode { "Code" } else { "Kiro" };
            let settings = cfg.join(dir).join("User/settings.json");
            let colors = serde_json::to_string_pretty(&vscode(p))
                .unwrap_or_default()
                .replace('\n', "\n    ");
            Plan {
                files: vec![(
                    settings.clone(),
                    jsonc_set(&read(&settings), "workbench.colorCustomizations", &colors),
                )],
                reload: Vec::new(),
            }
        }
        App::Gtk3 => Plan {
            files: vec![file("gtk-3.0/gtk.css", gtk(p))],
            reload: Vec::new(),
        },
        App::Gtk4 => Plan {
            files: vec![file("gtk-4.0/gtk.css", gtk(p))],
            reload: Vec::new(),
        },
        App::Qt => {
            let mut files = Vec::new();
            for v in ["qt5ct", "qt6ct"] {
                let scheme = cfg.join(v).join("colors/rune.conf");
                let conf = cfg.join(v).join(format!("{v}.conf"));
                let mut text = set_ini(&read(&conf), "Appearance", "custom_palette", "true");
                text = set_ini(
                    &text,
                    "Appearance",
                    "color_scheme_path",
                    &scheme.display().to_string(),
                );
                files.push((scheme, qt(p)));
                files.push((conf, text));
            }
            Plan {
                files,
                reload: Vec::new(),
            }
        }
        App::Firefox => {
            // Newer Firefox keeps profiles under ~/.config/mozilla, older under ~/.mozilla.
            let roots = [cfg.join("mozilla/firefox"), home.join(".mozilla/firefox")];
            let Some(dir) = roots
                .iter()
                .find_map(|r| firefox_profile(&read(&r.join("profiles.ini"))).map(|p| r.join(p)))
            else {
                return Plan::default(); // Firefox never ran
            };
            let user_chrome = dir.join("chrome/userChrome.css");
            let user_js = dir.join("user.js");
            let pref = "user_pref(\"toolkit.legacyUserProfileCustomizations.stylesheets\", true);";
            Plan {
                files: vec![
                    (dir.join("chrome/rune.css"), firefox(p)),
                    // @import must come first in a stylesheet.
                    (
                        user_chrome.clone(),
                        prepend_once(&read(&user_chrome), "@import \"rune.css\";"),
                    ),
                    (
                        user_js.clone(),
                        ensure_line(&read(&user_js), pref, |l| {
                            l.contains("legacyUserProfileCustomizations")
                        }),
                    ),
                ],
                reload: Vec::new(),
            }
        }
        App::Chrome => Plan {
            // An unpacked theme: load it once at chrome://extensions, then restarts pick up changes.
            files: vec![(cfg.join("rune-shell/chrome-theme/manifest.json"), chrome(p))],
            reload: Vec::new(),
        },
        App::Vivaldi => Plan {
            files: vec![(cfg.join("vivaldi/rune/rune.css"), vivaldi(p))],
            reload: Vec::new(),
        },
        App::Kde => {
            let globals = cfg.join("kdeglobals");
            let mut text = read(&globals);
            for (section, key, value) in kde_keys(p) {
                text = set_ini(&text, section, key, &value);
            }
            Plan {
                files: vec![
                    (
                        home.join(".local/share/color-schemes/Rune.colors"),
                        kde_scheme(p),
                    ),
                    (globals, text),
                ],
                reload: Vec::new(),
            }
        }
    }
}

/// Applies `apps`, backing up every file first. Returns a summary line per app.
pub fn apply(apps: &[App], p: &Palette, home: &Path, backup: &mut Backup) -> Vec<String> {
    let read = |path: &Path| std::fs::read_to_string(path).unwrap_or_default();
    let mut report = Vec::new();
    for &app in apps {
        let plan = plan(app, p, home, &read);
        let result: std::io::Result<()> = plan
            .files
            .iter()
            .try_for_each(|(path, text)| backup.write(path, text).map(drop));
        match result {
            Ok(()) => {
                for c in &plan.reload {
                    let _ = Command::new(&c[0]).args(&c[1..]).status(); // fine if the app isn't running
                }
                let note = match app {
                    App::Vivaldi => vivaldi_folder_note(home, &read),
                    _ => None,
                };
                report.push(note.unwrap_or_else(|| format!("{}: done", app.label())));
            }
            Err(e) => report.push(format!("{}: {e}", app.label())),
        }
    }
    if apps
        .iter()
        .any(|a| matches!(a, App::Gtk3 | App::Gtk4 | App::Kde))
    {
        let scheme = if p.dark { "'prefer-dark'" } else { "'default'" };
        if let Err(e) = backup.set_gsetting("org.gnome.desktop.interface", "color-scheme", scheme) {
            report.push(format!("color scheme preference: {e}"));
        }
    }
    if apps.contains(&App::Gtk4) || apps.contains(&App::Gtk3) {
        refresh_gtk();
    }
    report
}

/// Makes running apps re-read their (restored) theme files.
pub fn reload_running() {
    // kitty reloads on SIGUSR1, Ghostty (1.2+) on SIGUSR2; no-ops if not running.
    let _ = Command::new("pkill")
        .args(["-USR1", "-x", "kitty"])
        .status();
    let _ = Command::new("pkill")
        .args(["-USR2", "-x", "ghostty"])
        .status();
    refresh_gtk();
}

/// Running GTK apps only re-read `gtk.css` when the theme changes, so flip the
/// theme name away and back (the usual workaround; there's no reload signal).
fn refresh_gtk() {
    let get = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
        .output();
    let Ok(out) = get else { return };
    let current = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if current.is_empty() {
        return;
    }
    let other = if current == "'Adwaita'" {
        "'HighContrast'"
    } else {
        "'Adwaita'"
    };
    let set = |v: &str| {
        Command::new("gsettings")
            .args(["set", "org.gnome.desktop.interface", "gtk-theme", v])
            .status()
    };
    let _ = set(other);
    std::thread::sleep(std::time::Duration::from_millis(150));
    let _ = set(&current);
}

/// Appends `line` unless a line matching `present` exists.
fn ensure_line(text: &str, line: &str, present: impl Fn(&str) -> bool) -> String {
    if text.lines().any(|l| present(l.trim())) {
        return text.to_owned();
    }
    format!("{}\n{line}\n", text.trim_end())
        .trim_start()
        .to_owned()
}

/// Replaces the first line matching `matches` with `line`, or appends it.
fn set_line(text: &str, line: &str, matches: impl Fn(&str) -> bool) -> String {
    if !text.lines().any(|l| matches(l.trim())) {
        return ensure_line(text, line, |_| false);
    }
    let mut done = false;
    let mut out: Vec<&str> = Vec::new();
    for l in text.lines() {
        if !done && matches(l.trim()) {
            out.push(line);
            done = true;
        } else {
            out.push(l);
        }
    }
    out.join("\n") + "\n"
}

fn key_is(line: &str, key: &str, sep: char) -> bool {
    line.split_once(sep).is_some_and(|(k, _)| k.trim() == key)
}

/// Sets `key=value` in `[section]` of an INI file, keeping everything else.
pub fn set_ini(text: &str, section: &str, key: &str, value: &str) -> String {
    let header = format!("[{section}]");
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines.iter().position(|l| l.trim() == header) else {
        let sep = if text.trim().is_empty() { "" } else { "\n\n" };
        return format!("{}{sep}{header}\n{key}={value}\n", text.trim_end());
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |i| start + 1 + i);
    let entry = format!("{key}={value}");
    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    match (start + 1..end).find(|&i| key_is(lines[i].trim(), key, '=')) {
        Some(i) => out[i] = entry,
        None => {
            // After the section's last non-blank line.
            let at = (start + 1..end)
                .rev()
                .find(|&i| !lines[i].trim().is_empty())
                .map_or(start + 1, |i| i + 1);
            out.insert(at, entry);
        }
    }
    out.join("\n") + "\n"
}

/// Sets a top-level `key` of a JSONC object to `value` (raw JSON), keeping
/// comments and the rest of the file untouched.
pub fn jsonc_set(text: &str, key: &str, value: &str) -> String {
    if text.trim().is_empty() {
        return format!("{{\n    \"{key}\": {value}\n}}\n");
    }
    let b = text.as_bytes();
    let mut i = 0;
    let mut depth = 0;
    let mut open = None;
    // Scans past strings/comments, tracking depth; at depth 1 looks for `"key":`.
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                i = text[i..].find('\n').map_or(b.len(), |n| i + n)
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = text[i + 2..].find("*/").map_or(b.len(), |n| i + n + 4)
            }
            b'"' => {
                let end = string_end(b, i);
                if depth == 1 && &text[i + 1..end - 1] == key {
                    let mut j = skip_ws(text, end);
                    if b.get(j) == Some(&b':') {
                        j = skip_ws(text, j + 1);
                        let v_end = value_end(text, j);
                        return format!("{}{value}{}", &text[..j], &text[v_end..]);
                    }
                }
                i = end;
                continue;
            }
            b'{' | b'[' => {
                depth += 1;
                if depth == 1 && open.is_none() {
                    open = Some(i);
                }
            }
            b'}' | b']' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    // Not present: add it first (trailing commas are fine in JSONC).
    match open {
        Some(o) => format!("{}\n    \"{key}\": {value},{}", &text[..=o], &text[o + 1..]),
        None => text.to_owned(),
    }
}

/// Index just past the string starting at `i` (which is `"`).
fn string_end(b: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    b.len()
}

fn skip_ws(text: &str, mut i: usize) -> usize {
    let b = text.as_bytes();
    loop {
        while b.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if text[i..].starts_with("//") {
            i = text[i..].find('\n').map_or(b.len(), |n| i + n);
        } else if text[i..].starts_with("/*") {
            i = text[i + 2..].find("*/").map_or(b.len(), |n| i + n + 4);
        } else {
            return i;
        }
    }
}

/// End of the JSON value starting at `i`.
fn value_end(text: &str, mut i: usize) -> usize {
    let b = text.as_bytes();
    let mut depth = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                i = string_end(b, i);
                if depth == 0 {
                    return i;
                }
                continue;
            }
            b'/' if matches!(b.get(i + 1), Some(b'/' | b'*')) && depth > 0 => {
                i = skip_ws(text, i);
                continue;
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            // Scalars (numbers, true, null) end at whitespace or a delimiter.
            b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n' if depth == 0 => return i,
            _ => {}
        }
        i += 1;
    }
    b.len()
}

const HEADER: &str =
    "Generated by Rune Shell. Changes here are overwritten; edit the theme in Rune's settings.";

fn kitty(p: &Palette) -> String {
    let mut s = format!(
        "# {HEADER}\nbackground {}\nforeground {}\nselection_background {}\nselection_foreground {}\ncursor {}\ncursor_text_color {}\nurl_color {}\nactive_border_color {}\ninactive_border_color {}\nactive_tab_background {}\nactive_tab_foreground {}\ninactive_tab_background {}\ninactive_tab_foreground {}\ntab_bar_background {}\n",
        p.bg.hex(),
        p.fg.hex(),
        p.surface_high.hex(),
        p.fg.hex(),
        p.primary.hex(),
        p.bg.hex(),
        p.primary.hex(),
        p.primary.hex(),
        p.outline.hex(),
        p.primary.hex(),
        p.on_primary.hex(),
        p.surface.hex(),
        p.muted.hex(),
        p.bg_dim.hex()
    );
    for (i, c) in p.term.iter().enumerate() {
        s += &format!("color{i} {}\n", c.hex());
    }
    s
}

fn ghostty(p: &Palette) -> String {
    let mut s = format!("# {HEADER}\n");
    for (i, c) in p.term.iter().enumerate() {
        s += &format!("palette = {i}={}\n", c.hex());
    }
    s + &format!(
        "background = {}\nforeground = {}\ncursor-color = {}\ncursor-text = {}\nselection-background = {}\nselection-foreground = {}\n",
        p.bg.hex(),
        p.fg.hex(),
        p.primary.hex(),
        p.bg.hex(),
        p.surface_high.hex(),
        p.fg.hex()
    )
}

fn btop(p: &Palette) -> String {
    let (g, y, r) = (p.green().hex(), p.yellow().hex(), p.red().hex());
    let mut s = format!(
        "# {HEADER}\ntheme[main_bg]=\"{}\"\ntheme[main_fg]=\"{}\"\ntheme[title]=\"{}\"\ntheme[hi_fg]=\"{}\"\ntheme[selected_bg]=\"{}\"\ntheme[selected_fg]=\"{}\"\ntheme[inactive_fg]=\"{}\"\ntheme[proc_misc]=\"{}\"\ntheme[div_line]=\"{}\"\n",
        p.bg.hex(),
        p.fg.hex(),
        p.primary.hex(),
        p.primary.hex(),
        p.surface_high.hex(),
        p.fg.hex(),
        p.outline.hex(),
        p.muted.hex(),
        p.outline.hex()
    );
    for b in ["cpu_box", "mem_box", "net_box", "proc_box"] {
        s += &format!("theme[{b}]=\"{}\"\n", p.outline.hex());
    }
    for k in [
        "temp",
        "cpu",
        "free",
        "cached",
        "available",
        "used",
        "download",
        "upload",
    ] {
        s += &format!("theme[{k}_start]=\"{g}\"\ntheme[{k}_mid]=\"{y}\"\ntheme[{k}_end]=\"{r}\"\n");
    }
    s
}

fn yazi(p: &Palette) -> String {
    let fg = |c: Rgb| format!("{{ fg = \"{}\" }}", c.hex());
    let both = |f: Rgb, b: Rgb, extra: &str| {
        format!("{{ fg = \"{}\", bg = \"{}\"{extra} }}", f.hex(), b.hex())
    };
    let (a, on, hi) = (p.primary, p.on_primary, p.surface_high);
    format!(
        "# {HEADER}\n\n[mgr]\ncwd = {}\nborder_style = {}\nmarker_copied = {}\nmarker_cut = {}\nmarker_marked = {}\nmarker_selected = {}\ncount_copied = {}\ncount_cut = {}\ncount_selected = {}\n\n[tabs]\nactive = {}\ninactive = {}\n\n[mode]\nnormal_main = {}\nnormal_alt = {}\nselect_main = {}\nselect_alt = {}\nunset_main = {}\nunset_alt = {}\n\n[status]\nperm_sep = {}\nperm_type = {}\nperm_read = {}\nperm_write = {}\nperm_exec = {}\n\n[which]\nborder = {}\ncand = {}\nrest = {}\ndesc = {}\n\n[input]\nborder = {}\n\n[confirm]\nborder = {}\ntitle = {}\n",
        fg(a),
        fg(p.outline),
        both(p.green(), p.green(), ""),
        both(p.red(), p.red(), ""),
        both(a, a, ""),
        both(p.yellow(), p.yellow(), ""),
        both(on, p.green(), ""),
        both(on, p.red(), ""),
        both(on, p.yellow(), ""),
        both(on, a, ", bold = true"),
        both(p.muted, hi, ""),
        both(on, a, ", bold = true"),
        both(a, hi, ""),
        both(on, p.yellow(), ", bold = true"),
        both(p.yellow(), hi, ""),
        both(on, p.red(), ", bold = true"),
        both(p.red(), hi, ""),
        fg(p.outline),
        fg(a),
        fg(p.green()),
        fg(p.yellow()),
        fg(p.red()),
        fg(a),
        fg(a),
        fg(p.outline),
        fg(p.yellow()),
        fg(a),
        fg(a),
        fg(a)
    )
}

fn nvim(p: &Palette) -> String {
    let c = |r: Rgb| format!("\"{}\"", r.hex());
    let mut s = format!(
        "-- {HEADER}\n-- Use it with :colorscheme rune (or vim.cmd.colorscheme(\"rune\") in init.lua).\nvim.cmd(\"highlight clear\")\nvim.o.background = \"{}\"\nvim.g.colors_name = \"rune\"\nlocal c = {{ bg = {}, bg_dim = {}, surface = {}, surface_high = {}, fg = {}, muted = {}, outline = {}, primary = {}, on_primary = {}, secondary = {}, tertiary = {}, red = {}, green = {}, yellow = {}, blue = {}, magenta = {}, cyan = {} }}\nlocal hl = function(group, opts) vim.api.nvim_set_hl(0, group, opts) end\n",
        if p.dark { "dark" } else { "light" },
        c(p.bg),
        c(p.bg_dim),
        c(p.surface),
        c(p.surface_high),
        c(p.fg),
        c(p.muted),
        c(p.outline),
        c(p.primary),
        c(p.on_primary),
        c(p.secondary),
        c(p.tertiary),
        c(p.term[1]),
        c(p.term[2]),
        c(p.term[3]),
        c(p.term[4]),
        c(p.term[5]),
        c(p.term[6])
    );
    for line in [
        "Normal = { fg = c.fg, bg = c.bg }",
        "NormalFloat = { fg = c.fg, bg = c.surface }",
        "FloatBorder = { fg = c.outline, bg = c.surface }",
        "CursorLine = { bg = c.surface }",
        "CursorLineNr = { fg = c.primary, bold = true }",
        "LineNr = { fg = c.outline }",
        "Visual = { bg = c.surface_high }",
        "Search = { fg = c.on_primary, bg = c.primary }",
        "IncSearch = { fg = c.on_primary, bg = c.tertiary }",
        "Pmenu = { fg = c.fg, bg = c.surface }",
        "PmenuSel = { fg = c.on_primary, bg = c.primary }",
        "StatusLine = { fg = c.fg, bg = c.surface }",
        "StatusLineNC = { fg = c.muted, bg = c.bg_dim }",
        "WinSeparator = { fg = c.surface_high }",
        "SignColumn = { bg = c.bg }",
        "Comment = { fg = c.outline, italic = true }",
        "String = { fg = c.green }",
        "Character = { fg = c.green }",
        "Number = { fg = c.tertiary }",
        "Boolean = { fg = c.tertiary }",
        "Constant = { fg = c.tertiary }",
        "Identifier = { fg = c.fg }",
        "Function = { fg = c.primary }",
        "Keyword = { fg = c.secondary, italic = true }",
        "Statement = { fg = c.secondary }",
        "Type = { fg = c.yellow }",
        "Operator = { fg = c.muted }",
        "PreProc = { fg = c.magenta }",
        "Special = { fg = c.cyan }",
        "Title = { fg = c.primary, bold = true }",
        "Error = { fg = c.red }",
        "DiagnosticError = { fg = c.red }",
        "DiagnosticWarn = { fg = c.yellow }",
        "DiagnosticInfo = { fg = c.blue }",
        "DiagnosticHint = { fg = c.cyan }",
        "DiffAdd = { fg = c.green }",
        "DiffChange = { fg = c.yellow }",
        "DiffDelete = { fg = c.red }",
        "[\"@variable\"] = { fg = c.fg }",
        "[\"@property\"] = { fg = c.muted }",
    ] {
        let (group, opts) = line.split_once(" = ").unwrap_or_default();
        let group = group.trim_start_matches("[\"").trim_end_matches("\"]");
        s += &format!("hl(\"{group}\", {opts})\n");
    }
    for (i, t) in p.term.iter().enumerate() {
        s += &format!("vim.g.terminal_color_{i} = \"{}\"\n", t.hex());
    }
    s
}

fn zed(p: &Palette) -> String {
    let h = |c: Rgb| c.hex();
    let a = |c: Rgb, alpha: &str| format!("{}{alpha}", c.hex());
    let names = [
        "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    ];
    let mut style = serde_json::Map::new();
    let mut put = |k: &str, v: String| {
        style.insert(k.to_owned(), json!(v));
    };
    for (k, v) in [
        ("background", h(p.bg_dim)),
        ("border", h(p.surface_high)),
        ("border.variant", h(p.surface)),
        ("surface.background", h(p.bg_dim)),
        ("elevated_surface.background", h(p.surface)),
        ("element.background", h(p.surface)),
        ("element.hover", h(p.surface_high)),
        ("element.selected", a(p.primary, "40")),
        ("ghost_element.hover", a(p.fg, "14")),
        ("ghost_element.selected", a(p.primary, "33")),
        ("text", h(p.fg)),
        ("text.muted", h(p.muted)),
        ("text.placeholder", h(p.outline)),
        ("text.accent", h(p.primary)),
        ("icon", h(p.fg)),
        ("icon.muted", h(p.muted)),
        ("icon.accent", h(p.primary)),
        ("title_bar.background", h(p.bg_dim)),
        ("status_bar.background", h(p.bg_dim)),
        ("toolbar.background", h(p.bg)),
        ("tab_bar.background", h(p.bg_dim)),
        ("tab.active_background", h(p.bg)),
        ("tab.inactive_background", h(p.bg_dim)),
        ("panel.background", h(p.bg_dim)),
        ("editor.background", h(p.bg)),
        ("editor.foreground", h(p.fg)),
        ("editor.gutter.background", h(p.bg)),
        ("editor.line_number", h(p.outline)),
        ("editor.active_line_number", h(p.primary)),
        ("editor.active_line.background", a(p.surface, "aa")),
        ("terminal.background", h(p.bg)),
        ("terminal.foreground", h(p.fg)),
        ("error", h(p.error)),
        ("warning", h(p.yellow())),
        ("success", h(p.green())),
        ("info", h(p.primary)),
    ] {
        put(k, v);
    }
    for (i, n) in names.iter().enumerate() {
        put(&format!("terminal.ansi.{n}"), h(p.term[i]));
        put(&format!("terminal.ansi.bright_{n}"), h(p.term[i + 8]));
    }
    let syn = |c: Rgb| json!({ "color": c.hex() });
    style.insert("players".into(), json!([{ "cursor": h(p.primary), "background": h(p.primary), "selection": a(p.primary, "40") }]));
    style.insert(
        "syntax".into(),
        json!({
            "comment": { "color": h(p.outline), "font_style": "italic" },
            "string": syn(p.green()), "number": syn(p.tertiary), "boolean": syn(p.tertiary), "constant": syn(p.tertiary),
            "keyword": syn(p.secondary), "function": syn(p.primary), "type": syn(p.yellow()), "property": syn(p.muted),
            "variable": syn(p.fg), "operator": syn(p.muted), "punctuation": syn(p.muted), "tag": syn(p.term[5]),
            "attribute": syn(p.term[6]), "title": { "color": h(p.primary), "font_weight": 700 },
        }),
    );
    let theme = json!({
        "$schema": "https://zed.dev/schema/themes/v0.2.0.json",
        "name": "Rune",
        "author": "Rune Shell",
        "themes": [{ "name": "Rune", "appearance": if p.dark { "dark" } else { "light" }, "style": style }],
    });
    serde_json::to_string_pretty(&theme).unwrap_or_default() + "\n"
}

fn vscode(p: &Palette) -> serde_json::Value {
    let names = [
        "Black", "Red", "Green", "Yellow", "Blue", "Magenta", "Cyan", "White",
    ];
    let mut m = serde_json::Map::new();
    for (k, v) in [
        ("editor.background", p.bg),
        ("editor.foreground", p.fg),
        ("editorCursor.foreground", p.primary),
        ("editor.selectionBackground", p.surface_high),
        ("editor.lineHighlightBackground", p.surface),
        ("editorLineNumber.foreground", p.outline),
        ("editorLineNumber.activeForeground", p.primary),
        ("sideBar.background", p.bg_dim),
        ("sideBar.foreground", p.fg),
        ("activityBar.background", p.bg_dim),
        ("activityBar.foreground", p.fg),
        ("statusBar.background", p.bg_dim),
        ("statusBar.foreground", p.fg),
        ("titleBar.activeBackground", p.bg_dim),
        ("titleBar.activeForeground", p.fg),
        ("tab.activeBackground", p.bg),
        ("tab.inactiveBackground", p.bg_dim),
        ("panel.background", p.bg_dim),
        ("focusBorder", p.primary),
        ("button.background", p.primary),
        ("button.foreground", p.on_primary),
        ("badge.background", p.primary),
        ("badge.foreground", p.on_primary),
        ("terminal.background", p.bg),
        ("terminal.foreground", p.fg),
    ] {
        m.insert(k.into(), json!(v.hex()));
    }
    for (i, n) in names.iter().enumerate() {
        m.insert(format!("terminal.ansi{n}"), json!(p.term[i].hex()));
        m.insert(
            format!("terminal.ansiBright{n}"),
            json!(p.term[i + 8].hex()),
        );
    }
    serde_json::Value::Object(m)
}

fn gtk(p: &Palette) -> String {
    let mut s = format!("/* {HEADER} */\n");
    for (name, c) in [
        ("window_bg_color", p.bg),
        ("window_fg_color", p.fg),
        ("view_bg_color", p.surface),
        ("view_fg_color", p.fg),
        ("headerbar_bg_color", p.bg_dim),
        ("headerbar_fg_color", p.fg),
        ("headerbar_backdrop_color", p.bg),
        ("sidebar_bg_color", p.bg_dim),
        ("sidebar_fg_color", p.fg),
        ("card_bg_color", p.surface),
        ("card_fg_color", p.fg),
        ("dialog_bg_color", p.surface),
        ("dialog_fg_color", p.fg),
        ("popover_bg_color", p.surface_high),
        ("popover_fg_color", p.fg),
        ("accent_color", p.primary),
        ("accent_bg_color", p.primary),
        ("accent_fg_color", p.on_primary),
        ("destructive_color", p.error),
        ("destructive_bg_color", p.error),
        ("destructive_fg_color", p.on_primary),
        ("success_color", p.green()),
        ("warning_color", p.yellow()),
        ("error_color", p.error),
        ("theme_bg_color", p.bg),
        ("theme_fg_color", p.fg),
        ("theme_base_color", p.surface),
        ("theme_text_color", p.fg),
        ("theme_selected_bg_color", p.primary),
        ("theme_selected_fg_color", p.on_primary),
        ("insensitive_fg_color", p.outline),
        ("borders", p.outline),
    ] {
        s += &format!("@define-color {name} {};\n", c.hex());
    }
    s
}

fn qt(p: &Palette) -> String {
    let c = |r: Rgb| format!("#ff{}", &r.hex()[1..]);
    // QPalette::ColorRole order, as qt5ct/qt6ct expect.
    let roles = [
        p.fg,
        p.surface,
        p.surface_high,
        p.surface_high,
        p.bg_dim,
        p.surface,
        p.fg,
        p.fg,
        p.fg,
        p.bg,
        p.bg,
        p.bg_dim,
        p.primary,
        p.on_primary,
        p.primary,
        p.secondary,
        p.surface,
        p.bg,
        p.surface_high,
        p.fg,
        p.outline,
    ];
    let active = roles.map(c).join(", ");
    let disabled = roles
        .map(|r| if r == p.fg { c(p.outline) } else { c(r) })
        .join(", ");
    format!(
        "[ColorScheme]\nactive_colors={active}\ninactive_colors={active}\ndisabled_colors={disabled}\n"
    )
}

fn kde_keys(p: &Palette) -> Vec<(&'static str, &'static str, String)> {
    let mut keys = vec![("General", "ColorScheme", "Rune".to_owned())];
    for (section, bg) in [
        ("Colors:Window", p.bg),
        ("Colors:View", p.surface),
        ("Colors:Button", p.surface_high),
        ("Colors:Tooltip", p.surface_high),
        ("Colors:Header", p.bg_dim),
    ] {
        keys.push((section, "BackgroundNormal", bg.csv()));
        keys.push((section, "BackgroundAlternate", p.surface.csv()));
        keys.push((section, "ForegroundNormal", p.fg.csv()));
        keys.push((section, "ForegroundInactive", p.muted.csv()));
        keys.push((section, "DecorationFocus", p.primary.csv()));
        keys.push((section, "DecorationHover", p.primary.csv()));
    }
    keys.push(("Colors:Selection", "BackgroundNormal", p.primary.csv()));
    keys.push(("Colors:Selection", "ForegroundNormal", p.on_primary.csv()));
    keys.push(("WM", "activeBackground", p.bg_dim.csv()));
    keys.push(("WM", "activeForeground", p.fg.csv()));
    keys.push(("WM", "inactiveBackground", p.bg_dim.csv()));
    keys.push(("WM", "inactiveForeground", p.muted.csv()));
    keys
}

/// The profile Firefox opens by default: the `[Install*]` choice, else `Default=1`.
fn firefox_profile(ini: &str) -> Option<String> {
    let sections: Vec<&str> = ini.split("\n[").collect();
    let value = |section: &str, key: &str| {
        section
            .lines()
            .find_map(|l| l.trim().strip_prefix(key).map(|v| v.trim().to_owned()))
    };
    sections
        .iter()
        .filter(|s| s.trim_start_matches('[').starts_with("Install"))
        .find_map(|s| value(s, "Default="))
        .or_else(|| {
            sections
                .iter()
                .filter(|s| {
                    s.trim_start_matches('[').starts_with("Profile")
                        && s.lines().any(|l| l.trim() == "Default=1")
                })
                .find_map(|s| value(s, "Path="))
        })
}

/// Puts `line` at the top unless it's already in the file.
fn prepend_once(text: &str, line: &str) -> String {
    if text.lines().any(|l| l.trim() == line) {
        return text.to_owned();
    }
    format!("{line}\n{text}")
}

fn firefox(p: &Palette) -> String {
    let h = |c: Rgb| c.hex();
    format!(
        "/* {HEADER} */\n:root {{\n  --lwt-accent-color: {bg} !important;\n  --lwt-text-color: {fg} !important;\n  --toolbar-bgcolor: {surface} !important;\n  --toolbar-color: {fg} !important;\n  --toolbarbutton-icon-fill: {fg} !important;\n  --tab-selected-bgcolor: {sel} !important;\n  --tab-selected-textcolor: {fg} !important;\n  --urlbar-box-bgcolor: {bg} !important;\n  --toolbar-field-background-color: {bg} !important;\n  --toolbar-field-color: {fg} !important;\n  --toolbar-field-focus-background-color: {bg} !important;\n  --toolbar-field-focus-border-color: {accent} !important;\n  --arrowpanel-background: {surface} !important;\n  --arrowpanel-color: {fg} !important;\n  --focus-outline-color: {accent} !important;\n}}\n#navigator-toolbox, #TabsToolbar {{ background-color: {bg} !important; }}\n#nav-bar {{ background-color: {surface} !important; }}\n.tab-background[selected] {{ background: {sel} !important; }}\n",
        bg = h(p.bg_dim),
        fg = h(p.fg),
        surface = h(p.surface),
        sel = h(p.surface_high),
        accent = h(p.primary),
    )
}

fn chrome(p: &Palette) -> String {
    let c = |r: Rgb| serde_json::json!(r.0);
    let manifest = json!({
        "manifest_version": 3,
        "name": "Rune",
        "version": "1.0",
        "description": "Generated by Rune Shell. Follows the theme picked in Rune's settings.",
        "theme": {
            "colors": {
                "frame": c(p.bg_dim),
                "frame_inactive": c(p.bg_dim),
                "toolbar": c(p.surface),
                "toolbar_text": c(p.fg),
                "toolbar_button_icon": c(p.fg),
                "tab_text": c(p.fg),
                "tab_background_text": c(p.muted),
                "bookmark_text": c(p.fg),
                "omnibox_background": c(p.bg),
                "omnibox_text": c(p.fg),
                "ntp_background": c(p.bg),
                "ntp_text": c(p.fg),
                "ntp_link": c(p.primary),
                "button_background": c(p.surface_high),
            },
            "properties": { "ntp_background_alignment": "bottom" },
        },
    });
    serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n"
}

fn vivaldi(p: &Palette) -> String {
    // Vivaldi sets these inline on #browser from its own theme, so each needs !important.
    let mut s = format!("/* {HEADER} */\n#browser {{\n");
    for (name, c) in [
        ("colorBg", p.surface),
        ("colorBgDark", p.bg_dim),
        ("colorBgDarker", p.bg_dim),
        ("colorBgLight", p.surface_high),
        ("colorBgLighter", p.surface_high),
        ("colorBgIntense", p.surface_high),
        ("colorTabBar", p.bg_dim),
        ("colorFg", p.fg),
        ("colorFgIntense", p.fg),
        ("colorFgFaded", p.muted),
        ("colorBorder", p.outline),
        ("colorBorderSubtle", p.surface_high),
        ("colorHighlightBg", p.primary),
        ("colorHighlightFg", p.on_primary),
        ("colorAccentBg", p.bg),
        ("colorAccentFg", p.fg),
        ("colorErrorBg", p.error),
    ] {
        s += &format!("  --{name}: {} !important;\n", c.hex());
    }
    s + "}\n"
}

/// Vivaldi only loads the folder set in Settings → Appearance → Custom UI Modifications.
fn vivaldi_folder_note(home: &Path, read: &dyn Fn(&Path) -> String) -> Option<String> {
    let cfg = home.join(".config/vivaldi");
    let prefs: serde_json::Value =
        serde_json::from_str(&read(&cfg.join("Default/Preferences"))).ok()?;
    let folder = prefs
        .pointer("/vivaldi/appearance/css_ui_mods_directory")?
        .as_str()?;
    let rune = cfg.join("rune");
    (Path::new(folder) != rune).then(|| {
        format!(
            "Vivaldi: written, but Custom UI Modifications loads {folder}; set it to {} and restart Vivaldi",
            rune.display()
        )
    })
}

fn kde_scheme(p: &Palette) -> String {
    let mut text = format!("# {HEADER}\n[General]\nName=Rune\n");
    for (section, key, value) in kde_keys(p).into_iter().skip(1) {
        text = set_ini(&text, section, key, &value);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vivaldi_note_names_the_wrong_folder() {
        let home = Path::new("/home/u");
        let prefs = |dir: &str| {
            let text =
                format!(r#"{{"vivaldi":{{"appearance":{{"css_ui_mods_directory":"{dir}"}}}}}}"#);
            move |_: &Path| text.clone()
        };
        let note = vivaldi_folder_note(home, &prefs("/home/u/.config/vivaldi/helios"));
        assert!(note.is_some_and(|n| n.contains("helios") && n.contains("/home/u/.config/vivaldi/rune")));
        assert!(vivaldi_folder_note(home, &prefs("/home/u/.config/vivaldi/rune")).is_none());
        assert!(vivaldi_folder_note(home, &|_: &Path| String::new()).is_none());
    }
    use crate::theme::preset_named;

    #[test]
    fn browsers() {
        let ini = "[Profile1]\nName=default\nPath=vzird4lu.default\nDefault=1\n\n[Profile0]\nPath=tlm9k16q.default-release\n\n[Install11457493C5A56847]\nDefault=tlm9k16q.default-release\n";
        assert_eq!(
            firefox_profile(ini).as_deref(),
            Some("tlm9k16q.default-release"),
            "the install's choice wins"
        );
        assert_eq!(
            firefox_profile("[Profile0]\nPath=a.default\nDefault=1\n").as_deref(),
            Some("a.default")
        );
        assert_eq!(firefox_profile(""), None);
        assert_eq!(
            prepend_once("#nav-bar {}\n", "@import \"rune.css\";"),
            "@import \"rune.css\";\n#nav-bar {}\n"
        );
        assert_eq!(
            prepend_once("@import \"rune.css\";\nx\n", "@import \"rune.css\";"),
            "@import \"rune.css\";\nx\n"
        );
        let p = crate::theme::preset_named("Nord").unwrap();
        let m: serde_json::Value = serde_json::from_str(&chrome(&p)).unwrap();
        assert_eq!(
            m["theme"]["colors"]["ntp_link"],
            serde_json::json!([136, 192, 208])
        );
        assert!(vivaldi(&p).contains("--colorHighlightBg: #88c0d0 !important;"));
        // No Firefox profile on disk: nothing is written.
        assert!(
            plan(App::Firefox, &p, Path::new("/home/u"), &|_: &Path| {
                String::new()
            })
            .files
            .is_empty()
        );
    }

    #[test]
    fn edits_ini_sections() {
        let text = "[General]\nfont=Inter\n\n[Colors:View]\nBackgroundNormal=1,2,3\n";
        let out = set_ini(text, "Colors:View", "BackgroundNormal", "9,9,9");
        assert!(out.contains("BackgroundNormal=9,9,9") && !out.contains("1,2,3"));
        let out = set_ini(&out, "General", "ColorScheme", "Rune");
        assert!(out.contains("[General]\nfont=Inter\nColorScheme=Rune\n"));
        let out = set_ini(&out, "WM", "activeForeground", "1,1,1");
        assert!(out.ends_with("[WM]\nactiveForeground=1,1,1\n"));
        assert_eq!(set_ini("", "A", "k", "v"), "[A]\nk=v\n");
    }

    #[test]
    fn edits_jsonc_without_losing_comments() {
        let text = "{\n  // my font\n  \"buffer_font_size\": 15,\n  \"theme\": { \"mode\": \"system\", \"dark\": \"One Dark\" },\n  \"vim_mode\": true\n}\n";
        let out = jsonc_set(text, "theme", "\"Rune\"");
        assert_eq!(
            out,
            "{\n  // my font\n  \"buffer_font_size\": 15,\n  \"theme\": \"Rune\",\n  \"vim_mode\": true\n}\n"
        );
        let out = jsonc_set("{\n  \"a\": 1 // c\n}", "a", "2");
        assert_eq!(out, "{\n  \"a\": 2 // c\n}");
        let out = jsonc_set("{\n  \"nested\": { \"theme\": 1 }\n}", "theme", "\"Rune\"");
        assert!(
            out.starts_with("{\n    \"theme\": \"Rune\","),
            "nested keys are not top-level: {out}"
        );
        assert_eq!(jsonc_set("", "k", "1"), "{\n    \"k\": 1\n}\n");
        let commented = "{\n  /* \"theme\": \"x\" */\n  \"theme\": \"y\"\n}";
        assert_eq!(
            jsonc_set(commented, "theme", "\"Rune\""),
            "{\n  /* \"theme\": \"x\" */\n  \"theme\": \"Rune\"\n}"
        );
    }

    #[test]
    fn config_lines_are_added_once() {
        let conf = "font_size 12\ninclude themes/rune.conf\n";
        assert_eq!(
            ensure_line(conf, "include themes/rune.conf", |l| l
                == "include themes/rune.conf"),
            conf
        );
        assert_eq!(
            set_line("font = x\ntheme = helios\n", "theme = rune", |l| key_is(
                l, "theme", '='
            )),
            "font = x\ntheme = rune\n"
        );
        assert_eq!(
            set_line("", "theme = rune", |l| key_is(l, "theme", '=')),
            "theme = rune\n"
        );
    }

    #[test]
    fn plans_touch_only_expected_files() {
        let p = preset_named("Catppuccin Mocha").unwrap();
        let home = Path::new("/home/u");
        let read = |_: &Path| String::new();
        for app in App::ALL {
            let plan = plan(app, &p, home, &read);
            // Firefox writes nothing without a profile (see `browsers`).
            assert!(!plan.files.is_empty() || app == App::Firefox, "{app:?}");
            for (path, text) in &plan.files {
                assert!(
                    path.starts_with(home),
                    "{app:?} writes outside home: {}",
                    path.display()
                );
                assert!(!text.is_empty());
            }
        }
        let zed = plan(App::Zed, &p, home, &read);
        let theme: serde_json::Value = serde_json::from_str(&zed.files[0].1).unwrap();
        assert_eq!(theme["themes"][0]["style"]["text.accent"], "#cba6f7");
        assert!(kitty(&p).contains("color15 #a6adc8\n"));
        assert_eq!(qt(&p).lines().nth(1).unwrap().split(", ").count(), 21);
    }
}

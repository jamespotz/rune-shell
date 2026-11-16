//! Freedesktop icon theme lookup, simplified: index every app icon of the
//! user's theme chain once, then resolve names from memory.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// Icon name → best file, built once at startup.
#[derive(Debug, Default)]
pub struct IconIndex {
    icons: HashMap<String, PathBuf>,
}

impl IconIndex {
    /// Indexes the current GTK icon theme, what it inherits, and hicolor.
    /// Reads the disk; call it from a blocking thread.
    pub fn build() -> Self {
        let theme = gtk_icon_theme().unwrap_or_else(|| "hicolor".into());
        let bases: Vec<PathBuf> = data_dirs().iter().map(|d| d.join("icons")).collect();
        let mut index = Self::default();
        for theme in theme_chain(&theme, &bases) {
            let mut found: HashMap<String, (u32, PathBuf)> = HashMap::new();
            for base in &bases {
                walk(&base.join(&theme), 0, &mut found);
            }
            for (name, (_, path)) in found {
                index.icons.entry(name).or_insert(path); // earlier themes win
            }
        }
        for dir in data_dirs().iter().map(|d| d.join("pixmaps")) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for path in entries.flatten().map(|e| e.path()) {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    index.icons.entry(stem.to_owned()).or_insert(path);
                }
            }
        }
        index
    }

    /// Resolves a desktop `Icon=` value: an absolute path or a theme name.
    pub fn lookup(&self, icon: &str) -> Option<PathBuf> {
        if icon.is_empty() {
            return None;
        }
        let path = Path::new(icon);
        if path.is_absolute() {
            return path.exists().then(|| path.to_path_buf());
        }
        self.icons.get(icon).cloned()
    }
}

/// Collects app icons under a theme dir, keeping the best file per name:
/// SVG first, then the largest PNG.
fn walk(dir: &Path, depth: u8, found: &mut HashMap<String, (u32, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.flatten().map(|e| e.path()) {
        if path.is_dir() {
            // Themes nest `<size>/<category>` or `<category>/<size>`; only app
            // icons matter, and skipping other categories makes this ~10x faster.
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            let wanted = name == "apps"
                || name == "scalable"
                || name.starts_with(|c: char| c.is_ascii_digit());
            if depth < 3 && wanted {
                walk(&path, depth + 1, found);
            }
            continue;
        }
        if !path.components().any(|c| c.as_os_str() == "apps") {
            continue;
        }
        let (Some(stem), Some(ext)) = (path.file_stem().and_then(|s| s.to_str()), path.extension())
        else {
            continue;
        };
        // SVG beats PNG; within each, bigger sources are drawn with more detail
        // (Papirus ships simplified art at 16x16).
        let score = match ext.to_str() {
            Some("svg") => {
                1_000_000
                    + if path.components().any(|c| c.as_os_str() == "scalable") {
                        512
                    } else {
                        size_of(&path)
                    }
            }
            Some("png") => size_of(&path),
            _ => continue,
        };
        let best = found.get(stem).is_some_and(|(s, _)| *s >= score);
        if !best {
            found.insert(stem.to_owned(), (score, path));
        }
    }
}

/// Size from a `48x48` or `48x48@2` path component; 1 if unknown.
fn size_of(path: &Path) -> u32 {
    path.components()
        .filter_map(|c| {
            c.as_os_str()
                .to_str()?
                .split(['x', '@'])
                .next()?
                .parse()
                .ok()
        })
        .next()
        .unwrap_or(1)
}

/// `theme`, the themes it inherits (from `index.theme`), then hicolor.
fn theme_chain(theme: &str, bases: &[PathBuf]) -> Vec<String> {
    let mut chain = Vec::new();
    let mut queue = vec![theme.to_owned()];
    while let Some(t) = queue.pop() {
        if chain.contains(&t) || chain.len() > 8 {
            continue;
        }
        let inherits = bases
            .iter()
            .find_map(|b| std::fs::read_to_string(b.join(&t).join("index.theme")).ok())
            .and_then(|text| {
                text.lines()
                    .find_map(|l| l.strip_prefix("Inherits=").map(str::to_owned))
            });
        chain.push(t);
        if let Some(parents) = inherits {
            queue.extend(parents.split(',').rev().map(|p| p.trim().to_owned()));
        }
    }
    if !chain.iter().any(|t| t == "hicolor") {
        chain.push("hicolor".into());
    }
    chain
}

/// `gtk-icon-theme-name` from the user's GTK settings.
fn gtk_icon_theme() -> Option<String> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    ["gtk-4.0", "gtk-3.0"].iter().find_map(|v| {
        let text = std::fs::read_to_string(config.join(v).join("settings.ini")).ok()?;
        text.lines().find_map(|l| {
            l.trim()
                .strip_prefix("gtk-icon-theme-name=")
                .map(|s| s.trim().to_owned())
        })
    })
}

/// `$XDG_DATA_HOME` then `$XDG_DATA_DIRS`, without duplicates.
pub fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    let system =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let mut dirs: Vec<PathBuf> = Vec::new();
    for d in home.into_iter().chain(
        system
            .split(':')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from),
    ) {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sizes_from_paths() {
        assert_eq!(size_of(Path::new("/i/hicolor/48x48/apps/a.png")), 48);
        assert_eq!(size_of(Path::new("/i/Papirus/64x64@2/apps/a.png")), 64);
        assert_eq!(size_of(Path::new("/i/x/apps/a.png")), 1);
    }

    #[test]
    fn prefers_svg_then_larger_png() {
        let dir = std::env::temp_dir().join(format!("rune-icons-{}", std::process::id()));
        for f in [
            "16x16/apps/a.png",
            "48x48/apps/a.png",
            "16x16/apps/b.png",
            "scalable/apps/b.svg",
            "48x48/mimetypes/c.png",
        ] {
            let p = dir.join("theme").join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "").unwrap();
        }
        let mut found = HashMap::new();
        walk(&dir.join("theme"), 0, &mut found);
        assert!(found["a"].1.ends_with("48x48/apps/a.png"));
        assert!(found["b"].1.ends_with("scalable/apps/b.svg"));
        assert!(!found.contains_key("c"), "only app icons are indexed");
        std::fs::remove_dir_all(dir).unwrap();
    }
}

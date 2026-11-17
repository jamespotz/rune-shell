//! Every file the theme engine touches is backed up the first time, so
//! [`Backup::restore`] puts the user's setup back exactly as it was before Rune
//! changed it: file contents, symlinks (e.g. `gtk.css -> ...`), files that did
//! not exist, and GNOME settings.
//!
//! The manifest is written before each change, so a crash mid-apply still restores.

use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    /// Regular file, copied to `files/<n>`.
    File(PathBuf, usize),
    /// Symlink and its target.
    Link(PathBuf, PathBuf),
    /// Did not exist; restore deletes it.
    Absent(PathBuf),
    /// Directory we created; restore removes it if it is empty again.
    Dir(PathBuf),
    /// `gsettings` schema, key, previous value (GVariant text).
    Setting(String, String, String),
}

/// The backup store, normally `$XDG_STATE_HOME/rune-shell/theme-backup`.
#[derive(Debug)]
pub struct Backup {
    dir: PathBuf,
    entries: Vec<Entry>,
}

impl Backup {
    /// The default store.
    pub fn open_default() -> io::Result<Self> {
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
            .ok_or_else(|| io::Error::other("HOME is not set"))?;
        Self::open(state.join("rune-shell/theme-backup"))
    }

    /// Loads (or starts) a store in `dir`.
    pub fn open(dir: PathBuf) -> io::Result<Self> {
        let entries = match std::fs::read_to_string(dir.join("manifest")) {
            Ok(text) => text.lines().filter_map(parse_line).collect(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        Ok(Self { dir, entries })
    }

    /// Whether anything is backed up.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Writes `contents` to `path`, backing the original up first. A symlink at
    /// `path` is replaced by a file (its target is left untouched). Returns
    /// whether the file changed.
    pub fn write(&mut self, path: &Path, contents: &str) -> io::Result<bool> {
        let is_link = path
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink());
        if !is_link && std::fs::read_to_string(path).is_ok_and(|old| old == contents) {
            return Ok(false);
        }
        if let Some(parent) = path.parent() {
            // Record every directory we are about to create, outermost first.
            let missing: Vec<&Path> = parent.ancestors().take_while(|d| !d.exists()).collect();
            for dir in missing.into_iter().rev() {
                self.entries.push(Entry::Dir(dir.to_path_buf()));
            }
            self.record(path)?; // also saves the manifest
            std::fs::create_dir_all(parent)?;
        } else {
            self.record(path)?;
        }
        let tmp = path.with_extension("rune-tmp");
        std::fs::write(&tmp, contents)?;
        std::fs::rename(&tmp, path)?; // replaces a symlink itself, never its target
        Ok(true)
    }

    /// Sets a GNOME setting, remembering its previous value.
    pub fn set_gsetting(&mut self, schema: &str, key: &str, value: &str) -> io::Result<()> {
        let known = self
            .entries
            .iter()
            .any(|e| matches!(e, Entry::Setting(s, k, _) if s == schema && k == key));
        if !known {
            let out = Command::new("gsettings")
                .args(["get", schema, key])
                .output()?;
            if !out.status.success() {
                return Err(io::Error::other(format!(
                    "gsettings get {schema} {key} failed"
                )));
            }
            let old = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            self.entries
                .push(Entry::Setting(schema.into(), key.into(), old));
            self.save()?;
        }
        run_gsettings(schema, key, value)
    }

    fn record(&mut self, path: &Path) -> io::Result<()> {
        let known = self.entries.iter().any(|e| match e {
            Entry::File(p, _) | Entry::Link(p, _) | Entry::Absent(p) => p == path,
            Entry::Dir(_) | Entry::Setting(..) => false,
        });
        if known {
            return Ok(());
        }
        let text = path.to_string_lossy();
        if text.contains(['\t', '\n']) {
            return Err(io::Error::other(format!(
                "refusing to back up odd path {text:?}"
            )));
        }
        let entry = match path.symlink_metadata() {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Entry::Absent(path.to_path_buf()),
            Err(e) => return Err(e),
            Ok(m) if m.file_type().is_symlink() => {
                Entry::Link(path.to_path_buf(), std::fs::read_link(path)?)
            }
            Ok(_) => {
                let n = self.entries.len();
                std::fs::create_dir_all(self.dir.join("files"))?;
                std::fs::copy(path, self.dir.join("files").join(n.to_string()))?;
                Entry::File(path.to_path_buf(), n)
            }
        };
        self.entries.push(entry);
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let text: String = self.entries.iter().map(|e| format_line(e) + "\n").collect();
        let tmp = self.dir.join("manifest.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, self.dir.join("manifest"))
    }

    /// Puts everything back and clears the store. Returns what was restored;
    /// problems are reported but don't stop the rest from being restored.
    pub fn restore(self) -> (Vec<String>, Vec<String>) {
        let (mut done, mut failed) = (Vec::new(), Vec::new());
        for entry in self.entries.iter().rev() {
            let result = match entry {
                Entry::File(p, n) => copy_back(&self.dir.join("files").join(n.to_string()), p),
                Entry::Link(p, target) => {
                    remove_if_exists(p).and_then(|()| std::os::unix::fs::symlink(target, p))
                }
                Entry::Absent(p) => remove_if_exists(p),
                // Left in place if the user has put something else in it since.
                Entry::Dir(p) => {
                    let _ = std::fs::remove_dir(p);
                    Ok(())
                }
                Entry::Setting(schema, key, value) => run_gsettings(schema, key, value),
            };
            let label = match entry {
                Entry::File(p, _) | Entry::Link(p, _) | Entry::Absent(p) | Entry::Dir(p) => {
                    p.display().to_string()
                }
                Entry::Setting(s, k, _) => format!("gsettings {s} {k}"),
            };
            match result {
                Ok(()) => done.push(label),
                Err(e) => failed.push(format!("{label}: {e}")),
            }
        }
        if failed.is_empty() {
            let _ = std::fs::remove_dir_all(&self.dir);
            if let Some(parent) = self.dir.parent() {
                let _ = std::fs::remove_dir(parent); // only succeeds when empty
            }
        }
        (done, failed)
    }
}

fn copy_back(from: &Path, to: &Path) -> io::Result<()> {
    let tmp = to.with_extension("rune-tmp");
    std::fs::copy(from, &tmp)?;
    std::fs::rename(tmp, to)
}

fn remove_if_exists(p: &Path) -> io::Result<()> {
    match std::fs::remove_file(p) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn run_gsettings(schema: &str, key: &str, value: &str) -> io::Result<()> {
    let status = Command::new("gsettings")
        .args(["set", schema, key, value])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "gsettings set {schema} {key} failed"
        )))
    }
}

fn format_line(e: &Entry) -> String {
    match e {
        Entry::File(p, n) => format!("file\t{}\t{n}", p.display()),
        Entry::Link(p, t) => format!("link\t{}\t{}", p.display(), t.display()),
        Entry::Absent(p) => format!("absent\t{}", p.display()),
        Entry::Dir(p) => format!("dir\t{}", p.display()),
        Entry::Setting(s, k, v) => format!("gsetting\t{s}\t{k}\t{v}"),
    }
}

fn parse_line(line: &str) -> Option<Entry> {
    let mut f = line.split('\t');
    Some(match (f.next()?, f.next()?) {
        ("file", p) => Entry::File(p.into(), f.next()?.parse().ok()?),
        ("link", p) => Entry::Link(p.into(), f.next()?.into()),
        ("absent", p) => Entry::Absent(p.into()),
        ("dir", p) => Entry::Dir(p.into()),
        ("gsetting", s) => Entry::Setting(s.into(), f.next()?.into(), f.next()?.into()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_files_links_and_absent_paths() {
        let root = std::env::temp_dir().join(format!("rune-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let file = home.join("kitty.conf");
        std::fs::write(&file, "font_size 12\n").unwrap();
        let target = home.join("dank.css");
        std::fs::write(&target, "dank").unwrap();
        let link = home.join("gtk.css");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let fresh = home.join("themes/rune.conf");

        let mut b = Backup::open(root.join("store")).unwrap();
        assert!(b.write(&file, "font_size 12\ninclude rune.conf\n").unwrap());
        assert!(b.write(&file, "changed twice\n").unwrap()); // backup keeps the first original
        assert!(b.write(&link, "rune css").unwrap());
        assert!(b.write(&fresh, "colors").unwrap());
        assert!(
            !b.write(&fresh, "colors").unwrap(),
            "unchanged content is not rewritten"
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "dank",
            "symlink target untouched"
        );
        assert!(!link.symlink_metadata().unwrap().file_type().is_symlink());

        // A fresh handle reads the manifest from disk, as after a restart.
        let (done, failed) = Backup::open(root.join("store")).unwrap().restore();
        assert!(failed.is_empty(), "{failed:?}");
        assert_eq!(done.len(), 4); // kitty.conf, gtk.css link, rune.conf, themes/
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "font_size 12\n");
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
        assert!(!fresh.exists());
        assert!(
            !home.join("themes").exists(),
            "directories we created are removed again"
        );
        assert!(
            !root.join("store").exists(),
            "store is cleared after a clean restore"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manifest_round_trips() {
        let e = Entry::Setting(
            "org.gnome.desktop.interface".into(),
            "color-scheme".into(),
            "'prefer-dark'".into(),
        );
        assert_eq!(parse_line(&format_line(&e)), Some(e));
        assert_eq!(parse_line("garbage"), None);
    }
}

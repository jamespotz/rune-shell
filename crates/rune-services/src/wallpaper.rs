//! Wallpapers: images and GIFs through `awww`, videos through `mpvpaper`,
//! plus cached thumbnails for the picker.

use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const IMAGES: [&str; 7] = ["jpg", "jpeg", "png", "webp", "gif", "bmp", "avif"];
const VIDEOS: [&str; 6] = ["mp4", "webm", "mkv", "mov", "m4v", "avi"];

/// One entry in the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wallpaper {
    pub path: PathBuf,
    /// Cached thumbnail, once generated.
    pub thumb: Option<PathBuf>,
    pub video: bool,
}

fn ext_in(path: &Path, list: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| list.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

pub fn is_image(path: &Path) -> bool {
    ext_in(path, &IMAGES)
}

pub fn is_video(path: &Path) -> bool {
    ext_in(path, &VIDEOS)
}

/// Images and videos in `dir`, by name. Thumbnails that already exist are filled in.
pub fn list(dir: &Path) -> Vec<Wallpaper> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Wallpaper> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| ext_in(p, &IMAGES) || ext_in(p, &VIDEOS))
        .map(|path| Wallpaper {
            thumb: cache_path(&path, "thumb169").filter(|t| t.exists()),
            video: is_video(&path),
            path,
        })
        .collect();
    out.sort_by_cached_key(|w| {
        w.path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
    });
    out
}

/// Makes (or reuses) a small PNG thumbnail. Slow for big images: call from a blocking thread.
pub fn thumbnail(path: &Path) -> Option<PathBuf> {
    let thumb = cache_path(path, "thumb169")?;
    if thumb.exists() {
        return Some(thumb);
    }
    std::fs::create_dir_all(thumb.parent()?).ok()?;
    let source = if is_video(path) {
        video_frame(path)?
    } else {
        path.to_path_buf()
    };
    // Cropped to the picker's 16:9 tiles, so the UI never has to clip.
    let img =
        image::open(&source)
            .ok()?
            .resize_to_fill(320, 180, image::imageops::FilterType::Triangle);
    img.save_with_format(&thumb, image::ImageFormat::Png).ok()?;
    Some(thumb)
}

/// A still frame of a video (1 s in), cached as PNG. Needs `ffmpeg`.
pub fn video_frame(path: &Path) -> Option<PathBuf> {
    let frame = cache_path(path, "frame")?;
    if frame.exists() {
        return Some(frame);
    }
    std::fs::create_dir_all(frame.parent()?).ok()?;
    let ok = Command::new("ffmpeg")
        .args(["-loglevel", "error", "-y", "-ss", "1", "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-vf", "scale=960:-2"])
        .arg(&frame)
        .stdin(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    ok.then_some(frame)
}

/// `$XDG_CACHE_HOME/rune-shell/wallpaper/<kind>-<hash of path, size, mtime>.png`
pub(crate) fn cache_path(path: &Path, kind: &str) -> Option<PathBuf> {
    let meta = std::fs::metadata(path).ok()?;
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    meta.len().hash(&mut h);
    meta.modified().ok().hash(&mut h);
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(
        base.join("rune-shell/wallpaper")
            .join(format!("{kind}-{:016x}.png", h.finish())),
    )
}

/// What awww shows on the first output, if anything (`awww query`).
pub fn current() -> Option<PathBuf> {
    let out = Command::new("awww").arg("query").output().ok()?;
    parse_query(&String::from_utf8_lossy(&out.stdout))
}

fn parse_query(text: &str) -> Option<PathBuf> {
    text.lines().find_map(|l| {
        l.split_once("image: ")
            .map(|(_, p)| PathBuf::from(p.trim()))
    })
}

/// Shows `path` on every output. Videos replace awww with mpvpaper; images stop mpvpaper.
pub fn set(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("{} does not exist", path.display()));
    }
    // Only one wallpaper program may draw at a time.
    let _ = Command::new("pkill").args(["-x", "mpvpaper"]).status();
    if is_video(path) {
        use std::os::unix::process::CommandExt;
        let child = Command::new("mpvpaper")
            .args(["-o", "no-audio --loop-file=inf --panscan=1.0", "*"])
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0) // keeps playing if the shell restarts
            .spawn()
            .map_err(|e| format!("cannot run mpvpaper: {e}"))?;
        // Reap it whenever it exits so it never lingers as a zombie.
        std::thread::spawn(move || {
            let mut child = child;
            let _ = child.wait();
        });
        return Ok(());
    }
    let out = Command::new("awww")
        .arg("img")
        .arg(path)
        .args([
            "--transition-type",
            "grow",
            "--transition-pos",
            "top",
            "--transition-duration",
            "1",
        ])
        .output()
        .map_err(|e| format!("cannot run awww: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "awww: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_awww_query() {
        let q = ": DP-1: 2752x1152, scale: 1.25, currently displaying: image: /home/u/Pictures/Wall papers/a b.webp\n";
        assert_eq!(
            parse_query(q),
            Some(PathBuf::from("/home/u/Pictures/Wall papers/a b.webp"))
        );
        assert_eq!(
            parse_query(": DP-1: 1920x1080, currently displaying: color: 000000"),
            None
        );
    }

    #[test]
    fn classifies_files() {
        assert!(is_video(Path::new("/w/bus.MP4")));
        assert!(!is_video(Path::new("/w/a.webp")));
        assert!(ext_in(Path::new("/w/a.WebP"), &IMAGES));
    }
}

//! Rune's lock screen: the `[lock]` settings, the hyprlock config generated from
//! them, and the background images it needs. Rune owns
//! `~/.config/hypr/rune-hyprlock.conf` and runs `hyprlock --config` on it, so the
//! user's own `hyprlock.conf` is never read or changed.

use serde::{Deserialize, Serialize};
use std::{
    fmt::Write,
    path::{Path, PathBuf},
};

/// What fills the screen behind the clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Background {
    #[default]
    Wallpaper,
    /// What was on screen when locking (hyprlock grabs it).
    Screenshot,
    /// The theme's background color.
    Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Blur,
    Pixelate,
}

/// `[lock]` in `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Lock with Rune's hyprlock config instead of `power.lock_command`.
    pub enabled: bool,
    pub background: Background,
    /// `None` follows the look: pixelate in `pixel`, blur in `smooth`.
    pub effect: Option<Effect>,
    /// How strong the blur or how big the pixels, 0-100.
    pub strength: u8,
    /// Darkening of the background, 0-90 percent.
    pub dim: u8,
    pub clock_24h: bool,
    pub show_date: bool,
    /// The picture in `~/.face` and the user name.
    pub show_user: bool,
    pub show_media: bool,
    /// Keyboard layout, network and volume in the top right.
    pub show_status: bool,
    /// Placeholder in the password field.
    pub hint: String,
    /// Seconds after locking when any key unlocks, 0-30.
    pub grace_seconds: u8,
    pub hide_cursor: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            background: Background::Wallpaper,
            effect: None,
            strength: 60,
            dim: 45,
            clock_24h: true,
            show_date: true,
            show_user: true,
            show_media: true,
            show_status: false,
            hint: "Enter your password".into(),
            grace_seconds: 0,
            hide_cursor: true,
        }
    }
}

pub const MAX_HINT_CHARS: usize = 64;
pub const MAX_STRENGTH: u8 = 100;
pub const MAX_DIM: u8 = 90;
pub const MAX_GRACE_SECONDS: u8 = 30;

impl Settings {
    /// Clamps every value into range; the config file is user-editable.
    pub fn sanitize(&mut self) {
        self.strength = self.strength.min(MAX_STRENGTH);
        self.dim = self.dim.min(MAX_DIM);
        self.grace_seconds = self.grace_seconds.min(MAX_GRACE_SECONDS);
        self.hint = clean_hint(&self.hint);
    }

    /// The effect actually drawn. hyprlock grabs screenshots while locking, too
    /// late for Rune to pixelate them, so those always blur.
    pub fn effect(&self, pixel_look: bool) -> Effect {
        if self.background == Background::Screenshot {
            return Effect::Blur;
        }
        self.effect.unwrap_or(if pixel_look {
            Effect::Pixelate
        } else {
            Effect::Blur
        })
    }

    /// Pixel block size on a 1920-pixel-wide image, 4-64.
    pub fn block_size(&self) -> u32 {
        4 + u32::from(self.strength.min(MAX_STRENGTH)) * 60 / 100
    }
}

/// Keeps the hint printable and short.
pub fn clean_hint(hint: &str) -> String {
    hint.chars()
        .filter(|c| !c.is_control())
        .take(MAX_HINT_CHARS)
        .collect()
}

/// Everything besides [`Settings`] the generated config depends on.
#[derive(Debug, Clone, Copy)]
pub struct Theme<'a> {
    pub pixel: bool,
    pub reduce_motion: bool,
    pub font: &'a str,
    pub icon_font: &'a str,
    /// RGBA.
    pub foreground: [u8; 4],
    pub background: [u8; 4],
    pub accent: [u8; 4],
    pub urgent: [u8; 4],
    /// The image behind the clock, already pixelated when that effect is on.
    /// `None` falls back to the background color.
    pub image: Option<&'a Path>,
    pub avatar: Option<&'a Path>,
    /// This executable, for the commands hyprlock polls.
    pub rune: &'a Path,
}

/// `~/.config/hypr/rune-hyprlock.conf`.
pub fn config_path() -> Option<PathBuf> {
    Some(crate::hyprland_config::hypr_dir()?.join("rune-hyprlock.conf"))
}

/// Shell command that locks with Rune's config, unless a lock is already up.
pub fn command(grace_seconds: u8) -> Option<String> {
    let path = config_path()?;
    Some(format!(
        "pidof hyprlock || hyprlock --config {} --grace {}",
        shell_quote(&path.to_string_lossy()),
        grace_seconds.min(MAX_GRACE_SECONDS)
    ))
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// A hyprlang value: one line, with `#` escaped so it doesn't start a comment.
fn value(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .replace('#', "##")
}

/// Text hyprlock renders as Pango markup.
pub fn markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().filter(|c| !c.is_control()) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\'' => out.push_str("&apos;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

fn rgba([r, g, b, a]: [u8; 4]) -> String {
    format!("rgba({r:02x}{g:02x}{b:02x}{a:02x})")
}

fn alpha([r, g, b, _]: [u8; 4], a: u8) -> [u8; 4] {
    [r, g, b, a]
}

const BLACK: [u8; 4] = [0, 0, 0, 0xff];

/// One hyprlock widget block.
struct Block<'a> {
    kind: &'a str,
    lines: Vec<(&'a str, String)>,
}

impl<'a> Block<'a> {
    fn new(kind: &'a str) -> Self {
        Self {
            kind,
            lines: Vec::new(),
        }
    }

    fn set(mut self, key: &'a str, val: impl Into<String>) -> Self {
        self.lines.push((key, val.into()));
        self
    }

    fn at(self, x: i32, y: i32, halign: &str, valign: &str) -> Self {
        self.set("position", format!("{x}, {y}"))
            .set("halign", halign.to_owned())
            .set("valign", valign.to_owned())
    }

    /// The same widget in black, nudged down-right, under the original: the pixel look's hard shadow.
    fn shadowed(self, pixel: bool, offset: i32, out: &mut String) {
        if pixel {
            let mut shadow = Block::new(self.kind);
            for (key, val) in &self.lines {
                let val = match *key {
                    "color" | "font_color" | "border_color" => rgba(BLACK),
                    "position" => nudge(val, offset),
                    _ => val.clone(),
                };
                shadow = shadow.set(key, val);
            }
            shadow.set("zindex", "0").write(out);
        }
        self.set("zindex", "1").write(out);
    }

    fn write(self, out: &mut String) {
        let _ = writeln!(out, "{} {{\n    monitor =", self.kind);
        for (key, val) in self.lines {
            let _ = writeln!(out, "    {key} = {}", value(&val));
        }
        out.push_str("}\n\n");
    }
}

fn nudge(position: &str, offset: i32) -> String {
    let mut parts = position
        .split(',')
        .map(|p| p.trim().parse::<i32>().unwrap_or(0));
    let x = parts.next().unwrap_or(0).saturating_add(offset);
    let y = parts.next().unwrap_or(0).saturating_sub(offset);
    format!("{x}, {y}")
}

/// A shape the size of a widget, for widgets whose fill can't be recolored to a shadow.
fn hard_shadow(out: &mut String, size: (i32, i32), at: (i32, i32), offset: i32) {
    Block::new("shape")
        .set("size", format!("{}, {}", size.0, size.1))
        .set("color", rgba(BLACK))
        .set("rounding", "0")
        .at(at.0 + offset, at.1 - offset, "center", "center")
        .set("zindex", "0")
        .write(out);
}

/// The whole `rune-hyprlock.conf`.
pub fn render(settings: &Settings, theme: &Theme<'_>) -> String {
    let mut out = String::from(
        "# Written by Rune Shell from [lock] in config.toml. Edits here are overwritten;\n\
         # change the Lock Screen settings instead.\n\n",
    );
    let pixel = theme.pixel;
    let fg = theme.foreground;
    let rune = shell_quote(&theme.rune.to_string_lossy());

    // Not widgets, so no `monitor =` line, which hyprlock rejects here.
    let _ = writeln!(
        out,
        "general {{\n    hide_cursor = {}\n    ignore_empty_input = true\n}}\n\n\
         animations {{\n    enabled = {}\n}}\n",
        settings.hide_cursor, !theme.reduce_motion
    );

    let mut background = Block::new("background").set("color", rgba(alpha(theme.background, 0xff)));
    let blur = settings.effect(pixel) == Effect::Blur && settings.strength > 0;
    let image = match settings.background {
        Background::Wallpaper => theme.image.map(|p| p.to_string_lossy().into_owned()),
        Background::Screenshot => Some("screenshot".to_owned()),
        Background::Color => None,
    };
    if let Some(path) = image {
        background = background
            .set("path", path)
            .set("blur_passes", if blur { "3" } else { "0" })
            .set(
                "blur_size",
                (1 + u32::from(settings.strength) / 8).to_string(),
            )
            .set(
                "brightness",
                format!("{:.2}", 1. - f32::from(settings.dim.min(MAX_DIM)) / 100.),
            );
    }
    background.write(&mut out);

    let shadow = if pixel { 6 } else { 0 };
    let clock = if settings.clock_24h {
        "$TIME"
    } else {
        "$TIME12"
    };
    let clock = if pixel {
        clock.to_owned()
    } else {
        format!("<span font_weight=\"light\">{clock}</span>")
    };
    Block::new("label")
        .set("text", clock)
        .set("color", rgba(fg))
        .set("font_size", if pixel { "132" } else { "120" })
        .set("font_family", theme.font.to_owned())
        .at(0, 210, "center", "center")
        .shadowed(pixel, shadow, &mut out);

    if settings.show_date {
        let format = if pixel { "%^A, %^B %-d" } else { "%A, %B %-d" };
        Block::new("label")
            .set("text", format!("cmd[update:60000] date +'{format}'"))
            .set("color", rgba(alpha(fg, 0xd9)))
            .set("font_size", "20")
            .set("font_family", theme.font.to_owned())
            .at(0, 110, "center", "center")
            .shadowed(pixel, 2, &mut out);
    }

    if settings.show_user {
        if let Some(avatar) = theme.avatar {
            if pixel {
                hard_shadow(&mut out, (80, 80), (0, 10), 4);
            }
            Block::new("image")
                .set("path", avatar.to_string_lossy().into_owned())
                .set("size", "76")
                .set("rounding", if pixel { "0" } else { "-1" })
                .set("border_size", "2")
                .set(
                    "border_color",
                    rgba(alpha(fg, if pixel { 0xff } else { 0x80 })),
                )
                .at(0, 10, "center", "center")
                .set("zindex", "1")
                .write(&mut out);
        }
        Block::new("label")
            .set("text", "$USER")
            .set("color", rgba(fg))
            .set("font_size", "15")
            .set("font_family", theme.font.to_owned())
            .at(0, -50, "center", "center")
            .shadowed(pixel, 2, &mut out);
    }

    if pixel {
        hard_shadow(&mut out, (300, 50), (0, -110), 4);
    }
    Block::new("input-field")
        .set("size", "300, 50")
        .set("outline_thickness", "2")
        .set("rounding", if pixel { "0" } else { "-1" })
        .set("dots_size", "0.25")
        .set("dots_spacing", "0.4")
        .set("dots_rounding", if pixel { "0" } else { "-1" })
        .set("outer_color", rgba(theme.accent))
        .set(
            "inner_color",
            rgba(alpha(theme.background, if pixel { 0xd9 } else { 0x73 })),
        )
        .set("font_color", rgba(fg))
        .set("font_family", theme.font.to_owned())
        .set("check_color", rgba(theme.accent))
        .set("fail_color", rgba(theme.urgent))
        .set("fail_text", "$FAIL")
        .set("fade_on_empty", "false")
        .set("placeholder_text", markup(&settings.hint))
        .at(0, -110, "center", "center")
        .set("zindex", "1")
        .write(&mut out);

    if settings.show_media {
        Block::new("label")
            .set("text", format!("cmd[update:2000] {rune} lock-media"))
            .set("color", rgba(fg))
            .set("font_size", "13")
            .set("font_family", theme.font.to_owned())
            .at(0, 40, "center", "bottom")
            .shadowed(pixel, 2, &mut out);
    }

    if settings.show_status {
        Block::new("label")
            .set("text", format!("cmd[update:5000] {rune} lock-status"))
            .set("color", rgba(fg))
            .set("font_size", "16")
            .set("font_family", theme.icon_font.to_owned())
            .at(-30, -24, "right", "top")
            .shadowed(pixel, 2, &mut out);
        Block::new("label")
            .set("text", "$LAYOUT")
            .set("color", rgba(fg))
            .set("font_size", "13")
            .set("font_family", theme.font.to_owned())
            .at(-100, -26, "right", "top")
            .shadowed(pixel, 2, &mut out);
    }
    out
}

/// Writes `rune-hyprlock.conf` in one step, through any symlink to it. Blocking.
pub fn save(conf: &str) -> Result<PathBuf, String> {
    let path = config_path().ok_or("HOME is not set")?;
    let target = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let tmp = target.with_file_name(".rune-hyprlock.conf.rune-tmp");
    std::fs::write(&tmp, conf).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &target)
        .map_err(|e| format!("cannot replace {}: {e}", target.display()))?;
    Ok(path)
}

/// `~/.face`, or the first image inside a `~/.face` folder.
pub fn avatar() -> Option<PathBuf> {
    let face = PathBuf::from(std::env::var_os("HOME")?).join(".face");
    if face.is_file() {
        return Some(face);
    }
    let icon = face.with_extension("icon");
    if icon.is_file() {
        return Some(icon);
    }
    let mut images: Vec<PathBuf> = std::fs::read_dir(&face)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| crate::wallpaper::is_image(p) && p.is_file())
        .collect();
    images.sort();
    images.into_iter().next()
}

/// A full-size pixelated copy of `source`, cached. Slow: call from a blocking thread.
pub fn pixelated(source: &Path, block: u32) -> Result<PathBuf, String> {
    let out = crate::wallpaper::cache_path(source, &format!("lock-px{block}"))
        .ok_or_else(|| format!("cannot read {}", source.display()))?;
    if !out.exists() {
        pixelate_file(source, block, &out)?;
    }
    Ok(out)
}

fn pixelate_file(source: &Path, block: u32, out: &Path) -> Result<(), String> {
    let img = open_rgb(source)?;
    save_png(&blocky(&img, block), out)
}

fn open_rgb(path: &Path) -> Result<image::RgbImage, String> {
    Ok(image::open(path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?
        .to_rgb8())
}

/// Blocks are sized for a 1920-wide screen and scale with the image.
fn blocky(img: &image::RgbImage, block: u32) -> image::RgbImage {
    let (width, height) = img.dimensions();
    let block = (block.saturating_mul(width) / 1920).max(1);
    let small = image::imageops::resize(
        img,
        width.div_ceil(block),
        height.div_ceil(block),
        image::imageops::FilterType::Triangle,
    );
    image::imageops::resize(&small, width, height, image::imageops::FilterType::Nearest)
}

/// The 320×180 picture behind the settings preview, with the effect applied. Slow: call from a blocking thread.
pub fn preview(thumbnail: &Path, effect: Effect, strength: u8) -> Result<PathBuf, String> {
    let kind = match effect {
        Effect::Blur => format!("lock-blur{strength}"),
        Effect::Pixelate => format!("lock-pxprev{strength}"),
    };
    let out = crate::wallpaper::cache_path(thumbnail, &kind)
        .ok_or_else(|| format!("cannot read {}", thumbnail.display()))?;
    if !out.exists() {
        preview_file(thumbnail, effect, strength, &out)?;
    }
    Ok(out)
}

fn preview_file(thumbnail: &Path, effect: Effect, strength: u8, out: &Path) -> Result<(), String> {
    let img = open_rgb(thumbnail)?;
    let strength = strength.min(MAX_STRENGTH);
    let img = match effect {
        Effect::Blur => image::imageops::blur(&img, f32::from(strength) / 100. * 4.),
        Effect::Pixelate => {
            let settings = Settings {
                strength,
                ..Settings::default()
            };
            blocky(&img, settings.block_size())
        }
    };
    save_png(&img, out)
}

fn save_png(img: &image::RgbImage, out: &Path) -> Result<(), String> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    img.save_with_format(out, image::ImageFormat::Png)
        .map_err(|e| format!("cannot write {}: {e}", out.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme(rune: &Path) -> Theme<'_> {
        Theme {
            pixel: false,
            reduce_motion: false,
            font: "Noto Sans",
            icon_font: "Material Symbols Rounded",
            foreground: [0xec, 0xdc, 0xc8, 0xff],
            background: [0x14, 0x0e, 0x0b, 0xff],
            accent: [0xd4, 0xa3, 0x73, 0xff],
            urgent: [0xd0, 0x67, 0x5a, 0xff],
            image: Some(Path::new("/w/be still.png")),
            avatar: Some(Path::new("/home/u/.face")),
            rune,
        }
    }

    #[test]
    fn sanitize_clamps_hostile_values() {
        let mut s = Settings {
            strength: 255,
            dim: 200,
            grace_seconds: 255,
            hint: format!("a\nb\u{7}{}", "x".repeat(500)),
            ..Settings::default()
        };
        s.sanitize();
        assert_eq!((s.strength, s.dim, s.grace_seconds), (100, 90, 30));
        assert_eq!(s.hint.chars().count(), MAX_HINT_CHARS);
        assert!(s.hint.starts_with("abx"));
        assert_eq!(s.block_size(), 64);
    }

    #[test]
    fn screenshots_only_blur_and_effect_follows_the_look() {
        let mut s = Settings::default();
        assert_eq!(s.effect(true), Effect::Pixelate);
        assert_eq!(s.effect(false), Effect::Blur);
        s.effect = Some(Effect::Blur);
        assert_eq!(s.effect(true), Effect::Blur);
        s.effect = Some(Effect::Pixelate);
        s.background = Background::Screenshot;
        assert_eq!(s.effect(false), Effect::Blur);
    }

    #[test]
    fn hostile_text_cannot_break_out_of_its_line() {
        let s = Settings {
            hint: "x\n}\ninput-field { # <b>&".into(),
            ..Settings::default()
        };
        let conf = render(&s, &theme(Path::new("/bin/rune#x")));
        let hint = conf
            .lines()
            .find(|l| l.trim_start().starts_with("placeholder_text"))
            .unwrap();
        assert_eq!(
            hint.trim(),
            "placeholder_text = x}input-field { ## &lt;b&gt;&amp;"
        );
        assert!(conf.contains("cmd[update:2000] '/bin/rune##x' lock-media"));
        assert_eq!(conf.lines().filter(|l| *l == "input-field {").count(), 1);
    }

    #[test]
    fn renders_each_look() {
        let rune = Path::new("/usr/bin/rune-shell");
        let s = Settings {
            show_status: true,
            ..Settings::default()
        };
        let smooth = render(&s, &theme(rune));
        assert!(smooth.contains("path = /w/be still.png"));
        assert!(smooth.contains("blur_passes = 3"));
        assert!(smooth.contains("brightness = 0.55"));
        assert!(smooth.contains("rounding = -1"));
        assert!(smooth.contains("text = <span font_weight=\"light\">$TIME</span>"));
        assert!(!smooth.contains("shape {"));
        // `monitor` is a widget key; hyprlock rejects it in `general`.
        assert!(smooth.contains("general {\n    hide_cursor = true\n"));

        let pixel = render(
            &Settings {
                effect: Some(Effect::Pixelate),
                ..s.clone()
            },
            &Theme {
                pixel: true,
                font: "Tiny5",
                ..theme(rune)
            },
        );
        assert!(pixel.contains("blur_passes = 0"));
        assert!(pixel.contains("dots_rounding = 0"));
        assert!(pixel.contains("%^A, %^B %-d"));
        assert_eq!(pixel.matches("shape {").count(), 2);
        // Every pixel-look label has a black twin under it.
        assert_eq!(
            pixel.matches("label {").count(),
            2 * smooth.matches("label {").count()
        );

        let plain = render(
            &Settings {
                background: Background::Color,
                show_user: false,
                show_media: false,
                show_date: false,
                clock_24h: false,
                ..Settings::default()
            },
            &theme(rune),
        );
        assert!(!plain.contains("path ="));
        assert!(plain.contains("$TIME12</span>"));
        assert_eq!(plain.matches("label {").count(), 1);
    }

    #[test]
    fn command_quotes_the_config_path() {
        let cmd = command(200).unwrap();
        assert!(cmd.starts_with("pidof hyprlock || hyprlock --config '"));
        assert!(cmd.ends_with("rune-hyprlock.conf' --grace 30"));
        assert_eq!(shell_quote("a'b"), r"'a'\''b'");
    }

    #[test]
    fn pixelates_into_blocks() {
        let dir = std::env::temp_dir().join(format!("rune-lock-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.png");
        let img = image::RgbImage::from_fn(192, 108, |x, y| {
            image::Rgb([u8::try_from(x).unwrap(), u8::try_from(y).unwrap(), 0])
        });
        img.save(&src).unwrap();
        let out = dir.join("out.png");
        pixelate_file(&src, 640, &out).unwrap();
        let px = open_rgb(&out).unwrap();
        assert_eq!(px.dimensions(), (192, 108));
        // 640 px blocks on a 1920 screen are 64 px here (54 tall, two rows): one color per block.
        assert_eq!(px.get_pixel(0, 0), px.get_pixel(63, 53));
        assert_ne!(px.get_pixel(0, 0), px.get_pixel(64, 0));
        let prev = dir.join("prev.png");
        preview_file(&src, Effect::Blur, 50, &prev).unwrap();
        assert_eq!(open_rgb(&prev).unwrap().dimensions(), (192, 108));
        assert!(open_rgb(&dir.join("missing.png")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

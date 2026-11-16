//! Color themes: a [`Palette`] from a preset or generated from the wallpaper
//! (Material You), exported to apps by [`export`] with backups in [`backup`].

pub mod backup;
pub mod export;

use material_colors::{
    blend::harmonize,
    color::Rgb as MRgb,
    dynamic_color::DynamicScheme,
    hct::Hct,
    scheme::{
        Scheme,
        variant::{
            SchemeContent, SchemeExpressive, SchemeFidelity, SchemeFruitSalad, SchemeMonochrome,
            SchemeNeutral, SchemeRainbow, SchemeTonalSpot, SchemeVibrant,
        },
    },
};
use std::path::Path;

/// 8-bit sRGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    /// Parses `#rrggbb`.
    pub fn parse(s: &str) -> Option<Self> {
        let h = s.strip_prefix('#')?;
        if h.len() != 6 || !h.is_ascii() {
            return None;
        }
        let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Self([b(0)?, b(2)?, b(4)?]))
    }

    /// `#rrggbb`.
    pub fn hex(self) -> String {
        let [r, g, b] = self.0;
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    /// `r,g,b` (KDE color files).
    pub fn csv(self) -> String {
        let [r, g, b] = self.0;
        format!("{r},{g},{b}")
    }

    fn from_m(c: MRgb) -> Self {
        Self([c.red, c.green, c.blue])
    }

    fn to_m(self) -> MRgb {
        MRgb::new(self.0[0], self.0[1], self.0[2])
    }
}

/// Everything exporters need. Names follow Material roles, simplified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub name: String,
    pub dark: bool,
    /// Darkest surface (the notch, terminal backgrounds in some themes).
    pub bg_dim: Rgb,
    pub bg: Rgb,
    pub surface: Rgb,
    pub surface_high: Rgb,
    pub fg: Rgb,
    /// Secondary text.
    pub muted: Rgb,
    /// Borders, disabled text.
    pub outline: Rgb,
    pub primary: Rgb,
    pub on_primary: Rgb,
    pub secondary: Rgb,
    pub tertiary: Rgb,
    pub error: Rgb,
    /// Terminal colors 0-15.
    pub term: [Rgb; 16],
}

impl Palette {
    pub fn red(&self) -> Rgb {
        self.term[1]
    }
    pub fn green(&self) -> Rgb {
        self.term[2]
    }
    pub fn yellow(&self) -> Rgb {
        self.term[3]
    }
}

/// Material You scheme variants for wallpaper themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    TonalSpot,
    Vibrant,
    Expressive,
    FruitSalad,
    Content,
    Fidelity,
    Rainbow,
    Neutral,
    Monochrome,
}

impl Variant {
    pub const ALL: [Variant; 9] = [
        Variant::TonalSpot,
        Variant::Vibrant,
        Variant::Expressive,
        Variant::FruitSalad,
        Variant::Content,
        Variant::Fidelity,
        Variant::Rainbow,
        Variant::Neutral,
        Variant::Monochrome,
    ];

    /// Config name, e.g. `tonal_spot`.
    pub fn id(self) -> &'static str {
        match self {
            Variant::TonalSpot => "tonal_spot",
            Variant::Vibrant => "vibrant",
            Variant::Expressive => "expressive",
            Variant::FruitSalad => "fruit_salad",
            Variant::Content => "content",
            Variant::Fidelity => "fidelity",
            Variant::Rainbow => "rainbow",
            Variant::Neutral => "neutral",
            Variant::Monochrome => "monochrome",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Variant::TonalSpot => "Tonal spot",
            Variant::Vibrant => "Vibrant",
            Variant::Expressive => "Expressive",
            Variant::FruitSalad => "Fruit salad",
            Variant::Content => "Content",
            Variant::Fidelity => "Fidelity",
            Variant::Rainbow => "Rainbow",
            Variant::Neutral => "Neutral",
            Variant::Monochrome => "Monochrome",
        }
    }

    fn scheme(self, source: Hct, dark: bool) -> DynamicScheme {
        match self {
            Variant::TonalSpot => SchemeTonalSpot::new(source, dark, None).scheme,
            Variant::Vibrant => SchemeVibrant::new(source, dark, None).scheme,
            Variant::Expressive => SchemeExpressive::new(source, dark, None).scheme,
            Variant::FruitSalad => SchemeFruitSalad::new(source, dark, None).scheme,
            Variant::Content => SchemeContent::new(source, dark, None).scheme,
            Variant::Fidelity => SchemeFidelity::new(source, dark, None).scheme,
            Variant::Rainbow => SchemeRainbow::new(source, dark, None).scheme,
            Variant::Neutral => SchemeNeutral::new(source, dark, None).scheme,
            Variant::Monochrome => SchemeMonochrome::new(source, dark, None).scheme,
        }
    }
}

/// Builds a Material You palette from a seed color.
pub fn from_seed(seed: Rgb, variant: Variant, dark: bool) -> Palette {
    let s = Scheme::from(variant.scheme(Hct::new(seed.to_m()), dark));
    let c = Rgb::from_m;
    // Terminal hues stay recognisable (red is red) but lean towards the theme.
    let ansi = |base: &str, bright: bool| {
        let base = Rgb::parse(base).unwrap_or_default().to_m();
        let mut hct = Hct::new(harmonize(base, s.primary));
        let tone = match (dark, bright) {
            (true, false) => 70.0,
            (true, true) => 80.0,
            (false, false) => 40.0,
            (false, true) => 30.0,
        };
        hct.set_tone(tone);
        Rgb::from_m(hct.into())
    };
    let hues = [
        "#e5534b", "#57ab5a", "#c69026", "#539bf5", "#b083f0", "#39c5cf",
    ];
    let mut term = [Rgb::default(); 16];
    term[0] = c(s.surface_container_highest);
    term[7] = c(s.on_surface_variant);
    term[8] = c(s.outline);
    term[15] = c(s.on_surface);
    for (i, hue) in hues.iter().enumerate() {
        term[i + 1] = ansi(hue, false);
        term[i + 9] = ansi(hue, true);
    }
    Palette {
        name: format!("Wallpaper ({})", variant.label()),
        dark,
        bg_dim: c(s.surface_container_lowest),
        bg: c(s.surface),
        surface: c(s.surface_container),
        surface_high: c(s.surface_container_highest),
        fg: c(s.on_surface),
        muted: c(s.on_surface_variant),
        outline: c(s.outline),
        primary: c(s.primary),
        on_primary: c(s.on_primary),
        secondary: c(s.secondary),
        tertiary: c(s.tertiary),
        error: c(s.error),
        term,
    }
}

/// Picks the seed color of an image. Reads and decodes it; call from a blocking thread.
/// Videos are sampled with `ffmpeg` (one frame).
pub fn seed_from_image(path: &Path) -> Result<Rgb, String> {
    let img = if crate::wallpaper::is_video(path) {
        let frame = crate::wallpaper::video_frame(path)
            .ok_or("cannot read a frame from the video (is ffmpeg installed?)")?;
        image::open(&frame)
    } else {
        image::open(path)
    }
    .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    // Quantizing a few thousand pixels is plenty and keeps this fast.
    let small = img.thumbnail(112, 112).to_rgb8();
    let pixels: Vec<MRgb> = small
        .pixels()
        .map(|p| MRgb::new(p[0], p[1], p[2]))
        .collect();
    Ok(Rgb::from_m(material_colors::image::extract_color(&pixels)))
}

/// Built-in palettes for the palette selector.
pub fn presets() -> Vec<Palette> {
    PRESETS.iter().map(preset).collect()
}

/// A preset by name.
pub fn preset_named(name: &str) -> Option<Palette> {
    PRESETS.iter().find(|p| p.0 == name).map(preset)
}

type Row = (&'static str, bool, [&'static str; 12], [&'static str; 16]);

// name, dark, [bg_dim, bg, surface, surface_high, fg, muted, outline, primary, on_primary, secondary, tertiary, error], terminal 0-15
const PRESETS: [Row; 10] = [
    (
        "Catppuccin Mocha",
        true,
        [
            "#11111b", "#1e1e2e", "#313244", "#45475a", "#cdd6f4", "#a6adc8", "#6c7086", "#cba6f7",
            "#1e1e2e", "#89b4fa", "#f5c2e7", "#f38ba8",
        ],
        [
            "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
            "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8",
        ],
    ),
    (
        "Catppuccin Macchiato",
        true,
        [
            "#181926", "#24273a", "#363a4f", "#494d64", "#cad3f5", "#a5adcb", "#6e738d", "#c6a0f6",
            "#24273a", "#8aadf4", "#f5bde6", "#ed8796",
        ],
        [
            "#494d64", "#ed8796", "#a6da95", "#eed49f", "#8aadf4", "#f5bde6", "#8bd5ca", "#b8c0e0",
            "#5b6078", "#ed8796", "#a6da95", "#eed49f", "#8aadf4", "#f5bde6", "#8bd5ca", "#a5adcb",
        ],
    ),
    (
        "Catppuccin Frappé",
        true,
        [
            "#232634", "#303446", "#414559", "#51576d", "#c6d0f5", "#a5adce", "#737994", "#ca9ee6",
            "#303446", "#8caaee", "#f4b8e4", "#e78284",
        ],
        [
            "#51576d", "#e78284", "#a6d189", "#e5c890", "#8caaee", "#f4b8e4", "#81c8be", "#b5bfe2",
            "#626880", "#e78284", "#a6d189", "#e5c890", "#8caaee", "#f4b8e4", "#81c8be", "#a5adce",
        ],
    ),
    (
        "Catppuccin Latte",
        false,
        [
            "#dce0e8", "#eff1f5", "#ccd0da", "#bcc0cc", "#4c4f69", "#6c6f85", "#9ca0b0", "#8839ef",
            "#eff1f5", "#1e66f5", "#ea76cb", "#d20f39",
        ],
        [
            "#5c5f77", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb", "#179299", "#acb0be",
            "#6c6f85", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb", "#179299", "#bcc0cc",
        ],
    ),
    (
        "Gruvbox Dark",
        true,
        [
            "#1d2021", "#282828", "#3c3836", "#504945", "#ebdbb2", "#a89984", "#928374", "#fabd2f",
            "#282828", "#83a598", "#d3869b", "#fb4934",
        ],
        [
            "#282828", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a", "#a89984",
            "#928374", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c", "#ebdbb2",
        ],
    ),
    (
        "Nord",
        true,
        [
            "#242933", "#2e3440", "#3b4252", "#434c5e", "#eceff4", "#d8dee9", "#4c566a", "#88c0d0",
            "#2e3440", "#81a1c1", "#b48ead", "#bf616a",
        ],
        [
            "#3b4252", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
            "#4c566a", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#8fbcbb", "#eceff4",
        ],
    ),
    (
        "Tokyo Night",
        true,
        [
            "#16161e", "#1a1b26", "#24283b", "#292e42", "#c0caf5", "#a9b1d6", "#565f89", "#7aa2f7",
            "#1a1b26", "#bb9af7", "#7dcfff", "#f7768e",
        ],
        [
            "#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#a9b1d6",
            "#414868", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5",
        ],
    ),
    (
        "Rosé Pine",
        true,
        [
            "#12101a", "#191724", "#1f1d2e", "#26233a", "#e0def4", "#908caa", "#6e6a86", "#c4a7e7",
            "#191724", "#9ccfd8", "#ebbcba", "#eb6f92",
        ],
        [
            "#26233a", "#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba", "#e0def4",
            "#6e6a86", "#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba", "#e0def4",
        ],
    ),
    (
        "Dracula",
        true,
        [
            "#21222c", "#282a36", "#343746", "#44475a", "#f8f8f2", "#bfbfbf", "#6272a4", "#bd93f9",
            "#282a36", "#ff79c6", "#8be9fd", "#ff5555",
        ],
        [
            "#21222c", "#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2",
            "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5", "#d6acff", "#ff92df", "#a4ffff", "#ffffff",
        ],
    ),
    (
        "Everforest Dark",
        true,
        [
            "#232a2e", "#2d353b", "#343f44", "#3d484d", "#d3c6aa", "#9da9a0", "#7a8478", "#a7c080",
            "#2d353b", "#7fbbb3", "#d699b6", "#e67e80",
        ],
        [
            "#343f44", "#e67e80", "#a7c080", "#dbbc7f", "#7fbbb3", "#d699b6", "#83c092", "#d3c6aa",
            "#859289", "#e67e80", "#a7c080", "#dbbc7f", "#7fbbb3", "#d699b6", "#83c092", "#d3c6aa",
        ],
    ),
];

fn preset(row: &Row) -> Palette {
    let (name, dark, roles, term) = row;
    let p = |s: &str| Rgb::parse(s).unwrap_or_default(); // every entry is checked by a test
    let [
        bg_dim,
        bg,
        surface,
        surface_high,
        fg,
        muted,
        outline,
        primary,
        on_primary,
        secondary,
        tertiary,
        error,
    ] = roles.map(p);
    Palette {
        name: (*name).to_owned(),
        dark: *dark,
        bg_dim,
        bg,
        surface,
        surface_high,
        fg,
        muted,
        outline,
        primary,
        on_primary,
        secondary,
        tertiary,
        error,
        term: term.map(p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_parse() {
        for (name, _, roles, term) in PRESETS {
            for hex in roles.iter().chain(term.iter()) {
                assert!(Rgb::parse(hex).is_some(), "{name}: bad color {hex}");
            }
        }
        assert_eq!(presets().len(), 10);
        assert_eq!(preset_named("Nord").unwrap().primary.hex(), "#88c0d0");
    }

    #[test]
    fn generates_material_schemes() {
        let seed = Rgb::parse("#6750a4").unwrap();
        let dark = from_seed(seed, Variant::TonalSpot, true);
        let light = from_seed(seed, Variant::TonalSpot, false);
        let luma = |c: Rgb| c.0.iter().map(|v| u32::from(*v)).sum::<u32>();
        assert!(
            luma(dark.bg) < luma(dark.fg),
            "dark scheme: light text on dark background"
        );
        assert!(luma(light.bg) > luma(light.fg));
        assert_ne!(
            from_seed(seed, Variant::Vibrant, true).primary,
            from_seed(seed, Variant::Monochrome, true).primary
        );
        let red = dark.term[1].0;
        assert!(
            red[0] > red[1] && red[0] > red[2],
            "terminal red stays red: {red:?}"
        );
    }

    #[test]
    fn variant_ids_round_trip() {
        for v in Variant::ALL {
            assert_eq!(Variant::from_id(v.id()), Some(v));
        }
    }
}

//! Hyprland settings Rune overrides, written to `~/.config/hypr/modules/rune.lua`.
//!
//! The user's own Lua stays untouched: `hyprland.lua` loads `rune.lua` last, so it
//! holds only the values changed in Rune. Keys come from the fixed [`OPTIONS`]
//! schema and every value is clamped or matched against a list before it is
//! turned into Lua, so stored config can never inject code.

use crate::hyprland::{Error, Hyprland};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
};

/// A setting's value, in the shape Hyprland's Lua config takes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    /// Top, right, bottom, left.
    Sides([i64; 4]),
}

/// How a setting is edited and which values it accepts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Bool,
    Int {
        min: i64,
        max: i64,
        step: i64,
    },
    Float {
        min: f64,
        max: f64,
        step: f64,
    },
    Choice(&'static [&'static str]),
    Sides {
        max: i64,
    },
    /// Shown read-only; Rune never writes it.
    Info,
}

/// Settings pages the options are shown on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Windows,
    Effects,
    Input,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OptionSpec {
    /// Lua path, e.g. `decoration.blur.size`; the IPC name uses `:` instead of `.`.
    pub key: &'static str,
    pub label: &'static str,
    pub section: Section,
    pub group: &'static str,
    pub kind: Kind,
    /// Shown after the value, e.g. `px`.
    pub unit: &'static str,
}

impl OptionSpec {
    const fn unit(mut self, unit: &'static str) -> Self {
        self.unit = unit;
        self
    }
}

const fn opt(
    key: &'static str,
    label: &'static str,
    section: Section,
    group: &'static str,
    kind: Kind,
) -> OptionSpec {
    OptionSpec {
        key,
        label,
        section,
        group,
        kind,
        unit: "",
    }
}

const fn int(min: i64, max: i64) -> Kind {
    Kind::Int { min, max, step: 1 }
}

const fn float(min: f64, max: f64, step: f64) -> Kind {
    Kind::Float { min, max, step }
}

use Section::{Effects, Input, Windows};

/// Keyboard layouts offered in the layout picker.
pub const KB_LAYOUTS: &[&str] = &[
    "us", "gb", "de", "fr", "es", "it", "pt", "br", "se", "no", "dk", "fi", "pl", "cz", "ru", "ua",
    "tr", "jp", "kr",
];

/// Every setting the Hyprland pages show, in display order.
pub const OPTIONS: &[OptionSpec] = &[
    opt(
        "general.layout",
        "Tiling layout",
        Windows,
        "Layout",
        Kind::Choice(&["dwindle", "master", "scrolling", "monocle"]),
    ),
    opt(
        "scrolling.column_width",
        "Column width (Scrolling layout · fraction of the screen)",
        Windows,
        "Layout",
        float(0.1, 1.0, 0.05),
    ),
    opt(
        "scrolling.follow_focus",
        "Scroll to the focused window",
        Windows,
        "Layout",
        Kind::Bool,
    ),
    opt(
        "general.gaps_in",
        "Between windows",
        Windows,
        "Gaps",
        int(0, 40),
    )
    .unit("px"),
    opt(
        "general.gaps_out",
        "Screen edges (Top, right, bottom and left, in px)",
        Windows,
        "Gaps",
        Kind::Sides { max: 100 },
    ),
    opt(
        "general.border_size",
        "Border width",
        Windows,
        "Borders",
        int(0, 10),
    )
    .unit("px"),
    opt(
        "general.resize_on_border",
        "Drag borders to resize",
        Windows,
        "Borders",
        Kind::Bool,
    ),
    opt(
        "decoration.rounding",
        "Corner radius",
        Effects,
        "Corners & opacity",
        int(0, 30),
    )
    .unit("px"),
    opt(
        "decoration.active_opacity",
        "Focused window opacity",
        Effects,
        "Corners & opacity",
        float(0.3, 1.0, 0.01),
    ),
    opt(
        "decoration.inactive_opacity",
        "Other windows opacity",
        Effects,
        "Corners & opacity",
        float(0.3, 1.0, 0.01),
    ),
    opt(
        "decoration.blur.enabled",
        "Blur behind windows",
        Effects,
        "Blur",
        Kind::Bool,
    ),
    opt(
        "decoration.blur.size",
        "Blur size",
        Effects,
        "Blur",
        int(1, 20),
    ),
    opt(
        "decoration.blur.passes",
        "Passes",
        Effects,
        "Blur",
        int(1, 6),
    ),
    opt(
        "decoration.blur.vibrancy",
        "Vibrancy",
        Effects,
        "Blur",
        float(0.0, 1.0, 0.05),
    ),
    // Only some Hyprland builds have blur variants; shown when present, never written.
    opt(
        "decoration.blur.variant",
        "Variant",
        Effects,
        "Blur",
        Kind::Info,
    ),
    opt(
        "decoration.shadow.enabled",
        "Drop shadow",
        Effects,
        "Shadow",
        Kind::Bool,
    ),
    opt(
        "decoration.shadow.range",
        "Shadow size",
        Effects,
        "Shadow",
        int(0, 60),
    )
    .unit("px"),
    opt(
        "input.repeat_delay",
        "Delay before repeat",
        Input,
        "Keyboard",
        Kind::Int {
            min: 100,
            max: 1000,
            step: 10,
        },
    )
    .unit("ms"),
    opt(
        "input.repeat_rate",
        "Repeat rate",
        Input,
        "Keyboard",
        int(10, 80),
    )
    .unit("/s"),
    opt(
        "input.numlock_by_default",
        "Num Lock on at login",
        Input,
        "Keyboard",
        Kind::Bool,
    ),
    opt(
        "input.kb_layout",
        "Layout",
        Input,
        "Keyboard",
        Kind::Choice(KB_LAYOUTS),
    ),
];

/// Animations shown on the Window Motion page.
pub const LEAVES: &[&str] = &[
    "windowsIn",
    "windowsOut",
    "windowsMove",
    "layersIn",
    "layersOut",
    "fade",
    "border",
    "workspaces",
    "specialWorkspace",
];

pub fn spec(key: &str) -> Option<&'static OptionSpec> {
    OPTIONS.iter().find(|s| s.key == key)
}

impl OptionSpec {
    /// `value` in this option's type, without range checks.
    fn coerce(&self, value: Value) -> Option<Value> {
        match (self.kind, value) {
            (Kind::Bool, Value::Bool(b)) => Some(Value::Bool(b)),
            (Kind::Bool, Value::Int(i)) => Some(Value::Bool(i != 0)),
            (Kind::Int { .. }, Value::Int(i)) => Some(Value::Int(i)),
            // Gaps come back as CSS sides even when set to a single number.
            (Kind::Int { .. }, Value::Sides([top, ..])) => Some(Value::Int(top)),
            (Kind::Int { .. }, Value::Float(f)) => float_to_int(f).map(Value::Int),
            (Kind::Float { .. }, Value::Float(f)) if f.is_finite() => Some(Value::Float(f)),
            (Kind::Float { .. }, Value::Int(i)) => Some(Value::Float(i64_to_f64(i))),
            (Kind::Choice(_), Value::Text(t)) => Some(Value::Text(t)),
            (Kind::Sides { .. }, Value::Sides(s)) => Some(Value::Sides(s)),
            (Kind::Sides { .. }, Value::Int(i)) => Some(Value::Sides([i; 4])),
            (Kind::Info, Value::Text(t)) => Some(Value::Text(t)),
            (Kind::Info, Value::Int(i)) => Some(Value::Text(i.to_string())),
            _ => None,
        }
    }

    /// The value this option accepts closest to `value`, or `None` when it can't take it.
    pub fn clamp(&self, value: Value) -> Option<Value> {
        if self.kind == Kind::Info {
            return None;
        }
        match (self.kind, self.coerce(value)?) {
            (Kind::Int { min, max, step }, Value::Int(n)) => {
                let step = step.max(1);
                let n = n.clamp(min, max);
                Some(Value::Int(
                    min.checked_add((n.checked_sub(min)? + step / 2) / step * step)?
                        .min(max),
                ))
            }
            (Kind::Float { min, max, step }, Value::Float(f)) => {
                let snapped = ((f - min) / step).round() * step + min;
                // Drop float noise so 0.35 stays 0.35 in the file.
                Some(Value::Float(
                    (snapped.clamp(min, max) * 1000.).round() / 1000.,
                ))
            }
            (Kind::Choice(list), Value::Text(t)) => {
                list.contains(&t.as_str()).then_some(Value::Text(t))
            }
            (Kind::Sides { max }, Value::Sides(sides)) => {
                Some(Value::Sides(sides.map(|s| s.clamp(0, max))))
            }
            (_, v) => Some(v),
        }
    }

    /// Whether a value read from Hyprland can be written back as Lua unchanged.
    fn emittable(&self, value: &Value) -> bool {
        match (self.kind, value) {
            (Kind::Info, _) => false,
            (Kind::Choice(list), Value::Text(t)) => list.contains(&t.as_str()),
            (_, Value::Float(f)) => f.is_finite(),
            _ => true,
        }
    }

    /// Reads this option out of a `j/getoption` reply.
    fn parse_live(&self, reply: &serde_json::Value) -> Option<Value> {
        let raw = if let Some(b) = reply.get("bool").and_then(serde_json::Value::as_bool) {
            Value::Bool(b)
        } else if let Some(i) = reply.get("int").and_then(serde_json::Value::as_i64) {
            Value::Int(i)
        } else if let Some(f) = reply.get("float").and_then(serde_json::Value::as_f64) {
            Value::Float(f)
        } else if let Some(s) = reply.get("str").and_then(serde_json::Value::as_str) {
            Value::Text(s.to_owned())
        } else {
            parse_css_sides(reply.get("css").and_then(serde_json::Value::as_str)?)?
        };
        // Live values keep the user's own range; only Rune's edits are clamped.
        self.coerce(raw)
    }
}

fn float_to_int(f: f64) -> Option<i64> {
    let r = f.round();
    // Float-to-int `as` saturates rather than wraps; NaN and huge values are rejected first.
    (-9.0e15..=9.0e15).contains(&r).then_some(r as i64)
}

fn i64_to_f64(i: i64) -> f64 {
    // Option values are tiny; anything near 2^53 is rejected by the range clamp anyway.
    i32::try_from(i.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).map_or(0., f64::from)
}

/// `"0 16 16 16"` (top right bottom left) or one to three CSS-style numbers.
fn parse_css_sides(css: &str) -> Option<Value> {
    let n: Vec<i64> = css
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let sides = match n.as_slice() {
        [a] => [*a; 4],
        [v, h] => [*v, *h, *v, *h],
        [t, h, b] => [*t, *h, *b, *h],
        [t, r, b, l] => [*t, *r, *b, *l],
        _ => return None,
    };
    Some(Value::Sides(sides))
}

/// A named animation curve. Untagged encoding preserves existing spring settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Curve {
    Spring(Spring),
    Bezier(Bezier),
}

impl Curve {
    fn sanitized(self) -> Option<Self> {
        match self {
            Self::Spring(s) => s.sanitized().map(Self::Spring),
            Self::Bezier(b) => b.sanitized().map(Self::Bezier),
        }
    }

    pub fn spring(self) -> Option<Spring> {
        match self {
            Self::Spring(s) => Some(s),
            Self::Bezier(_) => None,
        }
    }
}

/// Cubic bezier control points, with endpoints fixed at (0, 0) and (1, 1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bezier {
    pub points: [f64; 4],
}

impl Bezier {
    pub fn sanitized(self) -> Option<Self> {
        if !self.points.iter().all(|v| v.is_finite()) {
            return None;
        }
        let [x1, y1, x2, y2] = self.points;
        let round = |v: f64| (v * 1000.).round() / 1000.;
        Some(Self {
            points: [
                round(x1.clamp(0., 1.)),
                round(y1.clamp(-2., 3.)),
                round(x2.clamp(0., 1.)),
                round(y2.clamp(-2., 3.)),
            ],
        })
    }

    pub fn position(self, t: f64) -> (f64, f64) {
        let [x1, y1, x2, y2] = self.points;
        let component =
            |a, b| 3. * (1. - t) * (1. - t) * t * a + 3. * (1. - t) * t * t * b + t * t * t;
        (component(x1, x2), component(y1, y2))
    }

    pub fn ease(self, progress: f64) -> f64 {
        let mut low = 0.;
        let mut high = 1.;
        for _ in 0..24 {
            let mid = (low + high) / 2.;
            if self.position(mid).0 < progress {
                low = mid;
            } else {
                high = mid;
            }
        }
        self.position((low + high) / 2.).1
    }
}

/// CSS easing presets shipped by hyprmod.
pub const BEZIER_PRESETS: &[(&str, Bezier)] = &[
    (
        "ease",
        Bezier {
            points: [0.25, 0.1, 0.25, 1.0],
        },
    ),
    (
        "easeIn",
        Bezier {
            points: [0.42, 0.0, 1.0, 1.0],
        },
    ),
    (
        "easeOut",
        Bezier {
            points: [0.0, 0.0, 0.58, 1.0],
        },
    ),
    (
        "easeInOut",
        Bezier {
            points: [0.42, 0.0, 0.58, 1.0],
        },
    ),
    (
        "easeInSine",
        Bezier {
            points: [0.12, 0.0, 0.39, 0.0],
        },
    ),
    (
        "easeOutSine",
        Bezier {
            points: [0.61, 1.0, 0.88, 1.0],
        },
    ),
    (
        "easeInOutSine",
        Bezier {
            points: [0.37, 0.0, 0.63, 1.0],
        },
    ),
    (
        "easeInQuad",
        Bezier {
            points: [0.11, 0.0, 0.5, 0.0],
        },
    ),
    (
        "easeOutQuad",
        Bezier {
            points: [0.5, 1.0, 0.89, 1.0],
        },
    ),
    (
        "easeInOutQuad",
        Bezier {
            points: [0.45, 0.0, 0.55, 1.0],
        },
    ),
    (
        "easeInCubic",
        Bezier {
            points: [0.32, 0.0, 0.67, 0.0],
        },
    ),
    (
        "easeOutCubic",
        Bezier {
            points: [0.33, 1.0, 0.68, 1.0],
        },
    ),
    (
        "easeInOutCubic",
        Bezier {
            points: [0.65, 0.0, 0.35, 1.0],
        },
    ),
    (
        "easeInExpo",
        Bezier {
            points: [0.7, 0.0, 0.84, 0.0],
        },
    ),
    (
        "easeOutExpo",
        Bezier {
            points: [0.16, 1.0, 0.3, 1.0],
        },
    ),
    (
        "easeInOutExpo",
        Bezier {
            points: [0.87, 0.0, 0.13, 1.0],
        },
    ),
    (
        "easeInBack",
        Bezier {
            points: [0.36, 0.0, 0.66, -0.56],
        },
    ),
    (
        "easeOutBack",
        Bezier {
            points: [0.34, 1.56, 0.64, 1.0],
        },
    ),
    (
        "easeInOutBack",
        Bezier {
            points: [0.68, -0.6, 0.32, 1.6],
        },
    ),
];

pub fn bezier_preset(name: &str) -> Option<Bezier> {
    BEZIER_PRESETS
        .iter()
        .find_map(|(n, b)| (*n == name).then_some(*b))
}

/// A `type = "spring"` curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Spring {
    pub mass: f64,
    pub stiffness: f64,
    pub dampening: f64,
}

impl Spring {
    fn sanitized(self) -> Option<Self> {
        let ok = |v: f64| v.is_finite();
        (ok(self.mass) && ok(self.stiffness) && ok(self.dampening)).then(|| Self {
            mass: self.mass.clamp(0.1, 10.),
            stiffness: self.stiffness.clamp(1., 1000.),
            dampening: self.dampening.clamp(1., 200.),
        })
    }

    /// Position (0 at rest, 1 at target) `t` seconds into a unit step.
    pub fn response(&self, t: f64) -> f64 {
        let w = (self.stiffness / self.mass).sqrt();
        let zeta = self.dampening / (2. * (self.stiffness * self.mass).sqrt());
        if zeta < 1. {
            let wd = w * (1. - zeta * zeta).sqrt();
            1. - (-zeta * w * t).exp() * ((wd * t).cos() + zeta * w / wd * (wd * t).sin())
        } else {
            1. - (-w * t).exp() * (1. + w * t)
        }
    }
}

/// One `hl.animation` leaf.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Leaf {
    pub enabled: bool,
    pub speed: f64,
    /// `spring:<name>` or a bezier name such as `default`.
    pub curve: String,
    #[serde(default)]
    pub style: String,
}

impl Leaf {
    fn sanitized(mut self) -> Option<Self> {
        if !self.speed.is_finite() {
            return None;
        }
        self.speed = ((self.speed.clamp(0.5, 20.)) * 2.).round() / 2.;
        let name = self.curve.strip_prefix("spring:").unwrap_or(&self.curve);
        if !valid_name(name) || !valid_style(&self.style) {
            return None;
        }
        Some(self)
    }
}

/// Curve names are emitted inside Lua strings, so they stay plain identifiers.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 32
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Styles come from Hyprland (`popin 96%`, `slidefadevert 10%`).
fn valid_style(style: &str) -> bool {
    style.len() <= 32
        && style
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '%' | '.'))
}

const MAX_CURVES: usize = 32;

/// Entries edited this session, so a preview re-applies only those.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Touched {
    pub options: std::collections::BTreeSet<String>,
    pub curves: std::collections::BTreeSet<String>,
    pub leaves: std::collections::BTreeSet<String>,
}

impl Touched {
    pub fn is_empty(&self) -> bool {
        self.options.is_empty() && self.curves.is_empty() && self.leaves.is_empty()
    }
}

/// The `[hyprland]` section of Rune's config: what `rune.lua` sets.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Overrides {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub curves: BTreeMap<String, Curve>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub animations: BTreeMap<String, Leaf>,
}

impl Overrides {
    /// Drops anything Rune doesn't know or can't emit safely; clamps the rest.
    pub fn sanitize(&mut self) {
        self.options = std::mem::take(&mut self.options)
            .into_iter()
            .filter_map(|(k, v)| Some((k.clone(), spec(&k)?.clamp(v)?)))
            .collect();
        self.curves = std::mem::take(&mut self.curves)
            .into_iter()
            .filter(|(name, _)| valid_name(name))
            .filter_map(|(name, s)| Some((name, s.sanitized()?)))
            .take(MAX_CURVES)
            .collect();
        self.animations = std::mem::take(&mut self.animations)
            .into_iter()
            .filter(|(leaf, _)| LEAVES.contains(&leaf.as_str()))
            .filter_map(|(leaf, l)| Some((leaf, l.sanitized()?)))
            .collect();
    }

    pub fn is_empty(&self) -> bool {
        self.options.is_empty() && self.curves.is_empty() && self.animations.is_empty()
    }

    /// What a live preview should apply for the `touched` entries (see [`Touched`]):
    /// the override where there is one, otherwise the value from before Rune changed it.
    /// The whole touched set is sent, not just the last edit, so previews can coalesce.
    pub fn preview(&self, live: &Snapshot, touched: &Touched) -> Self {
        let mut out = Self::default();
        for key in &touched.options {
            let value = self.options.get(key).cloned().or_else(|| {
                let v = live.options.get(key)?;
                spec(key)?.emittable(v).then(|| v.clone())
            });
            if let Some(v) = value {
                out.options.insert(key.clone(), v);
            }
        }
        for name in &touched.curves {
            if let Some(s) = self.curves.get(name).or_else(|| live.curves.get(name)) {
                out.curves.insert(name.clone(), *s);
            }
        }
        for leaf in &touched.leaves {
            if let Some(l) = self.animations.get(leaf).or_else(|| live.leaves.get(leaf)) {
                out.animations.insert(leaf.clone(), l.clone());
            }
        }
        for leaf in out.animations.values() {
            let name = leaf.curve.strip_prefix("spring:").unwrap_or(&leaf.curve);
            if let Some(curve) = self.curves.get(name).or_else(|| live.curves.get(name)) {
                out.curves.insert(name.to_owned(), *curve);
            }
        }
        out
    }

    /// The Lua that applies these settings (without the file header).
    pub fn lua(&self) -> String {
        let mut out = String::new();
        if !self.options.is_empty() {
            out.push_str("hl.config({\n");
            write_table(&mut out, &tree(&self.options), 1);
            out.push_str("})\n");
        }
        let mut curves = self.curves.clone();
        for leaf in self.animations.values() {
            if let Some(preset) = bezier_preset(&leaf.curve) {
                curves
                    .entry(leaf.curve.clone())
                    .or_insert(Curve::Bezier(preset));
            }
        }
        for (name, curve) in curves {
            match curve {
                Curve::Spring(s) => {
                    let _ = writeln!(
                        out,
                        "hl.curve(\"{name}\", {{ type = \"spring\", mass = {}, stiffness = {}, dampening = {} }})",
                        number(s.mass),
                        number(s.stiffness),
                        number(s.dampening)
                    );
                }
                Curve::Bezier(b) => {
                    let [x1, y1, x2, y2] = b.points;
                    let _ = writeln!(
                        out,
                        "hl.curve(\"{name}\", {{ type = \"bezier\", points = {{ {{{}, {}}}, {{{}, {}}} }} }})",
                        number(x1),
                        number(y1),
                        number(x2),
                        number(y2)
                    );
                }
            }
        }
        for (leaf, l) in &self.animations {
            let curve = match l.curve.strip_prefix("spring:") {
                Some(name) => format!("spring = \"{name}\""),
                None => format!("bezier = \"{}\"", l.curve),
            };
            let style = if l.style.is_empty() {
                String::new()
            } else {
                format!(", style = \"{}\"", l.style)
            };
            let _ = writeln!(
                out,
                "hl.animation({{ leaf = \"{leaf}\", enabled = {}, speed = {}, {curve}{style} }})",
                l.enabled,
                number(l.speed)
            );
        }
        out
    }

    /// Contents of `modules/rune.lua`.
    pub fn render(&self) -> String {
        let mut out = String::from(HEADER);
        if self.is_empty() {
            out.push_str("-- Nothing is overridden.\n");
        } else {
            out.push('\n');
            out.push_str(&self.lua());
        }
        out
    }
}

const HEADER: &str = "-- Generated by Rune Shell. Edit in Rune → Settings → Hyprland;\n\
                      -- changes made here are overwritten on the next save.\n\
                      -- Loaded last from hyprland.lua, so it overrides modules/*.lua.\n";

enum Node<'a> {
    Leaf(&'a Value),
    Table(BTreeMap<&'a str, Node<'a>>),
}

fn tree(options: &BTreeMap<String, Value>) -> BTreeMap<&str, Node<'_>> {
    let mut root = BTreeMap::new();
    for (key, value) in options {
        let mut table = &mut root;
        let mut parts = key.split('.').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                table.insert(part, Node::Leaf(value));
                break;
            }
            let entry = table
                .entry(part)
                .or_insert_with(|| Node::Table(BTreeMap::new()));
            let Node::Table(next) = entry else { break };
            table = next;
        }
    }
    root
}

fn write_table(out: &mut String, table: &BTreeMap<&str, Node<'_>>, depth: usize) {
    let pad = "  ".repeat(depth);
    for (name, node) in table {
        match node {
            Node::Leaf(v) => {
                let _ = writeln!(out, "{pad}{name} = {},", value(v));
            }
            Node::Table(t) => {
                let _ = writeln!(out, "{pad}{name} = {{");
                write_table(out, t, depth + 1);
                let _ = writeln!(out, "{pad}}},");
            }
        }
    }
}

fn value(v: &Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => number(*f),
        // Only schema choices get here (see `OptionSpec::clamp`), so plain quotes are safe.
        Value::Text(t) => format!("\"{t}\""),
        Value::Sides([t, r, b, l]) => {
            format!("{{ top = {t}, right = {r}, bottom = {b}, left = {l} }}")
        }
    }
}

fn number(f: f64) -> String {
    let s = format!("{f:.3}");
    let s = s.trim_end_matches('0');
    if s.ends_with('.') {
        format!("{s}0")
    } else {
        s.to_owned()
    }
}

/// Whether `hyprland.lua` loads `rune.lua`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Setup {
    #[default]
    Unknown,
    /// There's no `hyprland.lua` (Hyprlang config or no config).
    NoLuaConfig,
    NotAdded,
    Added,
}

/// The running compositor's values, plus what's needed to edit them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snapshot {
    pub options: BTreeMap<String, Value>,
    pub leaves: BTreeMap<String, Leaf>,
    /// Dotfile springs and compositor-reported bezier curves.
    pub curves: BTreeMap<String, Curve>,
    pub sources: Sources,
    pub entry: Entry,
    pub setup: Setup,
}

/// Which dotfile (relative to the Hypr dir, e.g. `modules/general.lua`) sets each
/// entry, found by a best-effort text scan. Missing means Hyprland's default.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sources {
    pub options: BTreeMap<String, String>,
    pub curves: BTreeMap<String, String>,
    pub leaves: BTreeMap<String, String>,
    /// String literals for read-only options, e.g. a blur variant Hyprland reports as a number.
    pub literals: BTreeMap<String, String>,
}

/// `hyprland.lua` as the setup card shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Entry {
    /// `~/.config/hypr/hyprland.lua`.
    pub path: String,
    /// The real file, when `path` is a symlink.
    pub target: Option<String>,
    /// Its last non-empty line, as context for the appended lines.
    pub last_line: String,
}

/// `$XDG_CONFIG_HOME/hypr`, falling back to `~/.config/hypr`.
pub fn hypr_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("hypr"))
}

fn config_dir() -> Result<PathBuf, Error> {
    hypr_dir().ok_or(Error::NotRunning)
}

pub const REQUIRE_COMMENT: &str =
    "-- Rune Shell managed settings (edit in Rune → Settings → Hyprland)";
pub const REQUIRE_LINE: &str = "require(\"modules.rune\")";

/// Whether some uncommented line of `text` requires `modules.rune`.
pub fn requires_rune(text: &str) -> bool {
    text.lines().any(|line| {
        let code = line.split("--").next().unwrap_or_default();
        let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        [
            "require(\"modules.rune\")",
            "require('modules.rune')",
            "require\"modules.rune\"",
        ]
        .iter()
        .any(|form| compact.contains(form))
    })
}

fn file_error(path: &Path, source: std::io::Error) -> Error {
    Error::File {
        path: path.to_owned(),
        source,
    }
}

/// The real file behind `path`, following symlinks (dotfile managers link these).
async fn resolve(path: &Path) -> Result<PathBuf, Error> {
    if let Ok(real) = tokio::fs::canonicalize(path).await {
        return Ok(real);
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    let dir = tokio::fs::canonicalize(parent)
        .await
        .map_err(|e| file_error(parent, e))?;
    Ok(path.file_name().map_or(dir.clone(), |name| dir.join(name)))
}

/// Replaces the file behind `path` in one step, keeping any symlink to it.
async fn write_atomic(path: &Path, contents: &str) -> Result<(), Error> {
    let target = resolve(path).await?;
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = target.with_file_name(format!(".{name}.rune-tmp"));
    tokio::fs::write(&tmp, contents)
        .await
        .map_err(|e| file_error(&tmp, e))?;
    tokio::fs::rename(&tmp, &target)
        .await
        .map_err(|e| file_error(&target, e))
}

const MAX_LUA_FILE: u64 = 512 * 1024;

async fn read_small(path: &Path) -> Result<String, Error> {
    let meta = tokio::fs::metadata(path)
        .await
        .map_err(|e| file_error(path, e))?;
    if meta.len() > MAX_LUA_FILE {
        return Err(Error::Rejected(format!(
            "{} is larger than {MAX_LUA_FILE} bytes",
            path.display()
        )));
    }
    tokio::fs::read_to_string(path)
        .await
        .map_err(|e| file_error(path, e))
}

pub async fn setup_state(dir: &Path) -> Setup {
    match read_small(&dir.join("hyprland.lua")).await {
        Ok(text) if requires_rune(&text) => Setup::Added,
        Ok(_) => Setup::NotAdded,
        Err(_) => Setup::NoLuaConfig,
    }
}

/// Creates `modules/rune.lua` if needed and appends the `require` line to `hyprland.lua`.
pub async fn setup(dir: &Path, current: &Overrides) -> Result<(), Error> {
    let entry = dir.join("hyprland.lua");
    let text = read_small(&entry).await?;
    if requires_rune(&text) {
        return Ok(());
    }
    let modules = dir.join("modules");
    tokio::fs::create_dir_all(&modules)
        .await
        .map_err(|e| file_error(&modules, e))?;
    let module = modules.join("rune.lua");
    if tokio::fs::metadata(&module).await.is_err() {
        write_atomic(&module, &current.render()).await?;
    }
    let real = resolve(&entry).await?;
    let backup = real.with_file_name("hyprland.lua.rune-bak");
    if tokio::fs::metadata(&backup).await.is_err() {
        tokio::fs::copy(&real, &backup)
            .await
            .map_err(|e| file_error(&backup, e))?;
    }
    let mut updated = text;
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    let _ = write!(updated, "\n{REQUIRE_COMMENT}\n{REQUIRE_LINE}\n");
    write_atomic(&entry, &updated).await
}

pub async fn save(dir: &Path, overrides: &Overrides) -> Result<(), Error> {
    write_atomic(&dir.join("modules").join("rune.lua"), &overrides.render()).await
}

const MAX_MODULE_FILES: usize = 64;

/// Curves, sources and setup preview read from the user's Lua.
#[derive(Debug, Default)]
struct Dotfiles {
    curves: BTreeMap<String, Curve>,
    sources: Sources,
    entry: Entry,
}

fn tilde(path: &Path) -> String {
    let shown = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && shown.starts_with(&home) => {
            format!("~{}", shown.get(home.len()..).unwrap_or_default())
        }
        _ => shown,
    }
}

/// `require("a.b")` targets in `text`, in order, as paths relative to the Hypr dir.
fn required_files(text: &str) -> Vec<String> {
    let mut files = Vec::new();
    for line in text.lines() {
        let code = line.split("--").next().unwrap_or_default();
        let Some(at) = code.find("require") else {
            continue;
        };
        let rest = code.get(at + "require".len()..).unwrap_or_default();
        let rest = rest.trim_start().trim_start_matches('(').trim_start();
        let Some(quote) = rest.chars().next().filter(|c| matches!(c, '"' | '\'')) else {
            continue;
        };
        let Some(name) = rest.get(1..).and_then(|r| r.split(quote).next()) else {
            continue;
        };
        let valid = !name.is_empty()
            && name.split('.').all(|part| {
                !part.is_empty()
                    && part
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            });
        if valid && name != "modules.rune" {
            files.push(format!("{}.lua", name.replace('.', "/")));
        }
        if files.len() >= MAX_MODULE_FILES {
            break;
        }
    }
    files
}

/// `text` with Lua line comments removed, so commented-out settings don't count.
fn without_comments(text: &str) -> String {
    text.lines()
        .map(|line| line.split("--").next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The quoted string `name` is set to, if it's a short plain word.
fn literal(code: &str, name: &str) -> Option<String> {
    let mut search = code;
    while let Some(at) = search.find(name) {
        let after = search.get(at + name.len()..)?;
        search = after;
        let Some(value) = after.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        let quote = value.chars().next().filter(|c| matches!(c, '"' | '\''))?;
        let word = value.get(1..)?.split(quote).next()?;
        return (word.len() <= 32 && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
            .then(|| word.to_owned());
    }
    None
}

/// Whether `code` assigns `name` a literal (`name = 5`, `"x"`, `true`, `{`), not
/// `name == …` or a variable, which usually means a runtime change in a function.
fn assigns(code: &str, name: &str) -> bool {
    let mut search = code;
    while let Some(at) = search.find(name) {
        let before = search.get(..at).and_then(|s| s.chars().last());
        let Some(after) = search.get(at + name.len()..) else {
            return false;
        };
        search = after;
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let after = after.trim_start();
        if let Some(value) = after.strip_prefix('=').filter(|v| !v.starts_with('=')) {
            let value = value.trim_start();
            let literal =
                value.chars().next().is_some_and(|c| {
                    c.is_ascii_digit() || matches!(c, '-' | '.' | '"' | '\'' | '{')
                }) || value.starts_with("true")
                    || value.starts_with("false");
            if literal {
                return true;
            }
        }
    }
    false
}

fn scan(file: &str, text: &str, found: &mut Dotfiles) {
    let code = without_comments(text);
    for spec in OPTIONS {
        let mut parts = spec.key.split('.');
        let (Some(section), Some(leaf)) = (parts.next(), spec.key.rsplit('.').next()) else {
            continue;
        };
        if code.contains(section) && assigns(&code, leaf) {
            found
                .sources
                .options
                .insert(spec.key.to_owned(), file.to_owned());
            if spec.kind == Kind::Info
                && let Some(text) = literal(&code, leaf)
            {
                found.sources.literals.insert(spec.key.to_owned(), text);
            }
        }
    }
    for (name, spring) in parse_curves(&code)
        .into_iter()
        .map(|(n, s)| (n, Curve::Spring(s)))
        .chain(parse_beziers(&code))
    {
        if found.curves.len() < MAX_CURVES || found.curves.contains_key(&name) {
            found.sources.curves.insert(name.clone(), file.to_owned());
            found.curves.insert(name, spring);
        }
    }
    let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    for leaf in LEAVES {
        if compact.contains(&format!("leaf=\"{leaf}\""))
            || compact.contains(&format!("leaf='{leaf}'"))
        {
            found
                .sources
                .leaves
                .insert((*leaf).to_owned(), file.to_owned());
        }
    }
}

/// Reads `hyprland.lua` and the files it requires, in load order (later wins).
/// Without `hyprland.lua`, `modules/*.lua` are read alphabetically.
async fn read_dotfiles(dir: &Path) -> Dotfiles {
    let mut found = Dotfiles::default();
    let entry = dir.join("hyprland.lua");
    found.entry.path = tilde(&entry);
    let mut files = Vec::new();
    if let Ok(text) = read_small(&entry).await {
        if let Ok(real) = tokio::fs::canonicalize(&entry).await
            && real != entry
        {
            found.entry.target = Some(tilde(&real));
        }
        found.entry.last_line = text
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or_default()
            .chars()
            .take(80)
            .collect();
        scan("hyprland.lua", &text, &mut found);
        files = required_files(&text);
    } else if let Ok(mut entries) = tokio::fs::read_dir(dir.join("modules")).await {
        while let Ok(Some(e)) = entries.next_entry().await {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".lua") && name != "rune.lua" {
                files.push(format!("modules/{name}"));
            }
            if files.len() >= MAX_MODULE_FILES {
                break;
            }
        }
        files.sort();
    }
    for file in files {
        if let Ok(text) = read_small(&dir.join(&file)).await {
            scan(&file, &text, &mut found);
        }
    }
    found
}

/// Finds `hl.curve("name", { type = "spring", mass = 1, stiffness = 150, dampening = 25 })`.
/// Only literal numbers are read; anything computed is skipped.
fn parse_beziers(text: &str) -> Vec<(String, Curve)> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("hl.curve(") {
        rest = rest.get(at + "hl.curve(".len()..).unwrap_or_default();
        let Some((name, bezier)) = parse_bezier(rest) else {
            continue;
        };
        if found.len() < MAX_CURVES {
            found.push((name, Curve::Bezier(bezier)));
        }
    }
    found
}

fn lua_table(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0_usize;
    for (offset, c) in text.get(start..)?.char_indices() {
        match c {
            '{' => depth = depth.checked_add(1)?,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return text.get(start..=start.checked_add(offset)?);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_bezier(after: &str) -> Option<(String, Bezier)> {
    let after = after.trim_start();
    let quote = after.chars().next().filter(|c| matches!(c, '\"' | '\''))?;
    let after = after.get(1..)?;
    let end = after.find(quote)?;
    let name = after.get(..end)?;
    if !valid_name(name) {
        return None;
    }
    let body = lua_table(after.get(end..)?)?;
    if !body.contains("\"bezier\"") && !body.contains("'bezier'") {
        return None;
    }
    let points = lua_table(body.get(body.find("points")?..)?.strip_prefix("points")?)?;
    let mut values = points
        .split([',', '{', '}', ' ', '\n', '\t', '\r'])
        .filter(|s| !s.is_empty());
    let mut coordinates = [0.; 4];
    for coordinate in &mut coordinates {
        *coordinate = values.next()?.parse().ok()?;
    }
    if values.next().is_some() {
        return None;
    }
    Some((
        name.to_owned(),
        Bezier {
            points: coordinates,
        }
        .sanitized()?,
    ))
}

fn parse_curves(text: &str) -> Vec<(String, Spring)> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("hl.curve(") {
        rest = rest.get(at + "hl.curve(".len()..).unwrap_or_default();
        let Some(spring) = parse_curve(rest) else {
            continue;
        };
        found.push(spring);
    }
    found
}

fn parse_curve(after: &str) -> Option<(String, Spring)> {
    let after = after.trim_start();
    let quote = after.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let after = after.get(1..)?;
    let end = after.find(quote)?;
    let name = after.get(..end)?;
    if !valid_name(name) {
        return None;
    }
    let after = after.get(end..)?;
    let open = after.find('{')?;
    let close = after.find('}')?;
    let body = after.get(open..close)?;
    if !body.contains("\"spring\"") && !body.contains("'spring'") {
        return None;
    }
    let spring = Spring {
        mass: number_field(body, "mass").unwrap_or(1.),
        stiffness: number_field(body, "stiffness")?,
        dampening: number_field(body, "dampening")?,
    }
    .sanitized()?;
    Some((name.to_owned(), spring))
}

fn number_field(body: &str, key: &str) -> Option<f64> {
    let mut search = body;
    while let Some(at) = search.find(key) {
        let before = search.get(..at).and_then(|s| s.chars().last());
        let after = search.get(at + key.len()..)?;
        search = after;
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let value = after.trim_start().strip_prefix('=')?.trim_start();
        let number: String = value
            .chars()
            .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | '-'))
            .take(16)
            .collect();
        return number.parse().ok().filter(|f: &f64| f.is_finite());
    }
    None
}

#[derive(Deserialize)]
struct RawBezier {
    name: String,
    #[serde(rename = "X0")]
    x0: f64,
    #[serde(rename = "Y0")]
    y0: f64,
    #[serde(rename = "X1")]
    x1: f64,
    #[serde(rename = "Y1")]
    y1: f64,
}

#[derive(Deserialize)]
struct RawLeaf {
    name: String,
    overridden: bool,
    enabled: bool,
    speed: f64,
    bezier: String,
    style: String,
}

impl Hyprland {
    /// Live values of every [`OPTIONS`] key and [`LEAVES`] animation, plus setup state.
    pub async fn settings_snapshot(&self) -> Result<Snapshot, Error> {
        let mut snapshot = Snapshot::default();
        for spec in OPTIONS {
            let reply = self
                .request(&format!("j/getoption {}", spec.key.replace('.', ":")))
                .await?;
            // Options a build doesn't have answer with plain text; they're left out.
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&reply)
                && let Some(v) = spec.parse_live(&json)
            {
                snapshot.options.insert(spec.key.to_owned(), v);
            }
        }
        let (leaves, beziers): (Vec<RawLeaf>, Vec<RawBezier>) =
            serde_json::from_str(&self.request("j/animations").await?)?;
        snapshot.curves = beziers
            .into_iter()
            .take(MAX_CURVES)
            .filter_map(|b| {
                if !valid_name(&b.name) || matches!(b.name.as_str(), "default" | "linear") {
                    return None;
                }
                Some((
                    b.name,
                    Curve::Bezier(
                        Bezier {
                            points: [b.x0, b.y0, b.x1, b.y1],
                        }
                        .sanitized()?,
                    ),
                ))
            })
            .collect();
        snapshot.leaves = leaves
            .into_iter()
            .filter(|l| l.overridden && LEAVES.contains(&l.name.as_str()))
            .filter_map(|l| {
                let leaf = Leaf {
                    enabled: l.enabled,
                    speed: l.speed,
                    curve: l.bezier,
                    style: l.style,
                }
                .sanitized()?;
                Some((l.name, leaf))
            })
            .collect();
        if let Some(dir) = hypr_dir() {
            let found = read_dotfiles(&dir).await;
            snapshot.curves.extend(found.curves);
            for (key, text) in &found.sources.literals {
                if snapshot.options.contains_key(key) {
                    snapshot
                        .options
                        .insert(key.clone(), Value::Text(text.clone()));
                }
            }
            snapshot.sources = found.sources;
            snapshot.entry = found.entry;
            snapshot.setup = setup_state(&dir).await;
        }
        Ok(snapshot)
    }

    pub(crate) async fn settings(&self, command: Settings) -> Result<Snapshot, Error> {
        match command {
            Settings::Snapshot => {}
            Settings::Setup(current) => setup(&config_dir()?, &current).await?,
            Settings::Save(overrides) => {
                save(&config_dir()?, &overrides).await?;
                self.request("reload").await?;
            }
            Settings::Discard => {
                self.request("reload").await?;
            }
        }
        self.settings_snapshot().await
    }
}

/// Hyprland settings requests from the settings window.
#[derive(Debug, Clone, PartialEq)]
pub enum Settings {
    Snapshot,
    /// Add the `require` line; the overrides seed a missing `rune.lua`.
    Setup(Box<Overrides>),
    /// Write `rune.lua` and reload.
    Save(Box<Overrides>),
    /// Reload, dropping unsaved previews.
    Discard,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_bezier_curves_from_lua_for_dotfile_ownership() {
        assert_eq!(
            parse_beziers(
                "hl.curve(\"gentle\", { type = \"bezier\", points = {{0.25, -0.5}, {0.75, 1.5}} })"
            ),
            vec![(
                "gentle".into(),
                Curve::Bezier(Bezier {
                    points: [0.25, -0.5, 0.75, 1.5]
                })
            )]
        );
    }

    #[test]
    fn bezier_curves_emit_and_preview_dependencies() {
        let mut overrides = Overrides::default();
        overrides.curves.insert(
            "custom1".into(),
            Curve::Bezier(Bezier {
                points: [0.3, -0.4, 0.7, 1.4],
            }),
        );
        overrides.animations.insert(
            "windowsMove".into(),
            Leaf {
                enabled: true,
                speed: 5.,
                curve: "custom1".into(),
                style: String::new(),
            },
        );
        let mut touched = Touched::default();
        touched.leaves.insert("windowsMove".into());
        let lua = overrides.preview(&Snapshot::default(), &touched).lua();
        assert!(lua.contains("points = { {0.3, -0.4}, {0.7, 1.4} }"));
        assert!(lua.contains("bezier = \"custom1\""));
        overrides.animations.get_mut("windowsMove").unwrap().curve = "easeOutBack".into();
        assert!(overrides.lua().contains("hl.curve(\"easeOutBack\""));
    }

    #[test]
    fn bezier_validation_bounds_hostile_points() {
        let mut overrides = Overrides::default();
        overrides.curves.insert(
            "bad".into(),
            Curve::Bezier(Bezier {
                points: [f64::NAN, 0., 1., 1.],
            }),
        );
        overrides.curves.insert(
            "bounded".into(),
            Curve::Bezier(Bezier {
                points: [-9., -99., 99., 99.],
            }),
        );
        overrides.sanitize();
        assert!(!overrides.curves.contains_key("bad"));
        assert_eq!(
            overrides.curves["bounded"],
            Curve::Bezier(Bezier {
                points: [0., -2., 1., 3.]
            })
        );
    }

    fn sample() -> Overrides {
        let mut o = Overrides::default();
        o.options
            .insert("general.gaps_out".into(), Value::Sides([0, 16, 16, 16]));
        o.options.insert("general.gaps_in".into(), Value::Int(8));
        o.options
            .insert("general.layout".into(), Value::Text("scrolling".into()));
        o.options
            .insert("decoration.blur.vibrancy".into(), Value::Float(0.35));
        o.options
            .insert("decoration.blur.enabled".into(), Value::Bool(true));
        o.curves.insert(
            "stripGlide".into(),
            Curve::Spring(Spring {
                mass: 1.,
                stiffness: 150.,
                dampening: 25.,
            }),
        );
        o.animations.insert(
            "windowsIn".into(),
            Leaf {
                enabled: true,
                speed: 4.,
                curve: "spring:stripGlide".into(),
                style: "popin 96%".into(),
            },
        );
        o
    }

    #[test]
    fn schema_is_consistent() {
        for (i, s) in OPTIONS.iter().enumerate() {
            assert!(
                OPTIONS.iter().skip(i + 1).all(|o| o.key != s.key),
                "{}",
                s.key
            );
            match s.kind {
                Kind::Int { min, max, step } => assert!(min < max && step > 0),
                Kind::Float { min, max, step } => assert!(min < max && step > 0.),
                _ => {}
            }
        }
    }

    #[test]
    fn renders_nested_lua() {
        assert_eq!(
            sample().render(),
            format!(
                "{HEADER}
hl.config({{
  decoration = {{
    blur = {{
      enabled = true,
      vibrancy = 0.35,
    }},
  }},
  general = {{
    gaps_in = 8,
    gaps_out = {{ top = 0, right = 16, bottom = 16, left = 16 }},
    layout = \"scrolling\",
  }},
}})
hl.curve(\"stripGlide\", {{ type = \"spring\", mass = 1.0, stiffness = 150.0, dampening = 25.0 }})
hl.animation({{ leaf = \"windowsIn\", enabled = true, speed = 4.0, spring = \"stripGlide\", style = \"popin 96%\" }})
"
            )
        );
        assert!(
            Overrides::default()
                .render()
                .ends_with("-- Nothing is overridden.\n")
        );
    }

    #[test]
    fn hostile_config_is_dropped_or_clamped() {
        let mut o: Overrides = toml::from_str(
            r#"
            [options]
            "general.gaps_in" = 1000000000
            "decoration.active_opacity" = nan
            "general.layout" = "dwindle\" }) os.execute(\"rm -rf ~\") --"
            "os.execute" = 1
            "general.border_size" = "2"
            "decoration.rounding" = -5
            "general.gaps_out" = [-1, 9999, 5, 5]
            [curves.'x") os.execute("y']
            mass = 1.0
            stiffness = 100.0
            dampening = 10.0
            [curves.ok]
            mass = 1.0
            stiffness = inf
            dampening = 10.0
            [animations.windowsIn]
            enabled = true
            speed = 1e9
            curve = "spring:a\"b"
            [animations.fade]
            enabled = true
            speed = 1e9
            curve = "default"
            style = "popin\" })"
            [animations.notALeaf]
            enabled = true
            speed = 1
            curve = "default"
            "#,
        )
        .unwrap();
        o.sanitize();
        assert_eq!(o.options["general.gaps_in"], Value::Int(40));
        assert_eq!(o.options["decoration.rounding"], Value::Int(0));
        assert_eq!(o.options["general.gaps_out"], Value::Sides([0, 100, 5, 5]));
        for gone in [
            "decoration.active_opacity",
            "general.layout",
            "os.execute",
            "general.border_size",
        ] {
            assert!(!o.options.contains_key(gone), "{gone}");
        }
        assert!(o.curves.is_empty());
        assert!(o.animations.is_empty());
        let lua = o.render();
        assert!(!lua.contains("os.execute"));
    }

    #[test]
    fn clamps_snap_to_steps() {
        let delay = spec("input.repeat_delay").unwrap();
        assert_eq!(delay.clamp(Value::Int(254)), Some(Value::Int(250)));
        assert_eq!(delay.clamp(Value::Int(257)), Some(Value::Int(260)));
        assert_eq!(delay.clamp(Value::Float(f64::NAN)), None);
        let opacity = spec("decoration.active_opacity").unwrap();
        assert_eq!(opacity.clamp(Value::Float(0.954)), Some(Value::Float(0.95)));
        assert_eq!(opacity.clamp(Value::Int(1)), Some(Value::Float(1.)));
    }

    #[test]
    fn parses_getoption_replies() {
        let parse = |key: &str, json: &str| {
            spec(key)
                .unwrap()
                .parse_live(&serde_json::from_str(json).unwrap())
        };
        assert_eq!(
            parse(
                "general.gaps_out",
                r#"{"option":"general:gaps_out","css":"0 16 16 16","set":true}"#
            ),
            Some(Value::Sides([0, 16, 16, 16]))
        );
        assert_eq!(
            parse("general.gaps_in", r#"{"css":"5 5 5 5"}"#),
            Some(Value::Int(5))
        );
        assert_eq!(
            parse("scrolling.follow_focus", r#"{"int":1}"#),
            Some(Value::Bool(true))
        );
        assert_eq!(
            parse("scrolling.column_width", r#"{"float":0.500000}"#),
            Some(Value::Float(0.5))
        );
        assert_eq!(
            parse("general.layout", r#"{"str":"hy3"}"#),
            Some(Value::Text("hy3".into()))
        );
    }

    #[test]
    fn detects_the_require_line() {
        assert!(requires_rune("x()\nrequire(\"modules.rune\")\n"));
        assert!(requires_rune("  require ( 'modules.rune' ) -- managed"));
        assert!(!requires_rune("-- require(\"modules.rune\")"));
        assert!(!requires_rune("require(\"modules.runes\")"));
    }

    #[test]
    fn reads_spring_curves_from_lua() {
        let text = r#"
hl.curve("stripGlide", {
  type = "spring",
  mass = 1.0,
  stiffness = 150.0,
  dampening = 25.0,
})
hl.curve('easeOut', { type = "bezier", points = { {0, 0}, {1, 1} } })
hl.curve("bad name", { type = "spring", stiffness = 1, dampening = 1 })
hl.curve(name_var, { type = "spring", stiffness = 1, dampening = 1 })
"#;
        assert_eq!(
            parse_curves(text),
            [(
                "stripGlide".to_owned(),
                Spring {
                    mass: 1.,
                    stiffness: 150.,
                    dampening: 25.
                }
            )]
        );
    }

    #[test]
    fn previews_merge_over_the_live_baseline() {
        let mut live = Snapshot::default();
        live.options.insert("general.gaps_in".into(), Value::Int(5));
        live.options
            .insert("general.border_size".into(), Value::Int(2));
        live.options
            .insert("general.layout".into(), Value::Text("hy3".into()));
        let mut draft = Overrides::default();
        draft
            .options
            .insert("general.gaps_in".into(), Value::Int(9));
        let mut touched = Touched::default();
        for key in ["general.gaps_in", "general.border_size", "general.layout"] {
            touched.options.insert(key.into());
        }
        let all = draft.preview(&live, &touched);
        assert_eq!(all.options["general.gaps_in"], Value::Int(9));
        // Reverted: back to the value from before the edit.
        assert_eq!(all.options["general.border_size"], Value::Int(2));
        // A live value Rune can't emit is left alone rather than sent back.
        assert!(!all.options.contains_key("general.layout"));
        assert!(draft.preview(&live, &Touched::default()).is_empty());
    }

    #[test]
    fn critically_damped_spring_settles() {
        let s = Spring {
            mass: 1.,
            stiffness: 100.,
            dampening: 20.,
        };
        assert!(s.response(0.).abs() < 1e-9);
        assert!((s.response(2.) - 1.).abs() < 1e-3);
    }

    #[tokio::test]
    #[ignore = "reads the running Hyprland session (read-only)"]
    async fn snapshot_reads_the_running_hyprland() {
        let snapshot = Hyprland::from_env()
            .unwrap()
            .settings_snapshot()
            .await
            .unwrap();
        assert_eq!(
            snapshot.options.len(),
            OPTIONS.len(),
            "{:?}",
            snapshot.options
        );
        eprintln!("{snapshot:#?}");
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rune-hypr-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn setup_and_save_keep_dotfile_symlinks() {
        let root = temp("links");
        let dotfiles = root.join("dotfiles");
        let hypr = root.join("hypr");
        std::fs::create_dir_all(dotfiles.join("modules")).unwrap();
        std::fs::create_dir_all(&hypr).unwrap();
        std::fs::write(dotfiles.join("hyprland.lua"), "require(\"modules.env\")").unwrap();
        std::os::unix::fs::symlink(dotfiles.join("hyprland.lua"), hypr.join("hyprland.lua"))
            .unwrap();
        std::os::unix::fs::symlink(dotfiles.join("modules"), hypr.join("modules")).unwrap();

        assert_eq!(setup_state(&hypr).await, Setup::NotAdded);
        setup(&hypr, &Overrides::default()).await.unwrap();
        setup(&hypr, &Overrides::default()).await.unwrap();
        save(&hypr, &sample()).await.unwrap();

        assert!(
            std::fs::symlink_metadata(hypr.join("hyprland.lua"))
                .unwrap()
                .is_symlink()
        );
        assert!(
            std::fs::symlink_metadata(hypr.join("modules"))
                .unwrap()
                .is_symlink()
        );
        let entry = std::fs::read_to_string(dotfiles.join("hyprland.lua")).unwrap();
        assert_eq!(entry.matches(REQUIRE_LINE).count(), 1);
        assert!(entry.starts_with("require(\"modules.env\")\n"));
        assert_eq!(setup_state(&hypr).await, Setup::Added);
        assert_eq!(
            std::fs::read_to_string(dotfiles.join("hyprland.lua.rune-bak")).unwrap(),
            "require(\"modules.env\")"
        );
        assert_eq!(
            std::fs::read_to_string(dotfiles.join("modules/rune.lua")).unwrap(),
            sample().render()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn finds_which_dotfile_sets_each_value() {
        let root = temp("sources");
        std::fs::create_dir_all(root.join("modules")).unwrap();
        std::fs::write(
            root.join("hyprland.lua"),
            "hl.monitor({})\nrequire(\"modules.general\")\n-- require(\"modules.old\")\nrequire(\"modules.decorations\")\nrequire(\"modules.rules\")\nrequire(\"modules.rune\")\nrequire(\"../../etc/passwd\")\n",
        )
        .unwrap();
        std::fs::write(
            root.join("modules/general.lua"),
            "hl.config({ general = { gaps_in = 5, layout = \"scrolling\" } })",
        )
        .unwrap();
        std::fs::write(
            root.join("modules/old.lua"),
            "hl.config({ general = { gaps_in = 1 } })",
        )
        .unwrap();
        std::fs::write(
            root.join("modules/decorations.lua"),
            "hl.config({ decoration = { rounding = 16, -- active_opacity = 1\n } })\nhl.curve(\"snap\", { type = \"spring\", stiffness = 280, dampening = 34 })\nhl.animation({ leaf = \"fade\", spring = \"snap\" })",
        )
        .unwrap();
        // A window rule's `rounding` is not the decoration option, and a keybind
        // changing the layout at runtime doesn't set it at load.
        std::fs::write(
            root.join("modules/rules.lua"),
            "rule({ rounding = 12 })\nlocal function f() hl.config({ general = { layout = nxt } }) end",
        )
        .unwrap();
        let found = read_dotfiles(&root).await;
        let src = |k: &str| found.sources.options.get(k).map(String::as_str);
        assert_eq!(src("general.gaps_in"), Some("modules/general.lua"));
        assert_eq!(src("general.layout"), Some("modules/general.lua"));
        assert_eq!(src("decoration.rounding"), Some("modules/decorations.lua"));
        assert_eq!(src("decoration.active_opacity"), None);
        assert_eq!(found.sources.curves["snap"], "modules/decorations.lua");
        assert_eq!(found.sources.leaves["fade"], "modules/decorations.lua");
        assert_eq!(found.curves["snap"].spring().unwrap().stiffness, 280.);
        assert_eq!(found.entry.last_line, "require(\"../../etc/passwd\")");
        assert_eq!(
            required_files(&std::fs::read_to_string(root.join("hyprland.lua")).unwrap()),
            [
                "modules/general.lua",
                "modules/decorations.lua",
                "modules/rules.lua"
            ]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn setup_without_lua_config_names_the_file() {
        let root = temp("missing");
        assert_eq!(setup_state(&root).await, Setup::NoLuaConfig);
        let err = setup(&root, &Overrides::default()).await.unwrap_err();
        assert!(err.to_string().contains("hyprland.lua"), "{err}");
        std::fs::remove_dir_all(root).unwrap();
    }
}

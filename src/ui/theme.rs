//! Theme catalog and role resolution.
//!
//! Every palette is authored once in 24-bit color. Each tone also names its
//! 16-color stand-in, and the xterm-256 fallback is the perceptually nearest
//! cube or gray-ramp entry unless the tone pins one. Terminal-palette tones are
//! used unchanged at every depth, so they follow the user's own terminal theme.
use super::{Appearance, ColorPolicy};
use ratatui::style::{Color, Modifier, Style};
use std::sync::OnceLock;

mod catalog;

/// Stable configuration names, in palette order.
pub const THEMES: &[&str] = &NAMES;
const NAMES: [&str; catalog::CATALOG.len()] = {
    let mut names = [""; catalog::CATALOG.len()];
    let mut index = 0;
    while index < names.len() {
        names[index] = catalog::CATALOG[index].name;
        index += 1;
    }
    names
};
static SPECS: [ThemeSpec; catalog::CATALOG.len()] = catalog::CATALOG;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    Monochrome,
    Ansi16,
    Ansi256,
    TrueColor,
}
impl ColorDepth {
    pub fn detect(policy: ColorPolicy) -> Self {
        Self::classify(policy, |name| {
            std::env::var_os(name).map(|value| value.to_string_lossy().into_owned())
        })
    }
    /// Conservative capability rules over environment values. Only explicit
    /// advertisements and terminals that always render 24-bit color select it.
    pub fn classify(policy: ColorPolicy, var: impl Fn(&str) -> Option<String>) -> Self {
        let set = |name: &str| var(name).is_some_and(|value| !value.is_empty());
        if policy == ColorPolicy::Never || (policy == ColorPolicy::Auto && set("NO_COLOR")) {
            return Self::Monochrome;
        }
        let terminal = var("TERM").unwrap_or_default();
        let multiplexed = terminal.starts_with("screen") || terminal.starts_with("tmux");
        if matches!(
            var("COLORTERM").unwrap_or_default().as_str(),
            "truecolor" | "24bit"
        ) || terminal.ends_with("-direct")
            || matches!(
                terminal.as_str(),
                "xterm-kitty" | "xterm-ghostty" | "alacritty" | "wezterm" | "foot"
            )
            // Windows Terminal does not set COLORTERM; a multiplexer may stand between.
            || (set("WT_SESSION") && !multiplexed)
        {
            Self::TrueColor
        } else if terminal.contains("256color") {
            Self::Ansi256
        } else {
            Self::Ansi16
        }
    }
}

/// Whether a theme follows the terminal's own colors or paints its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Terminal,
    Dark,
    Light,
}
impl Variant {
    pub fn name(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

/// One palette color. `rgb` is the authored value, `basic` its 16-color
/// stand-in (and the only value of a terminal-palette tone), and `indexed` an
/// optional pinned xterm-256 index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tone {
    pub rgb: Option<[u8; 3]>,
    pub basic: Color,
    pub indexed: Option<u8>,
}
impl Tone {
    pub const fn rgb(hex: u32, basic: Color) -> Self {
        Self {
            rgb: Some([(hex >> 16) as u8, (hex >> 8) as u8, hex as u8]),
            basic,
            indexed: None,
        }
    }
    pub const fn terminal(basic: Color) -> Self {
        Self {
            rgb: None,
            basic,
            indexed: None,
        }
    }
    /// Keep a theme's character where the nearest xterm-256 entry would not.
    pub const fn indexed(self, index: u8) -> Self {
        Self {
            indexed: Some(index),
            ..self
        }
    }
    fn at(self, depth: ColorDepth, indexed: u8) -> Option<Color> {
        match (depth, self.rgb) {
            (ColorDepth::Monochrome, _) => None,
            (ColorDepth::TrueColor, Some([r, g, b])) => Some(Color::Rgb(r, g, b)),
            (ColorDepth::Ansi256, Some(_)) => Some(Color::Indexed(indexed)),
            _ => Some(self.basic),
        }
    }
}

/// Semantic colors shared by every screen.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub background: Tone,
    /// Raised rows, such as a selected list entry.
    pub surface: Tone,
    /// Typed text and primary interface text.
    pub text: Tone,
    /// Text still to be typed.
    pub subtext: Tone,
    /// Hints and secondary labels.
    pub muted: Tone,
    pub accent: Tone,
    pub caret: Tone,
    pub error: Tone,
    pub extra: Tone,
    pub pace: Tone,
}
const TONES: usize = 10;
impl Palette {
    pub const ROLES: [&'static str; TONES] = [
        "background",
        "surface",
        "text",
        "subtext",
        "muted",
        "accent",
        "caret",
        "error",
        "extra",
        "pace",
    ];
    pub const fn tones(&self) -> [Tone; TONES] {
        [
            self.background,
            self.surface,
            self.text,
            self.subtext,
            self.muted,
            self.accent,
            self.caret,
            self.error,
            self.extra,
            self.pace,
        ]
    }
}

/// Attribution for a palette adapted from another project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Credit {
    pub project: &'static str,
    pub copyright: &'static str,
    pub license: &'static str,
    pub url: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ThemeSpec {
    pub name: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub variant: Variant,
    pub credit: Option<&'static Credit>,
    pub palette: Palette,
}
impl ThemeSpec {
    pub fn all() -> &'static [ThemeSpec] {
        &SPECS
    }
    pub fn find(name: &str) -> Option<&'static ThemeSpec> {
        SPECS.iter().find(|spec| spec.name == name)
    }
    /// Concrete colors for every role at `depth`, in [`Palette::ROLES`] order.
    pub fn colors(&self, depth: ColorDepth) -> [Option<Color>; TONES] {
        let indexed = self.indexed();
        let tones = self.palette.tones();
        std::array::from_fn(|role| tones[role].at(depth, indexed[role]))
    }
    fn indexed(&self) -> [u8; TONES] {
        static CACHE: OnceLock<Vec<[u8; TONES]>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| {
            let candidates: Vec<[f64; 3]> = (16..=255).map(|index| oklab(xterm(index))).collect();
            SPECS
                .iter()
                .map(|spec| {
                    spec.palette.tones().map(|tone| {
                        tone.indexed.unwrap_or_else(|| {
                            tone.rgb.map_or(0, |rgb| nearest_xterm(rgb, &candidates))
                        })
                    })
                })
                .collect()
        });
        SPECS
            .iter()
            .position(|spec| spec.name == self.name)
            .map_or([0; TONES], |index| cache[index])
    }
}

/// Resolved styles for the semantic roles of one theme at one color depth.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub background: Style,
    pub pending: Style,
    pub correct: Style,
    pub incorrect: Style,
    pub extra: Style,
    pub muted: Style,
    pub accent: Style,
    pub caret: Style,
    pub pace: Style,
    /// A highlighted list row.
    pub selected: Style,
    /// Live timer, progress and speed while typing.
    pub status: Style,
}
impl Theme {
    pub fn from_preferences(appearance: &Appearance, depth: ColorDepth) -> Self {
        Self::resolve(
            ThemeSpec::find(&appearance.theme).unwrap_or(&SPECS[0]),
            appearance,
            depth,
        )
    }
    pub fn resolve(spec: &ThemeSpec, appearance: &Appearance, depth: ColorDepth) -> Self {
        let [
            background,
            surface,
            text,
            subtext,
            muted,
            accent,
            caret,
            error,
            extra,
            pace,
        ] = spec.colors(depth);
        let base = match (background, text) {
            (Some(background), Some(text)) => Style::default().bg(background).fg(text),
            _ => Style::default(),
        };
        let color = |value: Option<Color>| value.map_or(base, |value| base.fg(value));
        // A designed subdued color needs no help. Terminal-palette and 16-color
        // pending text also dims, so it stays distinct where colors are approximate.
        let precise = spec.palette.subtext.rgb.is_some()
            && matches!(depth, ColorDepth::Ansi256 | ColorDepth::TrueColor);
        let error_cue = if appearance.error_underline || depth == ColorDepth::Monochrome {
            Modifier::UNDERLINED
        } else {
            Modifier::REVERSED
        };
        let accent = color(accent);
        Self {
            background: base,
            pending: if precise {
                color(subtext)
            } else {
                color(subtext).add_modifier(Modifier::DIM)
            },
            correct: if appearance.typed_bold {
                base.add_modifier(Modifier::BOLD)
            } else {
                base
            },
            incorrect: color(error).add_modifier(error_cue),
            extra: color(extra).add_modifier(error_cue | Modifier::BOLD),
            muted: color(muted),
            accent,
            caret: color(caret).add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
            pace: color(pace).add_modifier(Modifier::UNDERLINED),
            selected: surface.map_or(accent, |surface| accent.bg(surface)),
            // The inherited theme keeps its quiet status line.
            status: if spec.variant == Variant::Terminal {
                color(muted)
            } else {
                accent
            },
        }
    }
}

const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
/// The standardized part of the xterm palette: a 6×6×6 cube and a gray ramp.
/// Entries 0–15 are user-configurable and never used as derived fallbacks.
fn xterm(index: u8) -> [u8; 3] {
    match index {
        16..=231 => {
            let index = usize::from(index - 16);
            [LEVELS[index / 36], LEVELS[index / 6 % 6], LEVELS[index % 6]]
        }
        232..=255 => [8 + 10 * (index - 232); 3],
        _ => [0; 3],
    }
}
fn nearest_xterm(rgb: [u8; 3], candidates: &[[f64; 3]]) -> u8 {
    let target = oklab(rgb);
    let distance = |candidate: &[f64; 3]| {
        (0..3)
            .map(|axis| (candidate[axis] - target[axis]).powi(2))
            .sum::<f64>()
    };
    let (offset, _) = candidates
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| distance(a).total_cmp(&distance(b)))
        .expect("the xterm cube is not empty");
    16 + offset as u8
}
fn linear(channel: u8) -> f64 {
    let value = f64::from(channel) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
/// Björn Ottosson's OKLab, where Euclidean distance tracks perceived difference.
fn oklab(rgb: [u8; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(linear);
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    ]
}

#[cfg(test)]
mod tests;

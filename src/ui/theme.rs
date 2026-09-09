use super::{Appearance, ColorPolicy};
use ratatui::style::{Color, Modifier, Style};

pub const THEMES: &[&str] = &["terminal", "dark", "light", "warm", "high_contrast"];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    Monochrome,
    Ansi16,
    Ansi256,
    TrueColor,
}
impl ColorDepth {
    pub fn detect(policy: ColorPolicy) -> Self {
        if policy == ColorPolicy::Never
            || (policy == ColorPolicy::Auto
                && std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()))
        {
            return Self::Monochrome;
        }
        let terminal = std::env::var("TERM").unwrap_or_default();
        let color = std::env::var("COLORTERM").unwrap_or_default();
        if matches!(color.as_str(), "truecolor" | "24bit") {
            Self::TrueColor
        } else if terminal.contains("256color") {
            Self::Ansi256
        } else {
            Self::Ansi16
        }
    }
}
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
}
impl Theme {
    pub fn from_preferences(appearance: &Appearance, depth: ColorDepth) -> Self {
        let pick = |rgb: (u8, u8, u8), indexed: u8, basic: Color| match depth {
            ColorDepth::TrueColor => Color::Rgb(rgb.0, rgb.1, rgb.2),
            ColorDepth::Ansi256 => Color::Indexed(indexed),
            _ => basic,
        };
        let (background, foreground, pending, accent) = match appearance.theme.as_str() {
            "dark" => (
                pick((23, 26, 30), 234, Color::Black),
                pick((220, 223, 225), 253, Color::White),
                pick((135, 145, 150), 245, Color::DarkGray),
                pick((158, 191, 204), 110, Color::Cyan),
            ),
            "light" => (
                pick((248, 247, 242), 230, Color::White),
                pick((38, 45, 51), 235, Color::Black),
                pick((109, 117, 124), 243, Color::DarkGray),
                pick((55, 101, 140), 24, Color::Blue),
            ),
            "warm" => (
                pick((34, 29, 27), 235, Color::Black),
                pick((227, 213, 193), 187, Color::White),
                pick((158, 143, 125), 138, Color::DarkGray),
                pick((225, 174, 97), 179, Color::Yellow),
            ),
            "high_contrast" => (Color::Black, Color::White, Color::Gray, Color::Yellow),
            _ => (Color::Reset, Color::Reset, Color::DarkGray, Color::Reset),
        };
        let base = if depth == ColorDepth::Monochrome {
            Style::default()
        } else {
            Style::default().bg(background).fg(foreground)
        };
        let color = |value| {
            if depth == ColorDepth::Monochrome {
                base
            } else {
                base.fg(value)
            }
        };
        let error_cue = if appearance.error_underline || depth == ColorDepth::Monochrome {
            Modifier::UNDERLINED
        } else {
            Modifier::REVERSED
        };
        Self {
            background: base,
            pending: color(pending).add_modifier(Modifier::DIM),
            correct: if appearance.typed_bold {
                base.add_modifier(Modifier::BOLD)
            } else {
                base
            },
            incorrect: color(Color::Red).add_modifier(error_cue),
            extra: color(Color::Magenta).add_modifier(error_cue | Modifier::BOLD),
            muted: color(pending),
            accent: color(accent),
            caret: color(accent).add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
            pace: color(Color::Cyan).add_modifier(Modifier::UNDERLINED),
        }
    }
}

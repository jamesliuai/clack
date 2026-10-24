//! Plain-terminal theme previews for `clack themes show`.
use super::{Appearance, Caret, ColorDepth, Palette, Theme, ThemeSpec};
use ratatui::style::{Color, Modifier, Style};
use std::fmt::Write;

const INDENT: usize = 18;

/// The theme's identity, then a themed typing sample when colors are available.
pub fn entry(spec: &ThemeSpec, appearance: &Appearance, depth: ColorDepth) -> String {
    let mut text = format!(
        "{:<INDENT$}{} — {}\n",
        spec.name, spec.label, spec.description
    );
    if depth != ColorDepth::Monochrome {
        text.push_str(&" ".repeat(INDENT));
        text.push_str(&sample(spec, appearance, depth));
        text.push('\n');
    }
    text
}

/// One typing moment in the theme's own roles: the live timer, typed text, a
/// wrong letter, an extra letter, the caret and the text still to type.
pub fn sample(spec: &ThemeSpec, appearance: &Appearance, depth: ColorDepth) -> String {
    let theme = Theme::resolve(spec, appearance, depth);
    let caret = if appearance.caret == Caret::Block {
        theme.caret.add_modifier(Modifier::REVERSED)
    } else {
        theme.caret
    };
    let mut text = String::new();
    for (part, style) in [
        (" ", theme.background),
        ("23s", theme.status),
        ("  the quick br", theme.correct),
        ("p", theme.incorrect),
        ("wn fox", theme.correct),
        ("x", theme.extra),
        (" ", theme.correct),
        ("j", caret),
        ("umps over the lazy dog", theme.pending),
        (" ", theme.background),
    ] {
        let _ = write!(text, "\x1b[0;{}m{part}", sgr(style));
    }
    text.push_str("\x1b[0m");
    text
}

/// Machine-readable palette data, including every derived fallback.
pub fn describe(spec: &ThemeSpec) -> serde_json::Value {
    let indexed = spec.colors(ColorDepth::Ansi256);
    let colors: serde_json::Map<String, serde_json::Value> = Palette::ROLES
        .iter()
        .zip(spec.palette.tones())
        .zip(indexed)
        .map(|((role, tone), index)| {
            (
                (*role).to_owned(),
                serde_json::json!({
                    "rgb": tone.rgb.map(|[r, g, b]| format!("#{r:02X}{g:02X}{b:02X}")),
                    "xterm256": match index {
                        Some(Color::Indexed(index)) if tone.rgb.is_some() => Some(index),
                        _ => None,
                    },
                    "ansi": name(tone.basic),
                }),
            )
        })
        .collect();
    serde_json::json!({
        "name": spec.name,
        "label": spec.label,
        "variant": spec.variant.name(),
        "description": spec.description,
        "credit": spec.credit.map(|credit| serde_json::json!({
            "project": credit.project,
            "copyright": credit.copyright,
            "license": credit.license,
            "url": credit.url,
        })),
        "colors": colors,
    })
}

fn name(color: Color) -> &'static str {
    match color {
        Color::Black => "black",
        Color::Red => "red",
        Color::Green => "green",
        Color::Yellow => "yellow",
        Color::Blue => "blue",
        Color::Magenta => "magenta",
        Color::Cyan => "cyan",
        Color::Gray => "gray",
        Color::DarkGray => "dark_gray",
        Color::LightRed => "light_red",
        Color::LightGreen => "light_green",
        Color::LightYellow => "light_yellow",
        Color::LightBlue => "light_blue",
        Color::LightMagenta => "light_magenta",
        Color::LightCyan => "light_cyan",
        Color::White => "white",
        _ => "reset",
    }
}

/// Select Graphic Rendition parameters for a resolved style.
fn sgr(style: Style) -> String {
    let mut codes = Vec::with_capacity(6);
    for (modifier, code) in [
        (Modifier::BOLD, "1"),
        (Modifier::DIM, "2"),
        (Modifier::UNDERLINED, "4"),
        (Modifier::REVERSED, "7"),
    ] {
        if style.add_modifier.contains(modifier) {
            codes.push(code.to_owned());
        }
    }
    codes.extend(style.fg.map(|color| color_code(color, 30)));
    codes.extend(style.bg.map(|color| color_code(color, 40)));
    codes.join(";")
}
fn color_code(color: Color, base: u8) -> String {
    let basic = |offset: u8| (base + offset).to_string();
    let bright = |offset: u8| (base + 60 + offset).to_string();
    match color {
        Color::Reset => basic(9),
        Color::Black => basic(0),
        Color::Red => basic(1),
        Color::Green => basic(2),
        Color::Yellow => basic(3),
        Color::Blue => basic(4),
        Color::Magenta => basic(5),
        Color::Cyan => basic(6),
        Color::Gray => basic(7),
        Color::DarkGray => bright(0),
        Color::LightRed => bright(1),
        Color::LightGreen => bright(2),
        Color::LightYellow => bright(3),
        Color::LightBlue => bright(4),
        Color::LightMagenta => bright(5),
        Color::LightCyan => bright(6),
        Color::White => bright(7),
        Color::Indexed(index) => format!("{};5;{index}", base + 8),
        Color::Rgb(r, g, b) => format!("{};2;{r};{g};{b}", base + 8),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_use_only_the_requested_color_depth() {
        let appearance = Appearance::default();
        for spec in ThemeSpec::all() {
            assert!(
                !entry(spec, &appearance, ColorDepth::Monochrome).contains('\x1b'),
                "monochrome output has no escapes"
            );
            let truecolor = sample(spec, &appearance, ColorDepth::TrueColor);
            let indexed = sample(spec, &appearance, ColorDepth::Ansi256);
            let basic = sample(spec, &appearance, ColorDepth::Ansi16);
            let uses = |text: &str, form: &str| {
                text.contains(&format!("38;{form};")) || text.contains(&format!("48;{form};"))
            };
            assert!(!uses(&indexed, "2") && !uses(&basic, "2") && !uses(&basic, "5"));
            if spec.palette.background.rgb.is_some() {
                assert!(truecolor.contains("48;2;") && indexed.contains("48;5;"));
            }
            for text in [&truecolor, &indexed, &basic] {
                assert!(
                    text.ends_with("\x1b[0m"),
                    "{} leaves styles active",
                    spec.name
                );
                assert!(text.contains("the quick br") && text.contains("umps over the lazy dog"));
            }
        }
        let nord = ThemeSpec::find("nord").unwrap();
        let text = sample(nord, &appearance, ColorDepth::TrueColor);
        assert!(
            text.contains("\x1b[0;4;38;2;191;97;106;48;2;46;52;64mp"),
            "{text:?}"
        );
    }

    #[test]
    fn descriptions_report_authored_and_derived_colors() {
        let nord = describe(ThemeSpec::find("nord").unwrap());
        assert_eq!(nord["variant"], "dark");
        assert_eq!(nord["credit"]["license"], "MIT");
        assert_eq!(nord["colors"]["accent"]["rgb"], "#88C0D0");
        assert_eq!(nord["colors"]["accent"]["ansi"], "cyan");
        assert!(nord["colors"]["accent"]["xterm256"].as_u64().is_some());
        let terminal = describe(ThemeSpec::find("terminal").unwrap());
        assert!(terminal["credit"].is_null());
        assert!(terminal["colors"]["background"]["rgb"].is_null());
        assert!(terminal["colors"]["background"]["xterm256"].is_null());
        assert_eq!(terminal["colors"]["subtext"]["ansi"], "dark_gray");
        assert_eq!(
            describe(ThemeSpec::find("dark").unwrap())["colors"]["accent"]["xterm256"],
            110,
            "pinned fallbacks are reported as used"
        );
    }
}

use super::*;

const BACKGROUND: usize = 0;
const SURFACE: usize = 1;
const TEXT: usize = 2;
const SUBTEXT: usize = 3;
const ACCENT: usize = 5;
const CARET: usize = 6;
const ERROR: usize = 7;
const EXTRA: usize = 8;
const PACE: usize = 9;

/// WCAG 2 contrast ratio, from 1 to 21.
fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let luminance =
        |[r, g, b]: [u8; 3]| 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
fn difference(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (a, b) = (oklab(a), oklab(b));
    (0..3)
        .map(|axis| (a[axis] - b[axis]).powi(2))
        .sum::<f64>()
        .sqrt()
}
/// The sRGB a terminal displays for every role, for themes that paint their own colors.
fn displayed(spec: &ThemeSpec, depth: ColorDepth) -> Option<[[u8; 3]; TONES]> {
    let colors = spec.colors(depth);
    let mut shown = [[0; 3]; TONES];
    for (role, color) in colors.into_iter().enumerate() {
        shown[role] = match color? {
            Color::Rgb(r, g, b) => [r, g, b],
            Color::Indexed(index) => xterm(index),
            _ => return None,
        };
    }
    Some(shown)
}

#[test]
fn catalog_names_are_stable_unique_configuration_values() {
    assert_eq!(THEMES.len(), 21);
    assert_eq!(
        THEMES[0], "terminal",
        "unknown names fall back to the first entry"
    );
    assert_eq!(
        &THEMES[..5],
        ["terminal", "dark", "light", "warm", "high_contrast"],
        "original themes keep their order"
    );
    for (index, spec) in ThemeSpec::all().iter().enumerate() {
        assert_eq!(THEMES[index], spec.name);
        assert!(
            spec.name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
            "{}",
            spec.name
        );
        assert_eq!(
            ThemeSpec::find(spec.name).map(|found| found.name),
            Some(spec.name)
        );
        assert_eq!(
            ThemeSpec::all()
                .iter()
                .filter(|other| other.label == spec.label)
                .count(),
            1,
            "duplicate label {}",
            spec.label
        );
        // The palette help line shows the label and description together.
        assert!(
            spec.description.chars().count() <= 32,
            "{} description is too long",
            spec.name
        );
        assert_eq!(spec.credit.is_none(), index < 5, "{} credit", spec.name);
        assert!(spec.credit.is_none_or(|credit| credit.license == "MIT"));
    }
    assert!(
        ThemeSpec::find("Nord").is_none(),
        "names are case-sensitive"
    );
}

#[test]
fn every_painted_theme_is_readable_at_truecolor_and_xterm_256() {
    // Minimum contrast against the background for text, subtext, muted,
    // accent, caret, error, extra and pace. Quantized fallbacks get a small
    // allowance because the xterm cube is coarse.
    let truecolor = [7.0, 3.0, 2.5, 3.0, 3.0, 3.0, 2.5, 2.5];
    let indexed = [6.0, 2.8, 2.3, 2.8, 2.8, 2.8, 2.3, 2.3];
    for spec in ThemeSpec::all() {
        for (depth, minimum, selection) in [
            (ColorDepth::TrueColor, truecolor, 3.0),
            (ColorDepth::Ansi256, indexed, 2.6),
        ] {
            let Some(color) = displayed(spec, depth) else {
                assert!(matches!(spec.name, "terminal" | "high_contrast"));
                continue;
            };
            let background = color[BACKGROUND];
            for (offset, limit) in minimum.into_iter().enumerate() {
                let role = TEXT + offset;
                let ratio = contrast(color[role], background);
                assert!(
                    ratio >= limit,
                    "{} {depth:?} {} contrast {ratio:.2} < {limit}",
                    spec.name,
                    Palette::ROLES[role]
                );
            }
            let raised = contrast(color[SURFACE], background);
            assert!(
                (1.05..=2.2).contains(&raised),
                "{} {depth:?} surface contrast {raised:.2}",
                spec.name
            );
            let highlighted = contrast(color[ACCENT], color[SURFACE]);
            assert!(
                highlighted >= selection,
                "{} {depth:?} selected row {highlighted:.2}",
                spec.name
            );
            let typed = contrast(color[TEXT], color[SUBTEXT]);
            assert!(
                typed >= 1.6,
                "{} {depth:?} typed and pending text are too similar ({typed:.2})",
                spec.name
            );
            for (a, b) in [
                (BACKGROUND, SURFACE),
                (TEXT, SUBTEXT),
                (ACCENT, ERROR),
                (CARET, ERROR),
                (ERROR, EXTRA),
                (CARET, SUBTEXT),
                (PACE, SUBTEXT),
            ] {
                assert!(
                    difference(color[a], color[b]) >= 0.03,
                    "{} {depth:?} {} and {} are indistinguishable",
                    spec.name,
                    Palette::ROLES[a],
                    Palette::ROLES[b]
                );
            }
        }
    }
}

#[test]
fn sixteen_color_fallbacks_follow_one_convention() {
    for spec in ThemeSpec::all() {
        let palette = spec.palette;
        if palette.background.rgb.is_none() {
            continue;
        }
        let tones = palette.tones();
        assert!(
            tones.iter().all(|tone| tone.rgb.is_some()),
            "{} mixes painted and terminal tones",
            spec.name
        );
        let [r, g, b] = palette.background.rgb.unwrap();
        let light = contrast([r, g, b], [0; 3]) > contrast([r, g, b], [255; 3]);
        assert_eq!(
            spec.variant == Variant::Light,
            light,
            "{} variant",
            spec.name
        );
        let (background, text) = if light {
            (Color::White, Color::Black)
        } else {
            (Color::Black, Color::White)
        };
        assert_eq!(palette.background.basic, background, "{}", spec.name);
        assert_eq!(palette.text.basic, text, "{}", spec.name);
        // No selection bar where the background is the terminal's own black or white.
        assert_eq!(palette.surface.basic, background, "{}", spec.name);
        assert_eq!(palette.subtext.basic, Color::DarkGray, "{}", spec.name);
        assert_eq!(palette.muted.basic, Color::DarkGray, "{}", spec.name);
        assert_eq!(palette.error.basic, Color::Red, "{}", spec.name);
        assert_ne!(palette.extra.basic, Color::Red, "{} extra", spec.name);
        assert!(
            !matches!(
                palette.accent.basic,
                Color::Reset | Color::Black | Color::White | Color::Gray | Color::DarkGray
            ),
            "{} accent needs a hue at 16 colors",
            spec.name
        );
    }
}

#[test]
fn pinned_indexed_fallbacks_differ_from_the_derived_match() {
    let candidates: Vec<[f64; 3]> = (16..=255).map(|index| oklab(xterm(index))).collect();
    for spec in ThemeSpec::all() {
        for (role, tone) in spec.palette.tones().into_iter().enumerate() {
            if let (Some(rgb), Some(pin)) = (tone.rgb, tone.indexed) {
                assert!((16..=255).contains(&pin));
                assert_ne!(
                    nearest_xterm(rgb, &candidates),
                    pin,
                    "{} {} pins the derived index; remove the pin",
                    spec.name,
                    Palette::ROLES[role]
                );
            }
        }
    }
}

#[test]
fn nearest_xterm_is_exact_for_standard_entries_and_ignores_user_colors() {
    let candidates: Vec<[f64; 3]> = (16..=255).map(|index| oklab(xterm(index))).collect();
    for index in 16..=255 {
        assert_eq!(nearest_xterm(xterm(index), &candidates), index);
    }
    assert_eq!(xterm(16), [0, 0, 0]);
    assert_eq!(xterm(231), [255, 255, 255]);
    assert_eq!(xterm(196), [255, 0, 0]);
    assert_eq!(xterm(232), [8, 8, 8]);
    assert_eq!(xterm(255), [238, 238, 238]);
    assert!(nearest_xterm([255, 0, 0], &candidates) >= 16);
}

#[test]
fn roles_follow_depth_and_inherited_theme_conventions() {
    let appearance = Appearance::default();
    let terminal = Theme::from_preferences(&appearance, ColorDepth::TrueColor);
    assert_eq!(terminal.background.bg, Some(Color::Reset));
    assert_eq!(terminal.correct.fg, Some(Color::Reset));
    assert_eq!(terminal.pending.fg, Some(Color::DarkGray));
    assert!(terminal.pending.add_modifier.contains(Modifier::DIM));
    assert_eq!(
        terminal.status, terminal.muted,
        "the default status line stays quiet"
    );
    let unknown = Appearance {
        theme: "not_a_theme".into(),
        ..Appearance::default()
    };
    assert_eq!(
        Theme::from_preferences(&unknown, ColorDepth::TrueColor).background,
        terminal.background
    );

    let nord = Appearance {
        theme: "nord".into(),
        ..Appearance::default()
    };
    let painted = Theme::from_preferences(&nord, ColorDepth::TrueColor);
    assert_eq!(painted.background.bg, Some(Color::Rgb(0x2E, 0x34, 0x40)));
    assert_eq!(painted.correct.fg, Some(Color::Rgb(0xEC, 0xEF, 0xF4)));
    assert!(
        !painted.pending.add_modifier.contains(Modifier::DIM),
        "designed subtext is not dimmed again"
    );
    assert_eq!(painted.status, painted.accent);
    assert_eq!(painted.selected.fg, painted.accent.fg);
    assert_eq!(painted.selected.bg, Some(Color::Rgb(0x43, 0x4C, 0x5E)));
    let basic = Theme::from_preferences(&nord, ColorDepth::Ansi16);
    assert!(basic.pending.add_modifier.contains(Modifier::DIM));
    assert_eq!(
        basic.selected.bg, basic.background.bg,
        "no bar at 16 colors"
    );
    let indexed = Theme::from_preferences(&nord, ColorDepth::Ansi256);
    assert!(matches!(indexed.accent.fg, Some(Color::Indexed(_))));
    let monochrome = Theme::from_preferences(&nord, ColorDepth::Monochrome);
    assert_eq!(monochrome.selected, Style::default());
    assert!(monochrome.pending.add_modifier.contains(Modifier::DIM));
}

#[test]
fn color_depth_rules_are_conservative() {
    use ColorDepth::*;
    let classify = |policy, pairs: &[(&str, &str)]| {
        ColorDepth::classify(policy, |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        })
    };
    let auto = ColorPolicy::Auto;
    assert_eq!(
        classify(ColorPolicy::Never, &[("COLORTERM", "truecolor")]),
        Monochrome
    );
    assert_eq!(
        classify(auto, &[("NO_COLOR", "1"), ("COLORTERM", "truecolor")]),
        Monochrome
    );
    assert_eq!(
        classify(auto, &[("NO_COLOR", ""), ("TERM", "xterm-256color")]),
        Ansi256
    );
    assert_eq!(
        classify(
            ColorPolicy::Always,
            &[("NO_COLOR", "1"), ("COLORTERM", "24bit")]
        ),
        TrueColor
    );
    assert_eq!(classify(auto, &[]), Ansi16);
    assert_eq!(classify(auto, &[("TERM", "xterm")]), Ansi16);
    assert_eq!(classify(auto, &[("TERM", "linux")]), Ansi16);
    assert_eq!(classify(auto, &[("TERM", "xterm-256color")]), Ansi256);
    for terminal in [
        "xterm-kitty",
        "xterm-ghostty",
        "alacritty",
        "wezterm",
        "foot",
        "xterm-direct",
    ] {
        assert_eq!(
            classify(auto, &[("TERM", terminal)]),
            TrueColor,
            "{terminal}"
        );
    }
    assert_eq!(classify(auto, &[("WT_SESSION", "1f9e")]), TrueColor);
    assert_eq!(
        classify(auto, &[("WT_SESSION", "1f9e"), ("TERM", "xterm-256color")]),
        TrueColor
    );
    assert_eq!(
        classify(auto, &[("WT_SESSION", "1f9e"), ("TERM", "screen-256color")]),
        Ansi256,
        "a multiplexer may not pass 24-bit color through"
    );
    assert_eq!(
        classify(auto, &[("WT_SESSION", "1f9e"), ("TERM", "tmux-256color")]),
        Ansi256
    );
}

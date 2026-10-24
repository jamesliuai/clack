//! Shipped themes. Adapted palettes keep their upstream colors and name the
//! source swatch for each role; tones without a name are Clack's own. Their
//! notices are reproduced in THIRD-PARTY-NOTICES.md.
use super::{Credit, Palette, ThemeSpec, Tone, Variant};
use ratatui::style::Color::{
    self, Black, Blue, Cyan, DarkGray, Gray, Green, LightBlue, LightCyan, LightMagenta, LightRed,
    Magenta, Red, Reset, White, Yellow,
};

const fn rgb(hex: u32, basic: Color) -> Tone {
    Tone::rgb(hex, basic)
}
const fn terminal(basic: Color) -> Tone {
    Tone::terminal(basic)
}

const NORD: Credit = mit(
    "Nord",
    "Copyright (c) 2016-present Sven Greb",
    "https://www.nordtheme.com",
);
const DRACULA: Credit = mit(
    "Dracula",
    "Copyright (c) 2023 Dracula Theme",
    "https://draculatheme.com",
);
const GRUVBOX: Credit = mit(
    "Gruvbox",
    "Copyright (c) 2018 Pavel Pertsev",
    "https://github.com/morhetz/gruvbox",
);
const SOLARIZED: Credit = mit(
    "Solarized",
    "Copyright (c) 2011 Ethan Schoonover",
    "https://ethanschoonover.com/solarized/",
);
const CATPPUCCIN: Credit = mit(
    "Catppuccin",
    "Copyright (c) 2021 Catppuccin",
    "https://catppuccin.com",
);
const TOKYO_NIGHT: Credit = mit(
    "Tokyo Night",
    "Copyright (c) 2018-present Enkia",
    "https://github.com/enkia/tokyo-night-vscode-theme",
);
const ROSE_PINE: Credit = mit(
    "Rosé Pine",
    "Copyright (c) 2023 Rosé Pine",
    "https://rosepinetheme.com",
);
const EVERFOREST: Credit = mit(
    "Everforest",
    "Copyright (c) 2019 sainnhe",
    "https://github.com/sainnhe/everforest",
);
const KANAGAWA: Credit = mit(
    "Kanagawa",
    "Copyright (c) 2021 Tommaso Laurenzi",
    "https://github.com/rebelot/kanagawa.nvim",
);
const ONE_DARK: Credit = mit(
    "One Dark",
    "Copyright (c) 2011-2022 GitHub Inc.",
    "https://github.com/atom/atom",
);
const SYNTHWAVE: Credit = mit(
    "SynthWave '84",
    "Copyright (c) 2019 Robb Owen",
    "https://github.com/robb0wen/synthwave-vscode",
);
const NIGHT_OWL: Credit = mit(
    "Night Owl",
    "Copyright (c) 2018 Sarah Drasner",
    "https://github.com/sdras/night-owl-vscode-theme",
);
const fn mit(project: &'static str, copyright: &'static str, url: &'static str) -> Credit {
    Credit {
        project,
        copyright,
        license: "MIT",
        url,
    }
}

pub(super) const CATALOG: [ThemeSpec; 21] = [
    ThemeSpec {
        name: "terminal",
        label: "Terminal",
        description: "your terminal's own colors",
        variant: Variant::Terminal,
        credit: None,
        palette: Palette {
            background: terminal(Reset),
            surface: terminal(Reset),
            text: terminal(Reset),
            subtext: terminal(DarkGray),
            muted: terminal(DarkGray),
            accent: terminal(Reset),
            caret: terminal(Reset),
            error: terminal(Red),
            extra: terminal(Magenta),
            pace: terminal(Cyan),
        },
    },
    // Clack originals. Their pinned xterm-256 entries predate derived fallbacks.
    ThemeSpec {
        name: "dark",
        label: "Dark",
        description: "quiet slate, steel-blue accent",
        variant: Variant::Dark,
        credit: None,
        palette: Palette {
            background: rgb(0x171A1E, Black),
            surface: rgb(0x252A30, Black),
            text: rgb(0xDCDFE1, White),
            subtext: rgb(0x7D878C, DarkGray),
            muted: rgb(0x68727A, DarkGray),
            accent: rgb(0x9EBFCC, Cyan).indexed(110),
            caret: rgb(0x9EBFCC, Cyan).indexed(110),
            error: rgb(0xE8737A, Red),
            extra: rgb(0xB5525C, Magenta),
            pace: rgb(0xC9A96E, Yellow),
        },
    },
    ThemeSpec {
        name: "light",
        label: "Light",
        description: "warm paper, ink-blue accent",
        variant: Variant::Light,
        credit: None,
        palette: Palette {
            background: rgb(0xF8F7F2, White).indexed(230),
            surface: rgb(0xE9E7DE, White),
            text: rgb(0x262D33, Black).indexed(235),
            subtext: rgb(0x7A8289, DarkGray),
            muted: rgb(0x868D93, DarkGray),
            accent: rgb(0x37658C, Blue),
            caret: rgb(0x37658C, Blue),
            error: rgb(0xC0392B, Red),
            extra: rgb(0x8E2C3E, Magenta),
            pace: rgb(0x8A5A9E, Cyan),
        },
    },
    ThemeSpec {
        name: "warm",
        label: "Warm",
        description: "ember brown with an amber glow",
        variant: Variant::Dark,
        credit: None,
        palette: Palette {
            background: rgb(0x221D1B, Black).indexed(235),
            surface: rgb(0x322A26, Black),
            text: rgb(0xE3D5C1, White).indexed(187),
            subtext: rgb(0x8F8170, DarkGray),
            muted: rgb(0x7D705F, DarkGray),
            accent: rgb(0xE1AE61, Yellow),
            caret: rgb(0xE1AE61, Yellow),
            error: rgb(0xE2725B, Red),
            extra: rgb(0xB0503E, Magenta),
            pace: rgb(0x9DB58A, Green),
        },
    },
    // Terminal-palette colors at every depth, so a tuned terminal is respected.
    ThemeSpec {
        name: "high_contrast",
        label: "High contrast",
        description: "maximum legibility",
        variant: Variant::Dark,
        credit: None,
        palette: Palette {
            background: terminal(Black),
            surface: terminal(Black),
            text: terminal(White),
            subtext: terminal(Gray),
            muted: terminal(Gray),
            accent: terminal(Yellow),
            caret: terminal(Yellow),
            error: terminal(LightRed),
            extra: terminal(LightMagenta),
            pace: terminal(LightCyan),
        },
    },
    ThemeSpec {
        name: "nord",
        label: "Nord",
        description: "arctic, north-bluish calm",
        variant: Variant::Dark,
        credit: Some(&NORD),
        palette: Palette {
            background: rgb(0x2E3440, Black),  // nord0
            surface: rgb(0x434C5E, Black),     // nord2
            text: rgb(0xECEFF4, White),        // nord6
            subtext: rgb(0x7B88A1, DarkGray),  // between nord3 and nord4
            muted: rgb(0x6B778E, DarkGray),    // between nord3 and nord4
            accent: rgb(0x88C0D0, Cyan),       // nord8
            caret: rgb(0x88C0D0, Cyan),        // nord8
            error: rgb(0xBF616A, Red),         // nord11
            extra: rgb(0xD08770, Yellow),      // nord12
            pace: rgb(0xB48EAD, LightMagenta), // nord15
        },
    },
    ThemeSpec {
        name: "dracula",
        label: "Dracula",
        description: "vivid purple and pink at night",
        variant: Variant::Dark,
        credit: Some(&DRACULA),
        palette: Palette {
            background: rgb(0x282A36, Black),    // background
            surface: rgb(0x44475A, Black),       // selection
            text: rgb(0xF8F8F2, White),          // foreground
            subtext: rgb(0x6272A4, DarkGray),    // comment
            muted: rgb(0x6272A4, DarkGray),      // comment
            accent: rgb(0xBD93F9, LightMagenta), // purple
            caret: rgb(0xFF79C6, LightMagenta),  // pink
            error: rgb(0xFF5555, Red),           // red
            extra: rgb(0xFFB86C, Yellow),        // orange
            pace: rgb(0x8BE9FD, Cyan),           // cyan
        },
    },
    ThemeSpec {
        name: "gruvbox_dark",
        label: "Gruvbox Dark",
        description: "retro groove, warm earth tones",
        variant: Variant::Dark,
        credit: Some(&GRUVBOX),
        palette: Palette {
            background: rgb(0x282828, Black), // dark0
            surface: rgb(0x3C3836, Black),    // dark1
            text: rgb(0xEBDBB2, White),       // light1
            subtext: rgb(0x928374, DarkGray), // gray
            muted: rgb(0x7C6F64, DarkGray),   // dark4
            accent: rgb(0xFABD2F, Yellow),    // bright yellow
            caret: rgb(0xFE8019, Yellow),     // bright orange
            error: rgb(0xFB4934, Red),        // bright red
            extra: rgb(0xCC241D, Magenta),    // neutral red
            pace: rgb(0x8EC07C, Cyan),        // bright aqua
        },
    },
    // Solarized publishes its own xterm-256 approximations; they are pinned.
    ThemeSpec {
        name: "solarized_dark",
        label: "Solarized Dark",
        description: "precise teal, golden accents",
        variant: Variant::Dark,
        credit: Some(&SOLARIZED),
        palette: Palette {
            background: rgb(0x002B36, Black).indexed(234), // base03
            surface: rgb(0x073642, Black).indexed(235),    // base02
            text: rgb(0xEEE8D5, White).indexed(187),       // base2
            subtext: rgb(0x839496, DarkGray).indexed(244), // base0
            muted: rgb(0x586E75, DarkGray),                // base01
            accent: rgb(0xB58900, Yellow),                 // yellow
            caret: rgb(0x268BD2, Blue).indexed(33),        // blue
            error: rgb(0xDC322F, Red),                     // red
            extra: rgb(0xCB4B16, LightRed),                // orange
            pace: rgb(0x2AA198, Cyan),                     // cyan
        },
    },
    ThemeSpec {
        name: "catppuccin_mocha",
        label: "Catppuccin Mocha",
        description: "soothing pastels on mocha",
        variant: Variant::Dark,
        credit: Some(&CATPPUCCIN),
        palette: Palette {
            background: rgb(0x1E1E2E, Black),    // base
            surface: rgb(0x313244, Black),       // surface0
            text: rgb(0xCDD6F4, White),          // text
            subtext: rgb(0x7F849C, DarkGray),    // overlay1
            muted: rgb(0x6C7086, DarkGray),      // overlay0
            accent: rgb(0xCBA6F7, LightMagenta), // mauve
            caret: rgb(0xF5E0DC, White),         // rosewater
            error: rgb(0xF38BA8, Red),           // red
            extra: rgb(0xFAB387, Yellow),        // peach
            pace: rgb(0x89B4FA, LightBlue),      // blue
        },
    },
    ThemeSpec {
        name: "tokyo_night",
        label: "Tokyo Night",
        description: "neon city lights on indigo",
        variant: Variant::Dark,
        credit: Some(&TOKYO_NIGHT),
        palette: Palette {
            background: rgb(0x1A1B26, Black),   // night background
            surface: rgb(0x24283B, Black),      // storm background
            text: rgb(0xC0CAF5, White),         // variables
            subtext: rgb(0x787C99, DarkGray),   // interface foreground
            muted: rgb(0x565F89, DarkGray),     // comments
            accent: rgb(0x7AA2F7, LightBlue),   // blue
            caret: rgb(0xBB9AF7, LightMagenta), // magenta
            error: rgb(0xF7768E, Red),          // red
            extra: rgb(0xFF9E64, Yellow),       // orange
            pace: rgb(0x9ECE6A, Green),         // green
        },
    },
    ThemeSpec {
        name: "rose_pine",
        label: "Rosé Pine",
        description: "dusky rose and gold",
        variant: Variant::Dark,
        credit: Some(&ROSE_PINE),
        palette: Palette {
            background: rgb(0x191724, Black),    // base
            surface: rgb(0x403D52, Black),       // highlight med
            text: rgb(0xE0DEF4, White),          // text
            subtext: rgb(0x908CAA, DarkGray),    // subtle
            muted: rgb(0x6E6A86, DarkGray),      // muted
            accent: rgb(0xEBBCBA, LightMagenta), // rose
            caret: rgb(0xF6C177, Yellow),        // gold
            error: rgb(0xEB6F92, Red),           // love
            extra: rgb(0xB4637A, Magenta),       // love (dawn)
            pace: rgb(0x9CCFD8, Cyan),           // foam
        },
    },
    ThemeSpec {
        name: "everforest",
        label: "Everforest",
        description: "soft sage of a quiet forest",
        variant: Variant::Dark,
        credit: Some(&EVERFOREST),
        palette: Palette {
            background: rgb(0x2D353B, Black), // bg0
            surface: rgb(0x3D484D, Black),    // bg2
            text: rgb(0xD3C6AA, White),       // fg
            subtext: rgb(0x859289, DarkGray), // grey1
            muted: rgb(0x7A8478, DarkGray),   // grey0
            accent: rgb(0xA7C080, Green),     // green
            caret: rgb(0xDBBC7F, Yellow),     // yellow
            error: rgb(0xE67E80, Red),        // red
            extra: rgb(0xE69875, Magenta),    // orange
            pace: rgb(0x7FBBB3, Cyan),        // blue
        },
    },
    ThemeSpec {
        name: "kanagawa",
        label: "Kanagawa",
        description: "ink and waves after Hokusai",
        variant: Variant::Dark,
        credit: Some(&KANAGAWA),
        palette: Palette {
            background: rgb(0x1F1F28, Black), // sumiInk3
            surface: rgb(0x2D4F67, Black),    // waveBlue2
            text: rgb(0xDCD7BA, White),       // fujiWhite
            subtext: rgb(0x727169, DarkGray), // fujiGray
            muted: rgb(0x727169, DarkGray),   // fujiGray
            accent: rgb(0x7E9CD8, LightBlue), // crystalBlue
            caret: rgb(0xE6C384, Yellow),     // carpYellow
            error: rgb(0xE46876, Red),        // waveRed
            extra: rgb(0xC34043, Magenta),    // autumnRed
            pace: rgb(0x98BB6C, Green),       // springGreen
        },
    },
    ThemeSpec {
        name: "one_dark",
        label: "One Dark",
        description: "the classic Atom editor look",
        variant: Variant::Dark,
        credit: Some(&ONE_DARK),
        palette: Palette {
            background: rgb(0x282C34, Black),  // syntax background
            surface: rgb(0x3E4451, Black),     // selection
            text: rgb(0xD7DAE0, White),        // highlighted text
            subtext: rgb(0x828997, DarkGray),  // mono 2
            muted: rgb(0x636D83, DarkGray),    // gutter
            accent: rgb(0x61AFEF, LightBlue),  // blue
            caret: rgb(0x528BFF, LightBlue),   // syntax accent
            error: rgb(0xE06C75, Red),         // red
            extra: rgb(0xBE5046, Magenta),     // dark red
            pace: rgb(0xC678DD, LightMagenta), // purple
        },
    },
    ThemeSpec {
        name: "synthwave",
        label: "SynthWave '84",
        description: "retro neon from a night drive",
        variant: Variant::Dark,
        credit: Some(&SYNTHWAVE),
        palette: Palette {
            background: rgb(0x262335, Black),    // editor background
            surface: rgb(0x34294F, Black),       // line highlight
            text: rgb(0xFFFFFF, White),          // foreground
            subtext: rgb(0x848BBD, DarkGray),    // comments
            muted: rgb(0x848BBD, DarkGray),      // comments
            accent: rgb(0xFF7EDB, LightMagenta), // pink
            caret: rgb(0x36F9F6, LightCyan),     // neon cyan
            error: rgb(0xFE4450, Red),           // errors
            extra: rgb(0xF97E72, LightRed),      // coral
            pace: rgb(0xFEDE5D, Yellow),         // yellow
        },
    },
    ThemeSpec {
        name: "night_owl",
        label: "Night Owl",
        description: "deep ocean blue for night owls",
        variant: Variant::Dark,
        credit: Some(&NIGHT_OWL),
        palette: Palette {
            background: rgb(0x011627, Black),  // editor background
            surface: rgb(0x1D3B53, Black),     // selection
            text: rgb(0xD6DEEB, White),        // foreground
            subtext: rgb(0x637777, DarkGray),  // comments
            muted: rgb(0x5F7E97, DarkGray),    // interface foreground
            accent: rgb(0x82AAFF, LightBlue),  // functions
            caret: rgb(0x7FDBCA, LightCyan),   // operators
            error: rgb(0xEF5350, Red),         // errors
            extra: rgb(0xF78C6C, Yellow),      // numbers
            pace: rgb(0xC792EA, LightMagenta), // keywords
        },
    },
    ThemeSpec {
        name: "solarized_light",
        label: "Solarized Light",
        description: "precise teal ink on cream",
        variant: Variant::Light,
        credit: Some(&SOLARIZED),
        palette: Palette {
            background: rgb(0xFDF6E3, White),           // base3
            surface: rgb(0xEEE8D5, White).indexed(187), // base2
            text: rgb(0x073642, Black).indexed(235),    // base02
            subtext: rgb(0x657B83, DarkGray),           // base00
            muted: rgb(0x839496, DarkGray),             // base0
            accent: rgb(0x268BD2, Blue),                // blue
            caret: rgb(0xCB4B16, LightRed),             // orange
            error: rgb(0xDC322F, Red),                  // red
            extra: rgb(0xD33682, Magenta).indexed(125), // magenta
            pace: rgb(0x2AA198, Cyan),                  // cyan
        },
    },
    ThemeSpec {
        name: "catppuccin_latte",
        label: "Catppuccin Latte",
        description: "soothing pastels on latte",
        variant: Variant::Light,
        credit: Some(&CATPPUCCIN),
        palette: Palette {
            background: rgb(0xEFF1F5, White), // base
            surface: rgb(0xCCD0DA, White),    // surface0
            text: rgb(0x4C4F69, Black),       // text
            subtext: rgb(0x7C7F93, DarkGray), // overlay2
            muted: rgb(0x8C8FA1, DarkGray),   // overlay1
            accent: rgb(0x8839EF, Magenta),   // mauve
            caret: rgb(0x8839EF, Magenta),    // mauve
            error: rgb(0xD20F39, Red),        // red
            extra: rgb(0xFE640B, Yellow),     // peach
            pace: rgb(0x1E66F5, Blue),        // blue
        },
    },
    ThemeSpec {
        name: "rose_pine_dawn",
        label: "Rosé Pine Dawn",
        description: "morning parchment, pine and rose",
        variant: Variant::Light,
        credit: Some(&ROSE_PINE),
        palette: Palette {
            background: rgb(0xFAF4ED, White), // base
            surface: rgb(0xDFDAD9, White),    // highlight med
            text: rgb(0x464261, Black),       // text
            subtext: rgb(0x797593, DarkGray), // subtle
            muted: rgb(0x9893A5, DarkGray),   // muted
            accent: rgb(0x286983, Blue),      // pine
            caret: rgb(0x286983, Blue),       // pine
            error: rgb(0xB4637A, Red),        // love
            extra: rgb(0xD7827E, Magenta),    // rose
            pace: rgb(0x56949F, Cyan),        // foam
        },
    },
    // The nearest xterm-256 red would merge the faded orange accent with errors.
    ThemeSpec {
        name: "gruvbox_light",
        label: "Gruvbox Light",
        description: "retro groove on faded paper",
        variant: Variant::Light,
        credit: Some(&GRUVBOX),
        palette: Palette {
            background: rgb(0xFBF1C7, White),           // light0
            surface: rgb(0xEBDBB2, White),              // light1
            text: rgb(0x3C3836, Black),                 // dark1
            subtext: rgb(0x7C6F64, DarkGray),           // dark4
            muted: rgb(0x928374, DarkGray),             // gray
            accent: rgb(0xAF3A03, Yellow).indexed(130), // faded orange
            caret: rgb(0xB57614, Yellow),               // faded yellow
            error: rgb(0x9D0006, Red),                  // faded red
            extra: rgb(0xCC241D, Magenta),              // neutral red
            pace: rgb(0x427B58, Cyan),                  // faded aqua
        },
    },
];

//! Render every shipped theme's typing screen through the real UI into one SVG.
//! Run with 24-bit color selected, for example:
//! `COLORTERM=truecolor cargo run --example theme_gallery > docs/images/themes.svg`
use clack::{
    engine::{Action, Engine, TestSpec},
    ui::{self, Appearance, ColorDepth, ColorPolicy, Status, ThemeSpec, Variant, Viewport},
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    style::{Color, Modifier},
};
use std::fmt::Write;

const COLUMNS: u16 = 62;
const ROWS: u16 = 12;
const CELL: (f32, f32) = (8.4, 18.0);
const GAP: f32 = 16.0;
const LABEL: f32 = 22.0;
const PER_ROW: usize = 3;

fn main() {
    assert_eq!(
        ColorDepth::detect(ColorPolicy::Always),
        ColorDepth::TrueColor,
        "set COLORTERM=truecolor so the gallery shows authored colors"
    );
    let panel = (
        f32::from(COLUMNS) * CELL.0,
        f32::from(ROWS) * CELL.1 + LABEL,
    );
    let specs = ThemeSpec::all();
    let rows = specs.len().div_ceil(PER_ROW);
    let size = (
        PER_ROW as f32 * (panel.0 + GAP) + GAP,
        rows as f32 * (panel.1 + GAP) + GAP,
    );
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.0}\" height=\"{:.0}\" \
         font-family=\"ui-monospace, SFMono-Regular, Menlo, Consolas, monospace\" font-size=\"14\">\n\
         <rect width=\"100%\" height=\"100%\" fill=\"#8a8a8a\"/>\n",
        size.0, size.1
    );
    for (index, spec) in specs.iter().enumerate() {
        let x = GAP + (index % PER_ROW) as f32 * (panel.0 + GAP);
        let y = GAP + (index / PER_ROW) as f32 * (panel.1 + GAP);
        let _ = writeln!(
            svg,
            "<text x=\"{x:.1}\" y=\"{:.1}\" fill=\"#111\" font-weight=\"bold\">{} \
             <tspan font-weight=\"normal\">· {}</tspan></text>",
            y + 15.0,
            spec.name,
            escape(spec.label)
        );
        let terminal = spec.variant == Variant::Terminal;
        draw(&mut svg, &screen(spec.name), (x, y + LABEL), terminal);
    }
    svg.push_str("</svg>\n");
    print!("{svg}");
}

/// A test in progress: typed words, a wrong letter, extra letters, the caret.
fn screen(theme: &str) -> Buffer {
    let appearance = Appearance {
        theme: theme.into(),
        color: ColorPolicy::Always,
        ..Appearance::default()
    };
    let status = Status {
        wpm: true,
        accuracy: true,
        ..Status::default()
    };
    let mut engine = Engine::new(
        TestSpec::default(),
        "the world can change when small things become part of the way we work \
         and think about what comes next in a place we know so well that every day",
    )
    .expect("valid sample");
    engine.apply(
        Action::Text("the world can chamge when smallxx thi"),
        1_000_000,
    );
    let mut terminal = Terminal::new(TestBackend::new(COLUMNS, ROWS)).expect("test backend");
    let mut viewport = Viewport::default();
    terminal
        .draw(|frame| {
            ui::render(
                frame,
                &engine,
                &appearance,
                &status,
                &mut viewport,
                7_000_000,
                None,
            );
        })
        .expect("frame");
    terminal.backend().buffer().clone()
}

fn draw(svg: &mut String, buffer: &Buffer, origin: (f32, f32), terminal: bool) {
    // The terminal theme inherits colors; show it on a typical dark terminal.
    let defaults = if terminal {
        ("#d4d4d4", "#1e1e1e")
    } else {
        ("#ffffff", "#000000")
    };
    let width = usize::from(buffer.area.width);
    for (row, cells) in buffer.content().chunks(width).enumerate() {
        let y = origin.1 + row as f32 * CELL.1;
        let styled: Vec<(String, String, Modifier, &str)> = cells
            .iter()
            .map(|cell| {
                let mut fg = color(cell.fg).unwrap_or_else(|| defaults.0.into());
                let mut bg = color(cell.bg).unwrap_or_else(|| defaults.1.into());
                if cell.modifier.contains(Modifier::REVERSED) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                (fg, bg, cell.modifier, cell.symbol())
            })
            .collect();
        // One rectangle per background run and one text element per style run.
        let mut start = 0;
        while start < styled.len() {
            let end = (start..styled.len())
                .find(|&index| styled[index].1 != styled[start].1)
                .unwrap_or(styled.len());
            let _ = writeln!(
                svg,
                "<rect x=\"{:.1}\" y=\"{y:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{}\"/>",
                origin.0 + start as f32 * CELL.0,
                (end - start) as f32 * CELL.0,
                CELL.1,
                styled[start].1
            );
            start = end;
        }
        let mut start = 0;
        while start < styled.len() {
            let (fg, _, modifier, _) = &styled[start];
            let end = (start..styled.len())
                .find(|&index| styled[index].0 != *fg || styled[index].2 != *modifier)
                .unwrap_or(styled.len());
            let run = &styled[start..end];
            if run
                .iter()
                .any(|(_, _, _, symbol)| !symbol.trim().is_empty())
            {
                let positions: Vec<String> = (start..end)
                    .map(|column| format!("{:.1}", origin.0 + column as f32 * CELL.0))
                    .collect();
                let text: String = run.iter().map(|(_, _, _, symbol)| escape(symbol)).collect();
                let mut attributes = String::new();
                if modifier.contains(Modifier::BOLD) {
                    attributes.push_str(" font-weight=\"bold\"");
                }
                if modifier.contains(Modifier::UNDERLINED) {
                    attributes.push_str(" text-decoration=\"underline\"");
                }
                if modifier.contains(Modifier::DIM) {
                    attributes.push_str(" opacity=\"0.6\"");
                }
                let _ = writeln!(
                    svg,
                    "<text x=\"{}\" y=\"{:.1}\" fill=\"{fg}\" xml:space=\"preserve\"{attributes}>{text}</text>",
                    positions.join(" "),
                    y + 13.5
                );
            }
            start = end;
        }
    }
}

fn color(color: Color) -> Option<String> {
    match color {
        Color::Rgb(..) | Color::Indexed(_) => Some(rgb(color)),
        _ => hex(color).map(str::to_owned),
    }
}
fn rgb(color: Color) -> String {
    let [r, g, b] = match color {
        Color::Rgb(r, g, b) => [r, g, b],
        Color::Indexed(index @ 16..=231) => {
            let levels = [0, 95, 135, 175, 215, 255];
            let index = usize::from(index - 16);
            [levels[index / 36], levels[index / 6 % 6], levels[index % 6]]
        }
        Color::Indexed(index @ 232..=255) => [8 + 10 * (index - 232); 3],
        _ => [0; 3],
    };
    format!("#{r:02x}{g:02x}{b:02x}")
}
/// Terminal-palette colors as a common default palette displays them.
fn hex(color: Color) -> Option<&'static str> {
    Some(match color {
        Color::Black => "#000000",
        Color::Red => "#cd3131",
        Color::Green => "#0dbc79",
        Color::Yellow => "#e5e510",
        Color::Blue => "#2472c8",
        Color::Magenta => "#bc3fbc",
        Color::Cyan => "#11a8cd",
        Color::Gray => "#e5e5e5",
        Color::DarkGray => "#767676",
        Color::LightRed => "#f14c4c",
        Color::LightGreen => "#23d18b",
        Color::LightYellow => "#f5f543",
        Color::LightBlue => "#3b8eea",
        Color::LightMagenta => "#d670d6",
        Color::LightCyan => "#29b8db",
        Color::White => "#ffffff",
        _ => return None,
    })
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

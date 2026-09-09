pub mod history;
pub(crate) mod live;
pub mod palette;
pub mod panel;
mod preferences;
pub mod review;
mod theme;
mod viewport;
pub use preferences::*;
pub use theme::*;
pub use viewport::*;

use crate::engine::{Engine, Integrity, Metrics, Mode, Outcome, ResultSnapshot, State, TestSpec};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Paragraph, Wrap},
};

pub fn metric(value: Option<f64>, precision: usize) -> String {
    value
        .filter(|value| value.is_finite())
        .map_or_else(|| "—".into(), |value| format!("{value:.precision$}"))
}
fn live_speed(value: Option<f64>) -> String {
    let value = metric(value, 0);
    if value.len() > 5 && value != "—" {
        "9999+".into()
    } else {
        value
    }
}
/// A standalone combining unit occupies a visible diagnostic cell instead of
/// attaching invisibly to an unrelated terminal cell. Scoring remains unchanged.
pub fn display_unit(text: &str, ascii: bool) -> std::borrow::Cow<'_, str> {
    match text {
        "\n" => std::borrow::Cow::Borrowed(if ascii { "<" } else { "↵" }),
        "\t" => std::borrow::Cow::Borrowed(if ascii { ">" } else { "→" }),
        "" => std::borrow::Cow::Borrowed(" "),
        _ if unicode_width::UnicodeWidthStr::width(text) == 0 => {
            if ascii {
                std::borrow::Cow::Borrowed("?")
            } else {
                std::borrow::Cow::Owned(format!("◌{text}"))
            }
        }
        _ => std::borrow::Cow::Borrowed(text),
    }
}
pub fn line(frame: &mut Frame, x: u16, y: u16, width: u16, text: impl Into<String>, style: Style) {
    if y < frame.area().height && x < frame.area().width {
        frame.render_widget(
            Paragraph::new(text.into()).style(style),
            Rect::new(x, y, width.min(frame.area().width - x), 1),
        );
    }
}
pub fn message(frame: &mut Frame, text: &str, style: Style) {
    let area = frame.area();
    frame.render_widget(
        Paragraph::new(text.to_owned())
            .style(style)
            .wrap(Wrap { trim: true }),
        Rect::new(
            2,
            area.height / 2,
            area.width.saturating_sub(4),
            area.height.saturating_sub(area.height / 2),
        ),
    );
}
pub fn render(
    frame: &mut Frame,
    engine: &Engine,
    appearance: &Appearance,
    status: &Status,
    viewport: &mut Viewport,
    elapsed_us: u64,
    notice: Option<&str>,
) -> Option<(u16, u16)> {
    let theme = Theme::from_preferences(appearance, ColorDepth::detect(appearance.color));
    let area = frame.area();
    frame.buffer_mut().set_style(area, theme.background);
    let Some(geometry) = Geometry::new(area, appearance) else {
        message(
            frame,
            "A terminal of at least 40 × 10 is needed. Resize to continue; Ctrl-C quits.",
            theme.muted,
        );
        return None;
    };
    if engine.state() == State::Results {
        render_results(frame, ResultView::from(engine), geometry, theme, notice);
        return None;
    }
    let ready = engine.state() == State::Ready;
    if appearance.focus != Focus::Always && (ready || appearance.focus == Focus::Off) {
        let mode = match engine.spec().mode {
            Mode::Time => format!("time {}", engine.spec().seconds),
            Mode::Words => format!("words {}", engine.spec().words),
            Mode::Quote => "quote".into(),
            Mode::Custom => "custom".into(),
            Mode::Code => "code · exact".into(),
            Mode::Zen => "zen".into(),
        };
        if ready || !geometry.compact {
            line(
                frame,
                geometry.text.x,
                if ready {
                    geometry.status_y
                } else {
                    geometry.status_y.saturating_sub(1)
                },
                geometry.text.width,
                format!("{mode}  ·  {}", engine.spec().source_id.replace('_', " ")),
                theme.muted,
            );
        }
        line(
            frame,
            geometry.text.x,
            geometry.hint_y,
            geometry.text.width,
            if ready {
                "start typing                 esc commands"
            } else {
                "esc commands                 ctrl-r new sample"
            },
            theme.muted,
        );
    }
    if !ready && appearance.focus != Focus::Always {
        let mut labels = Vec::with_capacity(3);
        if status.progress {
            labels.push(match engine.spec().mode {
                Mode::Time => format!(
                    "{:<8}",
                    format!(
                        "{}s",
                        (u64::from(engine.spec().seconds) * 1_000_000)
                            .saturating_sub(elapsed_us)
                            .div_ceil(1_000_000)
                    )
                ),
                Mode::Words => format!(
                    "{:<12}",
                    format!("{}/{}", engine.current_token(), engine.spec().words)
                ),
                _ => format!("{:<8}", format!("{}s", elapsed_us / 1_000_000)),
            });
        }
        let metrics = viewport.live.preview(engine, elapsed_us).metrics;
        if status.wpm {
            let (value, label) = if engine.spec().mode == Mode::Zen {
                if status.speed_unit == SpeedUnit::Cpm {
                    (metrics.raw_wpm.map(|speed| speed * 5.0), "cpm")
                } else {
                    (metrics.raw_wpm, "wpm")
                }
            } else if status.speed_unit == SpeedUnit::Cpm {
                (metrics.cpm, "cpm")
            } else {
                (metrics.wpm, "wpm")
            };
            labels.push(format!("{:>5} {label}   ", live_speed(value)));
        }
        if status.accuracy && engine.spec().mode != Mode::Zen {
            labels.push(format!("{:>5}%", metric(metrics.accuracy, 1)));
        }
        line(
            frame,
            geometry.text.x,
            geometry.status_y,
            geometry.text.width,
            labels.join(""),
            theme.muted,
        );
    }
    if ready
        && let Some(notice) = notice
        && !geometry.compact
        && appearance.focus != Focus::Always
    {
        line(
            frame,
            geometry.text.x,
            geometry.hint_y + 1,
            geometry.text.width,
            notice,
            theme.muted,
        );
    }
    let live = viewport.live.update(engine, elapsed_us);
    viewport.geometry = Some(geometry);
    let cursor = if engine.spec().mode == Mode::Zen {
        render_zen(frame, engine, geometry, theme, appearance)
    } else {
        viewport.sync(engine, geometry.text.width, appearance.tab_stop);
        let logical = viewport.select_anchor(engine, geometry);
        frame.render_widget(
            TypingWidget {
                engine,
                viewport,
                geometry,
                theme,
                appearance,
                pace: live.pace,
            },
            geometry.text,
        );
        viewport.screen_cursor(logical, geometry)
    };
    if let Some((x, y)) = cursor {
        frame.set_cursor_position((x, y));
    }
    cursor
}
/// Show the final adapter verdict, including an overload discovered when the
/// reader closes the completed epoch. A later save ACK must not erase it.
pub fn render_snapshot(
    frame: &mut Frame,
    result: &ResultSnapshot,
    appearance: &Appearance,
    notice: Option<&str>,
) {
    let theme = Theme::from_preferences(appearance, ColorDepth::detect(appearance.color));
    let area = frame.area();
    frame.buffer_mut().set_style(area, theme.background);
    let Some(geometry) = Geometry::new(area, appearance) else {
        message(frame, "Resize to at least 40 × 10.", theme.muted);
        return;
    };
    render_results(
        frame,
        ResultView {
            spec: &result.spec,
            metrics: result.metrics,
            elapsed_us: result.elapsed_us,
            outcome: result.outcome,
            reason: result.reason.as_deref(),
            integrity: &result.integrity,
        },
        geometry,
        theme,
        notice,
    );
}
struct ResultView<'a> {
    spec: &'a TestSpec,
    metrics: Metrics,
    elapsed_us: u64,
    outcome: Outcome,
    reason: Option<&'a str>,
    integrity: &'a Integrity,
}
impl<'a> From<&'a Engine> for ResultView<'a> {
    fn from(engine: &'a Engine) -> Self {
        Self {
            spec: engine.spec(),
            metrics: engine.metrics(),
            elapsed_us: engine.elapsed_us(),
            outcome: engine.outcome(),
            reason: engine.reason(),
            integrity: engine.integrity(),
        }
    }
}
fn render_results(
    frame: &mut Frame,
    result: ResultView<'_>,
    geometry: Geometry,
    theme: Theme,
    notice: Option<&str>,
) {
    let metrics = result.metrics;
    let x = geometry.text.x;
    let y = geometry.text.y.saturating_sub(1);
    let width = geometry.text.width;
    if result.spec.mode == Mode::Zen {
        line(
            frame,
            x,
            y,
            width,
            format!("{} wpm output", metric(metrics.raw_wpm, 0)),
            theme.correct,
        );
    } else {
        line(
            frame,
            x,
            y,
            width,
            format!(
                "{} wpm       {}% accuracy",
                metric(metrics.wpm, 0),
                metric(metrics.accuracy, 1)
            ),
            theme.correct,
        );
    }
    line(
        frame,
        x,
        y + 1,
        width,
        format!(
            "raw {}    {:.2}s    {}",
            metric(metrics.raw_wpm, 0),
            result.elapsed_us as f64 / 1_000_000.0,
            result.spec.source_id.replace('_', " ")
        ),
        theme.muted,
    );
    let mode = match result.spec.mode {
        Mode::Time => format!("time {}s", result.spec.seconds),
        Mode::Words => format!("words {}", result.spec.words),
        Mode::Quote => "quote".into(),
        Mode::Custom => format!("custom {:?}", result.spec.policy).to_lowercase(),
        Mode::Code => "code exact".into(),
        Mode::Zen => "zen".into(),
    };
    let profile = result.spec.profile_key();
    line(
        frame,
        x,
        y + 2,
        width,
        format!("{mode} · #{}", &profile[..8]),
        theme.muted,
    );
    let mut labels = Vec::new();
    if result.outcome != crate::engine::Outcome::Complete {
        labels.push(format!("{:?}", result.outcome).to_lowercase());
    }
    // Saving failures are actionable even at 40×10. Put their short verdict
    // beside the outcome, before optional explanation or source labels.
    if let Some(notice) = notice.filter(|notice| notice.contains("unsaved")) {
        labels.push(notice.split('·').next().unwrap_or(notice).trim().to_owned());
    }
    if result.spec.repeated {
        labels.push("repeated practice".into());
    } else if result.spec.practice_reason.is_some()
        || result.spec.explicit_seed
        || result.integrity.paste_attempted
        || result.spec.auto_indent
        || result.spec.pace_wpm.is_some()
    {
        labels.push("practice".into());
    }
    let mut primary = String::new();
    let mut secondary = String::new();
    for label in labels {
        let candidate = if primary.is_empty() {
            label.clone()
        } else {
            format!("{primary} · {label}")
        };
        if unicode_width::UnicodeWidthStr::width(candidate.as_str()) <= usize::from(width) {
            primary = candidate;
        } else {
            if !secondary.is_empty() {
                secondary.push_str(" · ");
            }
            secondary.push_str(&label);
        }
    }
    if let Some(explanation) = result
        .reason
        .or_else(|| notice.filter(|notice| !notice.contains("unsaved")))
    {
        if !secondary.is_empty() {
            secondary.push_str(" · ");
        }
        secondary.push_str(explanation);
    }
    line(frame, x, y + 3, width, primary, theme.accent);
    line(frame, x, y + 4, width, secondary, theme.accent);
    line(
        frame,
        x,
        (y + 5).min(frame.area().height - 1),
        width,
        if geometry.compact {
            "enter next   esc commands"
        } else {
            "enter next   f2 repeat   f3 practice   f4 details"
        },
        theme.muted,
    );
}
fn render_zen(
    frame: &mut Frame,
    engine: &Engine,
    geometry: Geometry,
    theme: Theme,
    appearance: &Appearance,
) -> Option<(u16, u16)> {
    let mut cells = Vec::with_capacity(engine.zen_window().len());
    let mut row = 0usize;
    let mut column = 0u16;
    for entry in engine.zen_window() {
        let text = entry.unit.as_str();
        let tab_stop = u16::from(appearance.tab_stop.clamp(1, 16));
        let mut width = if text == "\t" {
            tab_stop - column % tab_stop
        } else {
            entry.unit.width().max(1) as u16
        };
        if column + width > geometry.text.width {
            row += 1;
            column = 0;
            if text == "\t" {
                width = tab_stop.min(geometry.text.width);
            }
        }
        cells.push((row, column, text, width));
        column += width;
        if text == "\n" || column >= geometry.text.width {
            row += 1;
            column = 0;
        }
    }
    let anchor = row.saturating_sub(geometry.lines - 1);
    for (logical, x, text, width) in cells.into_iter().filter(|(row, _, _, _)| *row >= anchor) {
        let y = geometry.text.y + (logical - anchor) as u16 * (geometry.spacing + 1);
        frame.buffer_mut().set_stringn(
            geometry.text.x + x,
            y,
            display_unit(text, appearance.ascii_markers),
            usize::from(width),
            theme.correct,
        );
    }
    let cursor = (
        geometry.text.x + column,
        geometry.text.y + (row - anchor) as u16 * (geometry.spacing + 1),
    );
    frame.buffer_mut()[cursor].set_style(if appearance.caret == Caret::Block {
        theme.caret.add_modifier(ratatui::style::Modifier::REVERSED)
    } else {
        theme.caret
    });
    Some(cursor)
}

//! Bounded, on-demand review of an immutable result. No alternate scoring path.
use super::{Appearance, ColorDepth, Geometry, Theme, line, metric};
use crate::engine::{Mode, ResultSnapshot};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};
use std::cell::RefCell;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Summary,
    Text,
}
pub struct Review {
    pub result: ResultSnapshot,
    pub tab: Tab,
    pub scroll: usize,
    pub horizontal: usize,
    pub original_required: bool,
    pub entered_available: bool,
    original_text: Option<String>,
    source_layout: RefCell<Option<SourceLayout>>,
    pub notice: Option<String>,
}
impl Review {
    pub fn new(result: ResultSnapshot) -> Self {
        let original_required = result.words.is_empty() && result.spec.mode != Mode::Zen;
        Self {
            result,
            tab: Tab::Summary,
            scroll: 0,
            horizontal: 0,
            original_required,
            entered_available: true,
            original_text: None,
            source_layout: RefCell::new(None),
            notice: None,
        }
    }
    pub fn toggle(&mut self) {
        self.tab = if self.tab == Tab::Summary {
            Tab::Text
        } else {
            Tab::Summary
        };
        self.scroll = 0;
        self.horizontal = 0;
    }
    pub fn set_original_text(&mut self, text: Option<String>) {
        self.original_text = text;
        self.source_layout = RefCell::new(None);
        self.scroll = 0;
    }
    pub fn move_by(&mut self, delta: isize) {
        self.horizontal = 0;
        self.scroll = self
            .scroll
            .saturating_add_signed(delta)
            .min(if self.tab == Tab::Text {
                self.source_layout
                    .borrow()
                    .as_ref()
                    .map_or_else(|| self.result.words.len(), |layout| layout.rows.len())
                    .saturating_sub(1)
            } else {
                self.summary_rows().saturating_sub(1)
            });
    }
    fn summary_rows(&self) -> usize {
        if self.result.spec.mode == Mode::Zen {
            return 6;
        }
        15 + usize::from(self.result.reason.is_some())
            + usize::from(self.result.spec.practice_reason.is_some())
            + usize::from(self.result.integrity.paste_attempted)
            + usize::from(self.result.integrity.focus_lost)
    }
    fn selected_units(&self) -> usize {
        self.result.words.get(self.scroll).map_or(0, |word| {
            word.token
                .graphemes(true)
                .count()
                .max(word.entered.graphemes(true).count())
        })
    }
    pub fn pan(&mut self, delta: isize) {
        if self.tab == Tab::Text && self.original_text.is_none() {
            self.horizontal = self
                .horizontal
                .saturating_add_signed(delta)
                .min(self.selected_units().saturating_sub(1));
        }
    }
    pub fn show_tail(&mut self) {
        if self.tab == Tab::Text && self.original_text.is_none() {
            self.horizontal = self.selected_units().saturating_sub(8);
        } else {
            self.move_by(isize::MAX);
        }
    }
}
struct SourceLayout {
    width: u16,
    ascii: bool,
    rows: Vec<String>,
}
impl SourceLayout {
    fn prepare(text: &str, width: u16, ascii: bool) -> Self {
        let mut rows = Vec::new();
        let mut row = String::new();
        let mut cells = 0;
        for part in text.split_inclusive(char::is_whitespace) {
            let part_cells: usize = part
                .graphemes(true)
                .filter(|unit| *unit != "\n")
                .map(|unit| super::display_unit(unit, ascii).width())
                .sum();
            if part_cells <= usize::from(width)
                && cells + part_cells > usize::from(width)
                && !row.is_empty()
            {
                rows.push(std::mem::take(&mut row));
                cells = 0;
            }
            for unit in part.graphemes(true) {
                if unit == "\n" {
                    rows.push(std::mem::take(&mut row));
                    cells = 0;
                    continue;
                }
                let visible = super::display_unit(unit, ascii);
                let advance = visible.width();
                if cells + advance > usize::from(width) && !row.is_empty() {
                    rows.push(std::mem::take(&mut row));
                    cells = 0;
                }
                row.push_str(&visible);
                cells += advance;
            }
        }
        if !row.is_empty() || rows.is_empty() {
            rows.push(row);
        }
        Self { width, ascii, rows }
    }
}

pub fn render(frame: &mut Frame, review: &Review, appearance: &Appearance) {
    let theme = Theme::from_preferences(appearance, ColorDepth::detect(appearance.color));
    let area = frame.area();
    frame.buffer_mut().set_style(area, theme.background);
    let Some(geometry) = Geometry::new(area, appearance) else {
        super::message(frame, "Resize to at least 40 × 10.", theme.muted);
        return;
    };
    let (x, width) = (geometry.text.x, geometry.text.width);
    let result = &review.result;
    line(
        frame,
        x,
        1,
        width,
        format!(
            "Review · {} · {:?}",
            result.spec.source_id.replace('_', " "),
            result.outcome
        )
        .to_lowercase(),
        theme.correct,
    );
    line(
        frame,
        x,
        area.height - 2,
        width,
        review.notice.as_deref().unwrap_or(
            if review.tab == Tab::Text
                && review.selected_units() > usize::from(width.saturating_sub(4))
            {
                "↑↓ words  ←→ text  end tail  esc results"
            } else {
                "tab summary/text  ↑↓ scroll  esc results"
            },
        ),
        theme.muted,
    );
    let height = usize::from(area.height.saturating_sub(5));
    if review.tab == Tab::Text {
        if review.original_required {
            line(
                frame,
                x,
                3,
                width,
                "Original text was not retained.",
                theme.muted,
            );
            line(
                frame,
                x,
                4,
                width,
                "Press o to provide the original file:",
                theme.muted,
            );
            line(frame, x, 5, width, &result.spec.content_hash, theme.muted);
            return;
        }
        if let Some(original) = &review.original_text {
            let header = if result.spec.mode == Mode::Zen {
                "Stored output (bounded window when noted)"
            } else {
                "Source target · per-token input unavailable"
            };
            line(frame, x, 3, width, header, theme.muted);
            // Source display is independent of scoring. Do not reconstruct an input
            // transcript from aggregate counts or a correct-token flag.
            let mut cache = review.source_layout.borrow_mut();
            if cache.as_ref().is_none_or(|layout| {
                layout.width != width || layout.ascii != appearance.ascii_markers
            }) {
                *cache = Some(SourceLayout::prepare(
                    original,
                    width,
                    appearance.ascii_markers,
                ));
            }
            let layout = cache.as_ref().expect("prepared source layout");
            let lines: Vec<Line> = layout
                .rows
                .iter()
                .skip(review.scroll.min(layout.rows.len().saturating_sub(1)))
                .take(height.saturating_sub(1))
                .map(|text| Line::styled(text.as_str(), theme.pending))
                .collect();
            frame.render_widget(
                Paragraph::new(lines),
                Rect::new(x, 4, width, height.saturating_sub(1) as u16),
            );
            return;
        }
        if result.spec.mode == Mode::Zen {
            line(
                frame,
                x,
                3,
                width,
                "Zen has no expected target or accuracy score.",
                theme.muted,
            );
            return;
        }
        for (index, word) in result
            .words
            .iter()
            .enumerate()
            .skip(review.scroll)
            .take(height / 3)
        {
            let y = 3 + ((index - review.scroll) * 3) as u16;
            line(
                frame,
                x,
                y,
                width,
                format!(
                    "{} · {} attempts, {} mistaken · {}",
                    index + 1,
                    word.attempts,
                    word.errors,
                    if word.corrected {
                        "corrected while typing"
                    } else if word.completed_correct {
                        "completed correctly"
                    } else {
                        "final output differs or is unfinished"
                    }
                ),
                theme.muted,
            );
            line(
                frame,
                x,
                y + 1,
                width,
                format!(
                    "exp {}",
                    visible_text_from(&word.token, review.horizontal, appearance.ascii_markers)
                ),
                theme.pending,
            );
            let entered = if !review.entered_available {
                "[entered text was not retained]".into()
            } else if word.entered.is_empty() {
                "[no retained input]".into()
            } else {
                visible_text_from(&word.entered, review.horizontal, appearance.ascii_markers)
            };
            line(
                frame,
                x,
                y + 2,
                width,
                format!("got {entered}"),
                if word.completed_correct {
                    theme.correct
                } else {
                    theme.incorrect
                },
            );
        }
        return;
    }
    let counts = result.counts;
    let metrics = result.metrics;
    let chart_width = usize::from(width.saturating_sub(8));
    if result.spec.mode == Mode::Zen {
        let rows = vec![
            format!(
                "{} wpm output · {:.2}s",
                metric(metrics.raw_wpm, 1),
                result.elapsed_us as f64 / 1_000_000.0
            ),
            format!(
                "raw {}",
                sparkline(
                    result
                        .samples
                        .iter()
                        .map(|sample| sample.metrics.raw_wpm.unwrap_or(0.0)),
                    chart_width,
                    appearance.ascii_markers
                )
            ),
            format!(
                "{} typed units · {} retained",
                counts.attempts_total, counts.retained_units
            ),
            format!(
                "{} deletions · consistency {}",
                counts.deletion_count,
                metric(metrics.consistency, 1)
            ),
            "Accuracy and target-error metrics are unavailable in zen.".into(),
            format!("{} bounded chart buckets", result.samples.len()),
        ];
        let lines: Vec<Line> = rows
            .into_iter()
            .skip(review.scroll.min(5))
            .take(height)
            .map(|text| Line::from(Span::styled(text, theme.correct)))
            .collect();
        frame.render_widget(Paragraph::new(lines), Rect::new(x, 3, width, height as u16));
        return;
    }
    let mut rows: Vec<(String, Style)> = vec![
        (
            if result.spec.mode == Mode::Zen {
                format!(
                    "{} wpm output · {:.2}s",
                    metric(metrics.raw_wpm, 1),
                    result.elapsed_us as f64 / 1_000_000.0
                )
            } else {
                format!(
                    "{} wpm · raw {} · {}% accuracy",
                    metric(metrics.wpm, 1),
                    metric(metrics.raw_wpm, 1),
                    metric(metrics.accuracy, 1)
                )
            },
            theme.correct,
        ),
        (
            format!(
                "{:.2}s · {} cpm · {}",
                result.elapsed_us as f64 / 1_000_000.0,
                metric(metrics.cpm, 1),
                if result.personal_best_eligible {
                    "eligible for local records"
                } else {
                    "excluded from standard records"
                }
            ),
            theme.muted,
        ),
        ("".into(), theme.muted),
        (
            format!(
                "wpm  {}",
                sparkline(
                    result
                        .samples
                        .iter()
                        .map(|sample| sample.metrics.wpm.unwrap_or(0.0)),
                    chart_width,
                    appearance.ascii_markers
                )
            ),
            theme.correct,
        ),
        (
            format!(
                "raw  {}",
                sparkline(
                    result
                        .samples
                        .iter()
                        .map(|sample| sample.metrics.raw_wpm.unwrap_or(0.0)),
                    chart_width,
                    appearance.ascii_markers
                )
            ),
            theme.muted,
        ),
        (
            format!(
                "err  {}",
                sparkline(
                    result.samples.iter().map(|sample| sample.errors as f64),
                    chart_width,
                    appearance.ascii_markers
                )
            ),
            theme.incorrect,
        ),
        (
            format!(
                "{} retained buckets · cumulative speed / bucket errors",
                result.samples.len()
            ),
            theme.muted,
        ),
        ("".into(), theme.muted),
        (
            format!(
                "Typing: {} attempts · {} mistaken attempts",
                counts.attempts_total,
                counts.attempts_total - counts.attempts_correct
            ),
            theme.correct,
        ),
        (
            format!(
                "{} deletions · {} retained output units",
                counts.deletion_count, counts.retained_units
            ),
            theme.muted,
        ),
        (
            format!(
                "Final: {} correct · {} incorrect",
                counts.final_correct, counts.final_incorrect
            ),
            theme.correct,
        ),
        (
            format!(
                "{} extra · {} missed · {} credited units",
                counts.final_extra, counts.final_missed, counts.credited_units
            ),
            theme.muted,
        ),
        (
            format!("Consistency {} / 100", metric(metrics.consistency, 1)),
            theme.muted,
        ),
        ("".into(), theme.muted),
    ];
    let missed: Vec<&str> = result
        .words
        .iter()
        .filter(|word| word.errors > 0 || (!word.completed_correct && word.attempts > 0))
        .take(30)
        .map(|word| word.token.trim())
        .filter(|word| !word.is_empty())
        .collect();
    rows.push((
        if missed.is_empty() {
            "No retained mistaken-token labels".into()
        } else {
            format!("Mistaken tokens: {}", missed.join(", "))
        },
        theme.muted,
    ));
    if let Some(reason) = &result.reason {
        rows.push((format!("Reason: {reason}"), theme.accent));
    }
    if let Some(reason) = &result.spec.practice_reason {
        rows.push((format!("Practice: {reason}"), theme.accent));
    }
    if result.integrity.paste_attempted {
        rows.push(("Paste attempted · practice only".into(), theme.accent));
    }
    if result.integrity.focus_lost {
        rows.push(("Focus loss reported by the terminal".into(), theme.muted));
    }
    let lines: Vec<Line> = rows
        .into_iter()
        .skip(review.scroll.min(review.summary_rows().saturating_sub(1)))
        .take(height)
        .map(|(text, style)| Line::from(Span::styled(text, style)))
        .collect();
    frame.render_widget(Paragraph::new(lines), Rect::new(x, 3, width, height as u16));
}
fn visible_text_from(text: &str, offset: usize, ascii: bool) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    let mut visible: String = text
        .graphemes(true)
        .skip(offset)
        .take(512)
        .map(|unit| super::display_unit(unit, ascii))
        .collect();
    if text
        .graphemes(true)
        .nth(offset.saturating_add(512))
        .is_some()
    {
        visible.push_str(if ascii { "..." } else { "…" });
    }
    visible
}
pub fn sparkline(values: impl Iterator<Item = f64>, width: usize, ascii: bool) -> String {
    let values: Vec<f64> = values
        .map(|value| {
            if value.is_finite() {
                value.max(0.0)
            } else {
                0.0
            }
        })
        .collect();
    if width == 0 {
        return String::new();
    }
    if values.is_empty() {
        return if ascii { "-" } else { "—" }.into();
    }
    let count = values.len().min(width);
    let mut bins = vec![0.0_f64; count];
    for (index, value) in values.iter().enumerate() {
        let bin = index * count / values.len();
        bins[bin] = bins[bin].max(*value);
    }
    let max = bins.iter().copied().fold(0.0_f64, f64::max);
    let glyphs: Vec<char> = if ascii {
        " .:-=+*#"
    } else {
        "▁▂▃▄▅▆▇█"
    }
    .chars()
    .collect();
    bins.into_iter()
        .map(|value| {
            glyphs[if max > 0.0 {
                (value / max * 7.0).round() as usize
            } else {
                0
            }]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn charts_are_bounded_and_ignore_nonfinite_inputs() {
        assert_eq!(
            sparkline([0.0, f64::NAN, 4.0, 8.0].into_iter(), 2, false),
            "▁█"
        );
        assert_eq!(sparkline(std::iter::empty(), 20, false), "—");
        assert_eq!(sparkline([0.0; 40].into_iter(), 7, true).len(), 7);
    }
}

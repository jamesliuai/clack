use super::{Appearance, ColorDepth, Geometry, Theme, line, metric, review::sparkline};
use crate::storage::{Filter, HistoryPage, Statistics};
use ratatui::Frame;
pub struct History {
    pub filter: Filter,
    pub page: HistoryPage,
    pub selected: usize,
    pub offset: u64,
    pub statistics: Option<Statistics>,
    pub loading: bool,
    pub message: Option<String>,
    pub return_ready: bool,
    pub page_request: Option<u64>,
    pub stats_request: Option<u64>,
    pub review_request: Option<u64>,
}
impl History {
    pub fn new(profile: String, return_ready: bool) -> Self {
        Self {
            filter: Filter::current(profile),
            page: HistoryPage::default(),
            selected: 0,
            offset: 0,
            statistics: None,
            loading: true,
            message: None,
            return_ready,
            page_request: None,
            stats_request: None,
            review_request: None,
        }
    }
    pub fn move_by(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.page.results.len().saturating_sub(1));
    }
}
pub fn render(frame: &mut Frame, history: &History, appearance: &Appearance) {
    let theme = Theme::from_preferences(appearance, ColorDepth::detect(appearance.color));
    let area = frame.area();
    frame.buffer_mut().set_style(area, theme.background);
    let Some(geometry) = Geometry::new(area, appearance) else {
        super::message(frame, "Resize to at least 40 × 10.", theme.muted);
        return;
    };
    let (x, width) = (geometry.text.x, geometry.text.width);
    line(
        frame,
        x,
        1,
        width,
        format!(
            "History · {}",
            if history.filter.profile_key.is_some() {
                "matching profile"
            } else {
                "all profiles"
            }
        ),
        theme.correct,
    );
    if let Some(stats) = &history.statistics {
        line(
            frame,
            x,
            2,
            width,
            format!(
                "{} runs · aggregate {} wpm · {}% accuracy",
                stats.result_count,
                metric(stats.aggregate_wpm, 1),
                metric(stats.aggregate_accuracy, 1)
            ),
            theme.muted,
        );
    }
    let mut filters = Vec::new();
    if let Some(classification) = history.filter.classification {
        filters.push(
            format!("{classification:?}")
                .replace("PasteAttempted", "paste attempted")
                .replace("AssistedCode", "assisted code")
                .to_lowercase(),
        );
    }
    if let Some(outcome) = history.filter.outcome {
        filters.push(format!("{outcome:?}").to_lowercase());
    }
    if let Some(mode) = history.filter.mode {
        filters.push(format!("{mode:?}").to_lowercase());
    }
    if let Some(language) = &history.filter.language {
        filters.push(language.clone());
    }
    if history.filter.from_utc_ms.is_some() || history.filter.to_utc_ms.is_some() {
        filters.push("date range".into());
    }
    if !filters.is_empty() {
        line(frame, x, 3, width, filters.join(" · "), theme.muted);
    }
    let visible = usize::from(area.height.saturating_sub(8)).max(1);
    let first = history.selected.saturating_sub(visible - 1);
    for (offset, entry) in history
        .page
        .results
        .iter()
        .skip(first)
        .take(visible)
        .enumerate()
    {
        let selected = first + offset == history.selected;
        let result = &entry.snapshot;
        let chart = if width >= 60 {
            format!(
                "  {}",
                sparkline(
                    entry.sparkline.iter().map(|value| value.unwrap_or(0.0)),
                    12,
                    appearance.ascii_markers
                )
            )
        } else {
            String::new()
        };
        line(
            frame,
            x,
            4 + offset as u16,
            width,
            format!(
                "{} {} {:>5} {:>5}% {:?}{chart}",
                if selected { ">" } else { " " },
                entry.created_at_utc.get(..10).unwrap_or("unknown"),
                metric(
                    if result.spec.mode == crate::engine::Mode::Zen {
                        result.metrics.raw_wpm
                    } else {
                        result.metrics.wpm
                    },
                    1
                ),
                metric(result.metrics.accuracy, 1),
                result.outcome
            ),
            if selected { theme.accent } else { theme.muted },
        );
    }
    if history.page.results.is_empty() {
        line(
            frame,
            x,
            4,
            width,
            if history.loading {
                "Loading local history…"
            } else {
                "No results match this filter."
            },
            theme.muted,
        );
    }
    if let Some(message) = &history.message {
        line(frame, x, area.height - 3, width, message, theme.accent);
    }
    line(
        frame,
        x,
        area.height - 2,
        width,
        if geometry.compact {
            "enter review  ←→ page  f filter  esc"
        } else {
            "↑↓ select  enter review  ←→ page  f filters  esc close"
        },
        theme.muted,
    );
}

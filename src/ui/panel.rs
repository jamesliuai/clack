use super::{Appearance, ColorDepth, Geometry, Theme, line};
use ratatui::{Frame, layout::Rect, text::Line, widgets::Paragraph};
pub struct Panel {
    pub title: String,
    pub lines: Vec<String>,
    pub scroll: usize,
    pub return_ready: bool,
}
impl Panel {
    pub fn move_by(&mut self, delta: isize) {
        self.scroll = self
            .scroll
            .saturating_add_signed(delta)
            .min(self.lines.len().saturating_sub(1));
    }
}
pub fn render(frame: &mut Frame, panel: &Panel, appearance: &Appearance) {
    let theme = Theme::from_preferences(appearance, ColorDepth::detect(appearance.color));
    let area = frame.area();
    frame.buffer_mut().set_style(area, theme.background);
    let Some(geometry) = Geometry::new(area, appearance) else {
        super::message(frame, "Resize to at least 40 × 10.", theme.muted);
        return;
    };
    let (x, width) = (geometry.text.x, geometry.text.width);
    line(frame, x, 1, width, &panel.title, theme.correct);
    let rows = area.height.saturating_sub(5);
    let lines: Vec<Line> = panel
        .lines
        .iter()
        .skip(panel.scroll)
        .take(rows as usize)
        .map(|text| Line::styled(text.as_str(), theme.muted))
        .collect();
    frame.render_widget(Paragraph::new(lines), Rect::new(x, 3, width, rows));
    line(
        frame,
        x,
        area.height - 2,
        width,
        "↑↓ scroll  pgup/pgdn page  esc close",
        theme.muted,
    );
}

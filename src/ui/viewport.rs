use super::{Appearance, Caret, Theme, Width};
use crate::engine::{Engine, Policy, State};
use ratatui::{buffer::Buffer, layout::Rect, style::Modifier, widgets::Widget};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    pub row: usize,
    pub column: u16,
}
#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    pub text: Rect,
    pub status_y: u16,
    pub hint_y: u16,
    pub lines: usize,
    pub spacing: u16,
    pub compact: bool,
}
impl Geometry {
    pub fn new(area: Rect, appearance: &Appearance) -> Option<Self> {
        if area.width < 40 || area.height < 10 {
            return None;
        }
        let compact = area.width <= 40 || area.height <= 10;
        let available = area.width.saturating_sub(8);
        let width = match appearance.width {
            Width::Auto => 72,
            Width::Cells(value) => value,
        }
        .min(available)
        .max(1);
        let spacing = if compact {
            0
        } else {
            u16::from(appearance.line_spacing.min(1))
        };
        let lines = if compact {
            1
        } else {
            usize::from(appearance.lines.clamp(1, 5))
                .min(usize::from((area.height - 4 + spacing) / (spacing + 1)))
        };
        let height = lines as u16 + (lines as u16 - 1) * spacing;
        let top = match appearance.alignment {
            super::Alignment::Top => 4,
            super::Alignment::Center => {
                ((u32::from(area.height) * 45 / 100) as u16).saturating_sub(height / 2)
            }
        };
        let y = top.clamp(2, area.height.saturating_sub(height + 2));
        Some(Self {
            text: Rect::new(area.x + (area.width - width) / 2, area.y + y, width, height),
            status_y: area.y + y - 2,
            hint_y: area.y + y + height + 1,
            lines,
            spacing,
            compact,
        })
    }
}
#[derive(Debug, Clone, Copy)]
struct Cell {
    token: usize,
    unit: usize,
    separator: bool,
    position: Position,
    width: u16,
}
#[derive(Debug, Clone, Copy)]
struct TokenLayout {
    first: usize,
    before: Position,
    after: Position,
    footprint: u64,
    units: usize,
    display_width: usize,
}
#[derive(Debug, Default)]
pub struct Viewport {
    pub(crate) live: super::live::LiveCache,
    pub(crate) geometry: Option<Geometry>,
    cells: Vec<Cell>,
    tokens: Vec<TokenLayout>,
    width: u16,
    tab_stop: u8,
    last_current: usize,
    layout_version: u64,
    source_tokens: usize,
    partial: bool,
    pub anchor_row: usize,
    pub layout_rebuilds: u64,
}
impl Viewport {
    pub(crate) fn visible_pace(&self, engine: &Engine, units: usize) -> bool {
        let Some(position) = self.pace_position(engine, units) else {
            return false;
        };
        position != self.logical_cursor(engine)
            && self
                .geometry
                .is_some_and(|geometry| self.screen_cursor(position, geometry).is_some())
    }
    pub fn sync(&mut self, engine: &Engine, width: u16, tab_stop: u8) {
        let width = width.max(1);
        let tab_stop = tab_stop.clamp(1, 16);
        if self.width != width
            || self.tab_stop != tab_stop
            || self.source_tokens > engine.tokens().len()
        {
            self.width = width;
            self.tab_stop = tab_stop;
            self.rebuild(engine, 0);
        } else if let Some((index, unit)) = engine.layout_unit_changed_since(self.layout_version)
            && index < self.tokens.len()
        {
            self.rebuild_changed(engine, index, unit);
        } else if self.source_tokens > 0
            && self.source_tokens < engine.tokens().len()
            && self.tokens.len() == self.source_tokens
            && !self.partial
            && engine.spec().policy == Policy::Prose
        {
            // The old final word gains a separator when lookahead is appended.
            self.rebuild(engine, self.source_tokens - 1);
        } else if !self.tokens.is_empty() {
            let start = self
                .last_current
                .min(engine.current_token())
                .min(self.tokens.len() - 1);
            let end = self
                .last_current
                .max(engine.current_token())
                .min(self.tokens.len() - 1);
            for index in start..=end {
                if self.tokens[index].footprint != Self::footprint(engine, index) {
                    self.rebuild(engine, index);
                    break;
                }
            }
        }
        let cursor = self.logical_cursor(engine);
        if self.tokens.is_empty()
            || engine.current_token() >= self.tokens.len()
            || ((self.partial || self.tokens.len() < engine.tokens().len())
                && self
                    .tokens
                    .last()
                    .is_some_and(|token| token.after.row < cursor.row + 6))
        {
            self.extend(engine);
        }
        self.last_current = engine.current_token();
        self.source_tokens = engine.tokens().len();
        self.layout_version = engine.layout_version();
    }
    fn footprint(engine: &Engine, index: usize) -> u64 {
        engine.tokens()[index].layout_revision
    }
    fn rebuild_changed(&mut self, engine: &Engine, index: usize, unit: usize) {
        let layout = self.tokens[index];
        let token = &engine.tokens()[index];
        // A short prose word may move as a whole when its width changes. Exact
        // text and words wider than a row keep their unchanged unit prefix.
        if unit > 0
            && unit <= layout.units
            && (engine.spec().policy == Policy::Exact
                || (layout.display_width > usize::from(self.width)
                    && token.display_width > usize::from(self.width)))
        {
            let previous = self.cells[layout.first + unit - 1];
            let mut position = previous.position;
            position.column += previous.width;
            if position.column >= self.width
                || token
                    .target
                    .get(unit - 1)
                    .is_some_and(|unit| unit.as_str() == "\n")
            {
                position.row += 1;
                position.column = 0;
            }
            self.cells.truncate(layout.first + unit);
            self.tokens.truncate(index);
            self.layout_rebuilds += 1;
            self.append_layout(
                engine,
                index,
                position,
                Some(TokenLayout {
                    after: position,
                    units: unit,
                    ..layout
                }),
            );
        } else {
            self.rebuild(engine, index);
        }
    }
    fn rebuild(&mut self, engine: &Engine, from: usize) {
        self.layout_rebuilds += 1;
        let (cut, position) = self.tokens.get(from).map_or_else(
            || {
                (
                    self.cells.len(),
                    self.tokens
                        .last()
                        .map_or(Position::default(), |token| token.after),
                )
            },
            |token| (token.first, token.before),
        );
        self.cells.truncate(cut);
        self.tokens.truncate(from);
        self.append_layout(engine, from, position, None);
    }
    fn extend(&mut self, engine: &Engine) {
        self.layout_rebuilds += 1;
        let resume = if self.partial {
            self.tokens.pop()
        } else {
            None
        };
        let position = resume.as_ref().map_or_else(
            || {
                self.tokens
                    .last()
                    .map_or(Position::default(), |token| token.after)
            },
            |token| token.after,
        );
        self.append_layout(engine, self.tokens.len(), position, resume);
    }
    fn append_layout(
        &mut self,
        engine: &Engine,
        from: usize,
        mut position: Position,
        mut resume: Option<TokenLayout>,
    ) {
        // Keep a small lookahead beyond the caret. Prefixes remain cached, and
        // an unfinished long token resumes at its last cell instead of being
        // scanned from its beginning on every frame.
        let mut last_row = if engine.current_token() < from {
            self.logical_cursor(engine).row.saturating_add(8)
        } else {
            usize::MAX
        };
        self.partial = false;
        for index in from..engine.tokens().len() {
            let token = &engine.tokens()[index];
            let previous = resume.take();
            let before = previous.as_ref().map_or(position, |token| token.before);
            let first = previous
                .as_ref()
                .map_or(self.cells.len(), |token| token.first);
            let start = previous.as_ref().map_or(0, |token| token.units);
            let count = token.target.len().max(token.entered.len());
            if start == 0 && engine.spec().policy == Policy::Prose {
                let total = token.display_width;
                if total <= usize::from(self.width)
                    && usize::from(position.column) + total > usize::from(self.width)
                {
                    position.row += 1;
                    position.column = 0;
                }
            }
            if index == engine.current_token() && token.entered.len() < start {
                last_row = self.logical_cursor(engine).row.saturating_add(8);
                // The current layout was temporarily removed to resume it.
                if let Some(cell) = self.cells.get(first + token.entered.len()) {
                    last_row = cell.position.row.saturating_add(8);
                }
            }
            let mut units = start;
            for unit in start..count {
                if index == engine.current_token() && unit == token.entered.len() {
                    last_row = position.row.saturating_add(8);
                }
                if position.row > last_row {
                    self.partial = true;
                    break;
                }
                let expected = token.target.get(unit).map(|unit| unit.as_str());
                let newline = expected == Some("\n");
                let tab = expected == Some("\t");
                let mut width = if tab {
                    u16::from(self.tab_stop) - position.column % u16::from(self.tab_stop)
                } else {
                    token
                        .entered
                        .get(unit)
                        .map_or_else(|| token.target[unit].width(), |entry| entry.unit.width())
                        .max(1) as u16
                };
                width = width.min(self.width);
                if position.column + width > self.width {
                    position.row += 1;
                    position.column = 0;
                    if tab {
                        width = u16::from(self.tab_stop).min(self.width);
                    }
                }
                self.cells.push(Cell {
                    token: index,
                    unit,
                    separator: false,
                    position,
                    width,
                });
                units = unit + 1;
                position.column += width;
                if newline || position.column >= self.width {
                    position.row += 1;
                    position.column = 0;
                }
            }
            if !self.partial
                && engine.spec().policy == Policy::Prose
                && index + 1 < engine.tokens().len()
            {
                self.cells.push(Cell {
                    token: index,
                    unit: count,
                    separator: true,
                    position,
                    width: 1,
                });
                position.column += 1;
                if position.column >= self.width {
                    position.row += 1;
                    position.column = 0;
                }
            }
            self.tokens.push(TokenLayout {
                first,
                before,
                after: position,
                footprint: Self::footprint(engine, index),
                units,
                display_width: token.display_width,
            });
            if index == engine.current_token() && last_row == usize::MAX {
                last_row = position.row.saturating_add(8);
            }
            if self.partial || position.row > last_row {
                break;
            }
        }
    }
    pub fn logical_cursor(&self, engine: &Engine) -> Position {
        let current = engine.current_token();
        let Some(layout) = self.tokens.get(current) else {
            return self
                .tokens
                .last()
                .map_or(Position::default(), |token| token.after);
        };
        let unit = engine.tokens()[current].entered.len();
        self.cells
            .get(layout.first + unit)
            .filter(|cell| cell.token == current)
            .map_or(layout.after, |cell| cell.position)
    }
    pub fn select_anchor(&mut self, engine: &Engine, geometry: Geometry) -> Position {
        let cursor = self.logical_cursor(engine);
        self.anchor_row = cursor.row.saturating_sub(geometry.lines / 2);
        cursor
    }
    pub fn screen_cursor(&self, logical: Position, geometry: Geometry) -> Option<(u16, u16)> {
        let offset = logical.row.checked_sub(self.anchor_row)?;
        if offset >= geometry.lines {
            return None;
        }
        Some((
            geometry.text.x + logical.column.min(geometry.text.width - 1),
            geometry.text.y + offset as u16 * (geometry.spacing + 1),
        ))
    }
    pub fn target_units_before_cursor(&self, engine: &Engine) -> usize {
        engine.logical_target_position()
    }
    fn pace_position(&self, engine: &Engine, units: usize) -> Option<Position> {
        let index = engine
            .tokens()
            .partition_point(|token| token.target_start <= units)
            .checked_sub(1)?;
        let token = &engine.tokens()[index];
        let layout = self.tokens.get(index)?;
        let offset = units - token.target_start;
        if offset < token.target.len() {
            self.cells
                .get(layout.first + offset)
                .filter(|cell| cell.token == index && !cell.separator)
                .map(|cell| cell.position)
        } else if offset == token.target.len() && engine.spec().policy == Policy::Prose {
            self.cells
                .get(layout.first + token.target.len().max(token.entered.len()))
                .filter(|cell| cell.token == index && cell.separator)
                .map(|cell| cell.position)
        } else {
            None
        }
    }
}

pub struct TypingWidget<'a> {
    pub engine: &'a Engine,
    pub viewport: &'a Viewport,
    pub geometry: Geometry,
    pub theme: Theme,
    pub appearance: &'a Appearance,
    pub pace: Option<usize>,
}
impl Widget for TypingWidget<'_> {
    fn render(self, _area: Rect, buffer: &mut Buffer) {
        let current = self.engine.current_token();
        let first = self
            .viewport
            .cells
            .partition_point(|cell| cell.position.row < self.viewport.anchor_row);
        for cell in self.viewport.cells[first..]
            .iter()
            .take_while(|cell| cell.position.row < self.viewport.anchor_row + self.geometry.lines)
        {
            let token = &self.engine.tokens()[cell.token];
            let entry = (!cell.separator)
                .then(|| token.entered.get(cell.unit))
                .flatten();
            let expected = token
                .target
                .get(cell.unit)
                .map_or(" ", |unit| unit.as_str());
            let text = entry.map_or(expected, |entry| entry.unit.as_str());
            let mut style = if cell.separator {
                if token.separator {
                    self.theme.correct
                } else {
                    self.theme.pending
                }
            } else if let Some(entry) = entry {
                if self.engine.spec().rules.blind && self.engine.state() != State::Results {
                    self.theme.correct
                } else if cell.unit >= token.target.len() {
                    self.theme.extra
                } else if entry.correct {
                    self.theme.correct
                } else {
                    self.theme.incorrect
                }
            } else {
                self.theme.pending
            };
            if cell.token < current && !token.committed {
                style = self.theme.pending;
            }
            let display = super::display_unit(text, self.appearance.ascii_markers);
            if let Some((x, y)) = self.viewport.screen_cursor(cell.position, self.geometry) {
                // Never pass terminal escape bytes through: all text was validated/prepared.
                buffer.set_stringn(x, y, display, cell.width as usize, style);
                if cell.width > 1 && matches!(text, "\n" | "\t") {
                    for extra in 1..cell.width {
                        buffer[(x + extra, y)].set_symbol(" ").set_style(style);
                    }
                }
            }
        }
        let cursor = self.viewport.logical_cursor(self.engine);
        if let Some(units) = self.pace
            && let Some(position) = self.viewport.pace_position(self.engine, units)
            && position != cursor
            && let Some((x, y)) = self.viewport.screen_cursor(position, self.geometry)
        {
            buffer[(x, y)].set_style(self.theme.pace);
        }
        if let Some((x, y)) = self.viewport.screen_cursor(cursor, self.geometry) {
            let mut style = self.theme.caret;
            if self.appearance.caret == Caret::Block {
                style = style.add_modifier(Modifier::REVERSED);
            }
            // Also a useful fallback when an emulator ignores native cursor shape commands.
            buffer[(x, y)].set_style(style);
        }
    }
}

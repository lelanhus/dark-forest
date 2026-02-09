use bitflags::bitflags;
use ratatui::style::Color;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Attrs: u8 {
        const BOLD = 0b0000_0001;
        const DIM = 0b0000_0010;
        const UNDERLINE = 0b0000_0100;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub glyph: char,
    pub fg: Color,
    pub bg: Color,
    pub attrs: Attrs,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            glyph: ' ',
            fg: Color::Reset,
            bg: Color::Reset,
            attrs: Attrs::empty(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u16,
    pub height: u16,
    pub cells: Vec<Cell>,
}

impl Frame {
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        let len = usize::from(width) * usize::from(height);
        Self {
            width,
            height,
            cells: vec![Cell::default(); len],
        }
    }

    pub fn clear(&mut self) {
        self.cells.fill(Cell::default());
    }

    #[must_use]
    pub fn index(&self, x: u16, y: u16) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }

        Some(usize::from(y) * usize::from(self.width) + usize::from(x))
    }

    pub fn set(&mut self, x: u16, y: u16, cell: Cell) {
        if let Some(idx) = self.index(x, y) {
            self.cells[idx] = cell;
        }
    }

    #[must_use]
    pub fn get(&self, x: u16, y: u16) -> Option<Cell> {
        self.index(x, y).map(|idx| self.cells[idx])
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.cells = vec![Cell::default(); usize::from(width) * usize::from(height)];
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeltaCell {
    pub x: u16,
    pub y: u16,
    pub cell: Cell,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameDelta {
    pub changes: Vec<DeltaCell>,
}

impl FrameDelta {
    #[must_use]
    pub fn between(previous: &Frame, next: &Frame) -> Self {
        if previous.width != next.width || previous.height != next.height {
            return Self {
                changes: next
                    .cells
                    .iter()
                    .enumerate()
                    .map(|(idx, cell)| {
                        let width = usize::from(next.width);
                        let y = idx / width;
                        let x = idx % width;
                        DeltaCell {
                            x: u16::try_from(x).unwrap_or(u16::MAX),
                            y: u16::try_from(y).unwrap_or(u16::MAX),
                            cell: *cell,
                        }
                    })
                    .collect(),
            };
        }

        let mut changes = Vec::new();
        let width = usize::from(next.width);
        for (idx, (a, b)) in previous.cells.iter().zip(&next.cells).enumerate() {
            if a != b {
                let y = idx / width;
                let x = idx % width;
                changes.push(DeltaCell {
                    x: u16::try_from(x).unwrap_or(u16::MAX),
                    y: u16::try_from(y).unwrap_or(u16::MAX),
                    cell: *b,
                });
            }
        }

        Self { changes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn diff_only_reports_changed_cells() {
        let mut prev = Frame::new(4, 2);
        let mut next = prev.clone();

        next.set(
            1,
            0,
            Cell {
                glyph: 'X',
                fg: Color::Green,
                ..Cell::default()
            },
        );

        next.set(
            3,
            1,
            Cell {
                glyph: 'Y',
                fg: Color::Red,
                ..Cell::default()
            },
        );

        let delta = FrameDelta::between(&prev, &next);
        assert_eq!(delta.changes.len(), 2);

        prev.set(
            1,
            0,
            Cell {
                glyph: 'X',
                fg: Color::Green,
                ..Cell::default()
            },
        );
        prev.set(
            3,
            1,
            Cell {
                glyph: 'Y',
                fg: Color::Red,
                ..Cell::default()
            },
        );

        assert_eq!(FrameDelta::between(&prev, &next).changes.len(), 0);
    }
}

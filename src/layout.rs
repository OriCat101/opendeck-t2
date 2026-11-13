pub const COLS: u8 = 16;

/// Key grid in logical (landscape) bar coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    pub cols: u8,
}

impl Layout {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            cols: COLS,
        }
    }

    /// Left edge and width of a key cell.
    pub fn cell(&self, key: u8) -> (u32, u32) {
        let cols = self.cols as u32;
        let start = key as u32 * self.width / cols;
        let end = (key as u32 + 1) * self.width / cols;
        (start, end - start)
    }

    /// Key under a horizontal position given as a fraction of the bar length.
    pub fn key_at(&self, frac: f64) -> u8 {
        let key = (frac.clamp(0.0, 1.0) * self.cols as f64) as u8;
        key.min(self.cols - 1)
    }
}

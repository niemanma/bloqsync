//! Turn a captured `Frame` into one colour per LED via border sampling.

use crate::capture::Frame;
use crate::protocol::Rgb;

/// How many LEDs are assigned to each side of the screen border.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub left: usize,
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
}

impl Default for Layout {
    fn default() -> Self {
        Layout { left: 18, top: 18, right: 18, bottom: 0 }
    }
}

impl Layout {
    pub fn total(&self) -> usize {
        self.left + self.top + self.right + self.bottom
    }
}

#[inline]
fn avg(frame: &Frame, x0: usize, y0: usize, w: usize, h: usize) -> Rgb {
    if w == 0 || h == 0 {
        return [0, 0, 0];
    }
    // Sample a sparse grid (~8 x 4) instead of every pixel.
    let step_x = (w / 8).max(1);
    let step_y = (h / 4).max(1);
    let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
    let mut y = 0;
    while y < h {
        let yy = (y0 + y).min(frame.height.saturating_sub(1));
        let mut x = 0;
        while x < w {
            let xx = (x0 + x).min(frame.width.saturating_sub(1));
            let p = frame.pixel(xx, yy);
            r += p[0] as u64;
            g += p[1] as u64;
            b += p[2] as u64;
            n += 1;
            x += step_x;
        }
        y += step_y;
    }
    if n == 0 {
        [0, 0, 0]
    } else {
        [(r / n) as u8, (g / n) as u8, (b / n) as u8]
    }
}

/// Sample the screen border into `layout.total()` LED colours.
///
/// Ordering is clockwise starting at the bottom-left corner (matches how the
/// strips are usually mounted); the `layout` counts are honoured per side.
pub fn sample_border(frame: &Frame, layout: &Layout) -> Vec<Rgb> {
    let w = frame.width;
    let h = frame.height;
    if w == 0 || h == 0 {
        return vec![[0, 0, 0]; layout.total()];
    }

    // Sampling depth (thickness) along the edge, as a fraction of the screen.
    let depth_x = (w / 10).max(1);
    let depth_y = (h / 10).max(1);

    let mut out = Vec::with_capacity(layout.total());

    // Left edge, bottom -> top.
    for i in 0..layout.left {
        let band_h = (h / layout.left.max(1)).max(1);
        let y = h.saturating_sub((i + 1) * band_h);
        let hh = band_h.min(h - y);
        out.push(avg(frame, 0, y, depth_x, hh));
    }
    // Top edge, left -> right.
    for i in 0..layout.top {
        let band_w = (w / layout.top.max(1)).max(1);
        let x = i * band_w;
        let ww = band_w.min(w - x);
        out.push(avg(frame, x, 0, ww, depth_y));
    }
    // Right edge, top -> bottom.
    for i in 0..layout.right {
        let band_h = (h / layout.right.max(1)).max(1);
        let y = i * band_h;
        let hh = band_h.min(h - y);
        out.push(avg(frame, w.saturating_sub(depth_x), y, depth_x, hh));
    }
    // Bottom edge, right -> left.
    for i in 0..layout.bottom {
        let band_w = (w / layout.bottom.max(1)).max(1);
        let x = w.saturating_sub((i + 1) * band_w);
        let ww = band_w.min(w - x);
        out.push(avg(frame, x, h.saturating_sub(depth_y), ww, depth_y));
    }

    out
}

/// Exponential smoothing towards a new frame (avoids flicker).
pub fn smooth(prev: &[Rgb], next: &[Rgb], alpha: f32) -> Vec<Rgb> {
    next.iter()
        .enumerate()
        .map(|(i, n)| {
            let p = prev.get(i).copied().unwrap_or(*n);
            [
                (n[0] as f32 * alpha + p[0] as f32 * (1.0 - alpha)) as u8,
                (n[1] as f32 * alpha + p[1] as f32 * (1.0 - alpha)) as u8,
                (n[2] as f32 * alpha + p[2] as f32 * (1.0 - alpha)) as u8,
            ]
        })
        .collect()
}

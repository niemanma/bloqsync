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
    out.extend(sample_left(frame, layout.left, depth_x));
    out.extend(sample_top(frame, layout.top, depth_y));
    out.extend(sample_right(frame, layout.right, depth_x));
    out.extend(sample_bottom(frame, layout.bottom, depth_y));
    out
}

/// Left edge, bottom -> top.
fn sample_left(frame: &Frame, count: usize, depth: usize) -> Vec<Rgb> {
    let h = frame.height;
    (0..count)
        .map(|i| {
            let band_h = (h / count.max(1)).max(1);
            let y = h.saturating_sub((i + 1) * band_h);
            let hh = band_h.min(h - y);
            avg(frame, 0, y, depth, hh)
        })
        .collect()
}

/// Top edge, left -> right.
fn sample_top(frame: &Frame, count: usize, depth: usize) -> Vec<Rgb> {
    let w = frame.width;
    (0..count)
        .map(|i| {
            let band_w = (w / count.max(1)).max(1);
            let x = i * band_w;
            let ww = band_w.min(w - x);
            avg(frame, x, 0, ww, depth)
        })
        .collect()
}

/// Right edge, top -> bottom.
fn sample_right(frame: &Frame, count: usize, depth: usize) -> Vec<Rgb> {
    let (w, h) = (frame.width, frame.height);
    (0..count)
        .map(|i| {
            let band_h = (h / count.max(1)).max(1);
            let y = i * band_h;
            let hh = band_h.min(h - y);
            avg(frame, w.saturating_sub(depth), y, depth, hh)
        })
        .collect()
}

/// Bottom edge, right -> left.
fn sample_bottom(frame: &Frame, count: usize, depth: usize) -> Vec<Rgb> {
    let (w, h) = (frame.width, frame.height);
    (0..count)
        .map(|i| {
            let band_w = (w / count.max(1)).max(1);
            let x = w.saturating_sub((i + 1) * band_w);
            let ww = band_w.min(w - x);
            avg(frame, x, h.saturating_sub(depth), ww, depth)
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::PixelFormat;

    fn frame_rgb(w: usize, h: usize, px: impl Fn(usize, usize) -> Rgb) -> Frame {
        let mut data = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                data.extend_from_slice(&px(x, y));
            }
        }
        Frame {
            width: w,
            height: h,
            stride: w * 3,
            format: PixelFormat::Rgb,
            data,
            seq: 0,
        }
    }

    fn uniform(w: usize, h: usize, color: Rgb) -> Frame {
        frame_rgb(w, h, |_, _| color)
    }

    #[test]
    fn layout_total_sums_all_sides() {
        assert_eq!(Layout { left: 1, top: 2, right: 3, bottom: 4 }.total(), 10);
        assert_eq!(Layout::default().total(), 54);
        assert_eq!(Layout { left: 0, top: 0, right: 0, bottom: 0 }.total(), 0);
    }

    #[test]
    fn zero_sized_frame_yields_black() {
        let f = Frame {
            width: 0,
            height: 0,
            stride: 0,
            format: PixelFormat::Rgb,
            data: Vec::new(),
            seq: 0,
        };
        let layout = Layout { left: 2, top: 2, right: 2, bottom: 2 };
        assert_eq!(sample_border(&f, &layout), vec![[0, 0, 0]; 8]);
    }

    #[test]
    fn uniform_frame_yields_uniform_border() {
        let f = uniform(100, 100, [12, 34, 56]);
        let layout = Layout { left: 5, top: 5, right: 5, bottom: 5 };
        let cols = sample_border(&f, &layout);
        assert_eq!(cols.len(), 20);
        assert!(cols.iter().all(|c| *c == [12, 34, 56]));
    }

    #[test]
    fn border_order_is_left_top_right_bottom() {
        //   A B
        //   C D
        let a = [0, 0, 0];
        let b = [100, 0, 0];
        let c = [0, 100, 0];
        let d = [0, 0, 100];
        let f = frame_rgb(2, 2, |x, y| match (x, y) {
            (0, 0) => a,
            (1, 0) => b,
            (0, 1) => c,
            _ => d,
        });
        let layout = Layout { left: 1, top: 1, right: 1, bottom: 1 };
        let cols = sample_border(&f, &layout);
        // left column (A,C), top row (A,B), right column (B,D), bottom row (C,D)
        assert_eq!(cols, vec![[0, 50, 0], [50, 0, 0], [50, 0, 50], [0, 50, 50]]);
    }

    #[test]
    fn side_counts_are_honoured() {
        let f = uniform(64, 64, [1, 2, 3]);
        let layout = Layout { left: 3, top: 0, right: 0, bottom: 0 };
        assert_eq!(sample_border(&f, &layout).len(), 3);
        let layout = Layout { left: 0, top: 0, right: 4, bottom: 0 };
        assert_eq!(sample_border(&f, &layout).len(), 4);
    }

    #[test]
    fn avg_ignores_empty_region() {
        // depth 0 is clamped to 1 by callers, but `avg` itself must be safe.
        assert_eq!(avg(&uniform(4, 4, [9, 9, 9]), 0, 0, 0, 4), [0, 0, 0]);
        assert_eq!(avg(&uniform(4, 4, [9, 9, 9]), 0, 0, 4, 0), [0, 0, 0]);
    }

    #[test]
    fn smooth_alpha_one_returns_next() {
        let prev = [[10, 20, 30]];
        let next = [[200, 100, 50]];
        assert_eq!(smooth(&prev, &next, 1.0), next.to_vec());
    }

    #[test]
    fn smooth_alpha_zero_returns_prev() {
        let prev = [[10, 20, 30]];
        let next = [[200, 100, 50]];
        assert_eq!(smooth(&prev, &next, 0.0), prev.to_vec());
    }

    #[test]
    fn smooth_missing_prev_uses_next() {
        let next = [[10, 20, 30], [40, 50, 60]];
        assert_eq!(smooth(&[], &next, 0.5), next.to_vec());
    }

    #[test]
    fn smooth_handles_shorter_prev() {
        // Second LED has no previous value -> unchanged.
        let prev = [[0, 0, 0]];
        let next = [[100, 100, 100], [7, 8, 9]];
        assert_eq!(smooth(&prev, &next, 1.0), next.to_vec());
    }
}

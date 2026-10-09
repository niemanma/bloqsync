//! Wire protocol for the ROBOBLOQ SyncLight LED bar (VID 0x1A86 / PID 0xFE07).
//!
//! Verified against the SyncLight 2.22.1 desktop app (`app.asar`) and
//! live hardware (firmware 1.9.4, 54 LEDs):
//!
//! * Two frame families, both checksummed with `sum(bytes) % 256`:
//!   * `RB`: `52 42 LEN ID ACT payload... CHK`  (LEN = total bytes, 1 byte)
//!   * `SC`: `53 43 LENhi LENlo ID ACT payload... CHK` (16-bit big-endian LEN)
//! * `setSyncScreen` (0x80) streams colour over SC frames.
//! * Colour is encoded as 5-byte *sections*: `[start, R, G, B, end]`, 1-based,
//!   inclusive. `end == 254` means "until end of strip".
//! * CRITICAL: the device does NOT reassemble frames split across multiple USB
//!   reports. Every frame must fit in a single 64-byte report.
//!   => at most 11 sections (55 payload bytes) per frame.

use std::sync::atomic::{AtomicU8, Ordering};

/// A single USB HID report (the device uses unnumbered 64-byte reports).
pub const REPORT_SIZE: usize = 64;
/// RB overhead: 'R' 'B' LEN ID ACT ... CHK
pub const RB_OVERHEAD: usize = 6;
/// SC overhead: 'S' 'C' LENhi LENlo ID ACT ... CHK
pub const SC_OVERHEAD: usize = 7;
pub const SECTION_SIZE: usize = 5;
/// Max sections that still fit in one 64-byte SC frame: (64-7)/5 = 11.
pub const MAX_SECTIONS_PER_FRAME: usize = (REPORT_SIZE - SC_OVERHEAD) / SECTION_SIZE;
/// Largest 1-based LED index the firmware accepts.
pub const MAX_LEDS: usize = 254;

pub const ACT_SET_SYNC_SCREEN: u8 = 0x80;
pub const ACT_READ_DEVICE_INFO: u8 = 0x82;
pub const ACT_SET_LED_EFFECT: u8 = 0x85;
pub const ACT_SET_SECTION_LED: u8 = 0x86;
pub const ACT_SET_BRIGHTNESS: u8 = 0x87;
pub const ACT_TURN_OFF_LIGHT: u8 = 0x97;

pub type Rgb = [u8; 3];
pub const BLACK: Rgb = [0, 0, 0];

#[inline]
pub fn checksum(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |acc, &b| acc.wrapping_add(b))
}

/// Packet sequence id. Mirrors the firmware's `setID()`: starts at 1, is
/// incremented *before* being returned (first id is 2), wraps 255 -> 1.
#[derive(Debug)]
pub struct Seq(AtomicU8);

impl Default for Seq {
    fn default() -> Self {
        Self::new()
    }
}

impl Seq {
    pub fn new() -> Self {
        Seq(AtomicU8::new(1))
    }

    pub fn next(&self) -> u8 {
        let mut cur = self.0.load(Ordering::Relaxed);
        loop {
            let mut next = cur.wrapping_add(1);
            if next == 0 || next >= 255 {
                next = 1;
            }
            match self
                .0
                .compare_exchange_weak(cur, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => return next,
                Err(observed) => cur = observed,
            }
        }
    }
}

/// Build an `RB` frame.
pub fn rb_frame(seq: u8, action: u8, payload: &[u8]) -> Vec<u8> {
    let total = RB_OVERHEAD + payload.len();
    assert!(total <= 255, "RB frame too long: {total}");
    let mut b = vec![0u8; total];
    b[0] = b'R';
    b[1] = b'B';
    b[2] = total as u8;
    b[3] = seq;
    b[4] = action;
    b[5..5 + payload.len()].copy_from_slice(payload);
    b[total - 1] = checksum(&b[..total - 1]);
    b
}

/// Build an `SC` frame (16-bit big-endian length).
pub fn sc_frame(seq: u8, action: u8, payload: &[u8]) -> Vec<u8> {
    let total = SC_OVERHEAD + payload.len();
    let mut b = vec![0u8; total];
    b[0] = b'S';
    b[1] = b'C';
    b[2] = (total >> 8) as u8;
    b[3] = (total & 0xff) as u8;
    b[4] = seq;
    b[5] = action;
    b[6..6 + payload.len()].copy_from_slice(payload);
    b[total - 1] = checksum(&b[..total - 1]);
    b
}

/// One colour range on the strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Section {
    pub start: u8,
    pub end: u8,
    pub color: Rgb,
}

/// Compress a full-strip colour buffer into the fewest possible sections by
/// merging runs of identical colour. LEDs beyond `colors.len()` (up to
/// `led_count`) are blanked.
pub fn collapse(colors: &[Rgb], led_count: usize) -> Vec<Section> {
    collapse_tol(colors, led_count, 0)
}

#[inline]
fn within(a: Rgb, b: Rgb, tol: u8) -> bool {
    if tol == 0 {
        return a == b;
    }
    (0..3).all(|k| (a[k] as i16 - b[k] as i16).unsigned_abs() as u8 <= tol)
}

/// Like [`collapse`] but merges adjacent LEDs whose colour differs by at most
/// `tol` per channel. Fewer sections => fewer frames per update => less
/// tearing and a more atomic update. `tol = 0` keeps exact colours.
pub fn collapse_tol(colors: &[Rgb], led_count: usize, tol: u8) -> Vec<Section> {
    let n = colors.len().min(MAX_LEDS);
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let color = colors[i];
        let mut j = i + 1;
        while j < n && within(colors[j], color, tol) {
            j += 1;
        }
        out.push(Section {
            start: (i + 1) as u8,
            end: j as u8,
            color,
        });
        i = j;
    }
    // Blank any trailing LEDs we did not provide a colour for.
    if n < led_count && led_count <= MAX_LEDS {
        out.push(Section {
            start: (n + 1) as u8,
            end: led_count as u8,
            color: BLACK,
        });
    }
    out
}

/// Serialise sections into the 5-byte wire groups.
pub fn sections_payload(sections: &[Section]) -> Vec<u8> {
    let mut p = Vec::with_capacity(sections.len() * SECTION_SIZE);
    for s in sections {
        p.push(s.start);
        p.extend_from_slice(&s.color);
        p.push(s.end);
    }
    p
}

/// Split sections into 64-byte-safe `setSyncScreen` frames.
fn encode_screen_frames(seq: &Seq, sections: &[Section]) -> Vec<Vec<u8>> {
    sections
        .chunks(MAX_SECTIONS_PER_FRAME)
        .map(|chunk| {
            let payload = sections_payload(chunk);
            sc_frame(seq.next(), ACT_SET_SYNC_SCREEN, &payload)
        })
        .collect()
}

/// Build all `setSyncScreen` (SC / 0x80) frames needed to display `colors`,
/// each guaranteed to fit in one 64-byte report.
pub fn sc_frames(seq: &Seq, colors: &[Rgb], led_count: usize, tol: u8) -> Vec<Vec<u8>> {
    encode_screen_frames(seq, &collapse_tol(colors, led_count, tol))
}

/// Build `setSyncScreen` frames constrained to at most `max_frames` per update
/// by progressively merging near colours. `max_frames = 1` makes every update
/// a single HID report => atomic, no tearing/flicker (at the cost of colour
/// resolution, which is imperceptible for ambient light).
pub fn sc_frames_fit(
    seq: &Seq,
    colors: &[Rgb],
    led_count: usize,
    max_frames: usize,
) -> Vec<Vec<u8>> {
    let max_frames = max_frames.max(1);
    let max_sections = max_frames * MAX_SECTIONS_PER_FRAME;
    let mut tol: u16 = 0;
    loop {
        let sections = collapse_tol(colors, led_count, tol.min(255) as u8);
        if sections.len() <= max_sections || tol >= 255 {
            return encode_screen_frames(seq, &sections);
        }
        tol += 2;
    }
}

/// Persistent single-colour command (`RB` / `0x86`). Works even while no
/// stream is running; used as the fallback state.
pub fn persistent_color(seq: u8, color: Rgb) -> Vec<u8> {
    rb_frame(seq, ACT_SET_SECTION_LED, &[1, color[0], color[1], color[2], 254])
}

pub fn set_brightness(seq: u8, value: u8) -> Vec<u8> {
    rb_frame(seq, ACT_SET_BRIGHTNESS, &[value])
}

pub fn turn_off(seq: u8) -> Vec<u8> {
    rb_frame(seq, ACT_TURN_OFF_LIGHT, &[])
}

pub fn read_device_info(seq: u8) -> Vec<u8> {
    rb_frame(seq, ACT_READ_DEVICE_INFO, &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seq_ids(seq: &Seq, n: usize) -> Vec<u8> {
        (0..n).map(|_| seq.next()).collect()
    }

    #[test]
    fn checksum_empty_is_zero() {
        assert_eq!(checksum(&[]), 0);
    }

    #[test]
    fn checksum_sums_mod_256() {
        assert_eq!(checksum(&[1, 2, 3]), 6);
        assert_eq!(checksum(&[255, 1]), 0);
        assert_eq!(checksum(&[255, 255]), 254);
        assert_eq!(checksum(&[255; 256]), 0);
    }

    #[test]
    fn seq_starts_at_two_and_skips_zero_and_255() {
        let seq = Seq::new();
        let seen = seq_ids(&seq, 300);
        assert_eq!(seen[0], 2);
        assert!(seen.iter().all(|&v| v != 0 && v != 255));
        // 2..=254 is 253 values, then it wraps back to 1.
        assert_eq!(seen[253], 1);
        assert_eq!(seen[254], 2);
    }

    #[test]
    fn golden_rb_frames() {
        // Matches the app's vectors (offset by the id counter).
        let f = rb_frame(0x0e, ACT_TURN_OFF_LIGHT, &[]);
        assert_eq!(f, vec![0x52, 0x42, 0x06, 0x0e, 0x97, 0x3f]);
    }

    #[test]
    fn rb_frame_layout_and_checksum() {
        let f = rb_frame(0x10, 0x42, &[1, 2, 3]);
        assert_eq!(&f[0..2], b"RB");
        assert_eq!(f[2] as usize, RB_OVERHEAD + 3);
        assert_eq!(f[3], 0x10);
        assert_eq!(f[4], 0x42);
        assert_eq!(&f[5..8], &[1, 2, 3]);
        assert_eq!(f[f.len() - 1], checksum(&f[..f.len() - 1]));
    }

    #[test]
    fn rb_frame_accepts_max_payload() {
        let payload = [0u8; 255 - RB_OVERHEAD];
        let f = rb_frame(1, 0x80, &payload);
        assert_eq!(f.len(), 255);
        assert_eq!(f[2], 255);
        assert_eq!(f[f.len() - 1], checksum(&f[..f.len() - 1]));
    }

    #[test]
    #[should_panic]
    fn rb_frame_rejects_overflow() {
        rb_frame(1, 0x80, &[0u8; 255 - RB_OVERHEAD + 1]);
    }

    #[test]
    fn golden_sc_single_green() {
        let f = sc_frame(0x02, ACT_SET_SYNC_SCREEN, &[1, 0, 255, 0, 254]);
        // 53 43 00 0C 02 80 01 00 FF 00 FE 27
        assert_eq!(f.len(), 12);
        assert_eq!(&f[0..2], b"SC");
        assert_eq!((f[2] as u16) << 8 | f[3] as u16, 12);
        assert_eq!(f[4], 0x02);
        assert_eq!(f[5], ACT_SET_SYNC_SCREEN);
        assert_eq!(f[f.len() - 1], checksum(&f[..f.len() - 1]));
    }

    #[test]
    fn sc_frame_uses_big_endian_length() {
        // 300-byte payload forces a >255 total, exercising the high length byte.
        let f = sc_frame(1, ACT_SET_SYNC_SCREEN, &[0u8; 300]);
        let total = SC_OVERHEAD + 300;
        assert_eq!(f[2] as usize, total >> 8);
        assert_eq!(f[3] as usize, total & 0xff);
        assert_eq!(f.len(), total);
    }

    #[test]
    fn collapse_blanks_trailing_leds() {
        let cols = [[10, 20, 30]; 3];
        let s = collapse(&cols, 5);
        assert_eq!(
            s,
            vec![
                Section { start: 1, end: 3, color: [10, 20, 30] },
                Section { start: 4, end: 5, color: BLACK },
            ]
        );
    }

    #[test]
    fn collapse_handles_empty_input() {
        assert_eq!(collapse(&[], 0), vec![]);
        assert_eq!(collapse(&[], 4), vec![Section { start: 1, end: 4, color: BLACK }]);
    }

    #[test]
    fn collapse_merges_identical_runs() {
        let cols = [[1, 1, 1], [1, 1, 1], [9, 9, 9], [1, 1, 1]];
        let s = collapse(&cols, 4);
        let spans: Vec<(u8, u8)> = s.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(spans, vec![(1, 2), (3, 3), (4, 4)]);
        assert!(s.iter().all(|sec| sec.color != BLACK));
    }

    #[test]
    fn collapse_truncates_beyond_max_leds() {
        let cols = vec![[1, 2, 3]; MAX_LEDS + 10];
        let s = collapse(&cols, MAX_LEDS + 10);
        assert_eq!(s.first().unwrap().start, 1);
        assert_eq!(s.last().unwrap().end as usize, MAX_LEDS);
    }

    #[test]
    fn collapse_tol_zero_keeps_exact_colors() {
        let cols = [[100, 100, 100], [110, 100, 100], [200, 200, 200]];
        assert_eq!(collapse_tol(&cols, 3, 0).len(), 3);
    }

    #[test]
    fn collapse_tol_merges_within_threshold() {
        let cols = [[100, 100, 100], [110, 100, 100], [200, 200, 200]];
        let s = collapse_tol(&cols, 3, 10);
        let spans: Vec<(u8, u8)> = s.iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(spans, vec![(1, 2), (3, 3)]);
        assert_eq!(s[0].color, [100, 100, 100]);
    }

    #[test]
    fn collapse_tol_compares_against_run_anchor() {
        // Adjacent diffs are within tolerance but the run anchor is far from
        // the third LED, which must therefore start a new section.
        let cols = [[0, 0, 0], [10, 0, 0], [21, 0, 0]];
        assert_eq!(collapse_tol(&cols, 3, 10).len(), 2);
    }

    #[test]
    fn sections_payload_is_five_bytes_each() {
        let s = vec![
            Section { start: 1, end: 2, color: [4, 5, 6] },
            Section { start: 3, end: 3, color: [7, 8, 9] },
        ];
        assert_eq!(sections_payload(&s), vec![1, 4, 5, 6, 2, 3, 7, 8, 9, 3]);
        assert_eq!(sections_payload(&s).len(), s.len() * SECTION_SIZE);
    }

    #[test]
    fn sc_frames_chunk_into_single_reports() {
        let seq = Seq::new();
        let cols: Vec<Rgb> = (0..23).map(|i| [i as u8, 0, 0]).collect();
        let frames = sc_frames(&seq, &cols, 23, 0);
        assert_eq!(frames.len(), 23usize.div_ceil(MAX_SECTIONS_PER_FRAME));
        for f in &frames {
            assert!(f.len() <= REPORT_SIZE, "frame {} > {}", f.len(), REPORT_SIZE);
            assert_eq!(&f[0..2], b"SC");
        }
    }

    #[test]
    fn sc_frames_fit_respects_budget() {
        let seq = Seq::new();
        let cols: Vec<Rgb> = (0..54).map(|i| [i as u8, (i * 2) as u8, (i * 3) as u8]).collect();
        assert!(sc_frames_fit(&seq, &cols, 54, 1).len() <= 1);
        assert!(sc_frames_fit(&seq, &cols, 54, 5).len() <= 5);
        // Budget 0 behaves like 1 (never zero frames).
        assert_eq!(sc_frames_fit(&seq, &cols, 54, 0).len(), 1);
    }

    #[test]
    fn sc_frames_fit_can_always_reach_one_frame() {
        let seq = Seq::new();
        let cols: Vec<Rgb> = (0..MAX_LEDS as u8).map(|i| [i, 255 - i, 128]).collect();
        assert_eq!(sc_frames_fit(&seq, &cols, MAX_LEDS, 1).len(), 1);
    }

    #[test]
    fn command_helpers_wrap_protocol_frames() {
        assert_eq!(
            persistent_color(1, [1, 2, 3]),
            rb_frame(1, ACT_SET_SECTION_LED, &[1, 1, 2, 3, 254])
        );
        assert_eq!(set_brightness(2, 200), rb_frame(2, ACT_SET_BRIGHTNESS, &[200]));
        assert_eq!(turn_off(3), rb_frame(3, ACT_TURN_OFF_LIGHT, &[]));
        assert_eq!(read_device_info(4), rb_frame(4, ACT_READ_DEVICE_INFO, &[]));
    }

    #[test]
    fn frames_never_exceed_one_report() {
        let seq = Seq::new();
        let colors: Vec<Rgb> = (0..54).map(|i| [i as u8, 0, 255 - i as u8]).collect();
        for f in sc_frames(&seq, &colors, 54, 0) {
            assert!(f.len() <= REPORT_SIZE, "frame {} > {}", f.len(), REPORT_SIZE);
        }
    }
}

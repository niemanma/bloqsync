//! Monitor-setup detection via `xrandr` (works on XWayland/GNOME).
//!
//! A setup is identified by a stable signature of connected outputs incl.
//! resolution and position, e.g.
//! `DVI-I-1:1920x1080+0+0|DVI-I-2:1920x1080+1920+0`.

use std::process::Command;

#[derive(Clone, Debug)]
pub struct Output {
    pub name: String,
    pub w: u32,
    pub h: u32,
    pub x: i32,
    pub y: i32,
}

/// Stable signature of the current monitor setup (empty if unavailable).
pub fn signature() -> String {
    signature_of(&outputs())
}

fn signature_of(outs: &[Output]) -> String {
    let mut parts: Vec<String> = outs
        .iter()
        .map(|o| format!("{}:{}x{}+{}+{}", o.name, o.w, o.h, o.x, o.y))
        .collect();
    parts.sort();
    parts.join("|")
}

pub fn outputs() -> Vec<Output> {
    match Command::new("xrandr").arg("--query").output() {
        Ok(o) => parse_outputs(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => Vec::new(),
    }
}

fn parse_outputs(text: &str) -> Vec<Output> {
    let mut v = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.contains(" connected ") {
            continue;
        }
        let name = match line.split_whitespace().next() {
            Some(n) => n.to_string(),
            None => continue,
        };
        for tok in line.split_whitespace() {
            if let Some((w, h, x, y)) = parse_geometry(tok) {
                v.push(Output { name, w, h, x, y });
                break;
            }
        }
    }
    v
}

fn parse_geometry(tok: &str) -> Option<(u32, u32, i32, i32)> {
    let (w, rest) = tok.split_once('x')?;
    let w: u32 = w.parse().ok()?;
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    let h: u32 = rest[..i].parse().ok()?;
    let (x, y) = parse_two_signed(&rest[i..])?;
    Some((w, h, x, y))
}

fn parse_two_signed(s: &str) -> Option<(i32, i32)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut nums = [0i32; 2];
    for num in nums.iter_mut() {
        if i >= bytes.len() || (bytes[i] != b'+' && bytes[i] != b'-') {
            return None;
        }
        let neg = bytes[i] == b'-';
        i += 1;
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return None;
        }
        let val: i32 = s[start..i].parse().ok()?;
        *num = if neg { -val } else { val };
    }
    Some((nums[0], nums[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_geometry_accepts_signed_offsets() {
        assert_eq!(parse_geometry("1920x1080+0+0"), Some((1920, 1080, 0, 0)));
        assert_eq!(parse_geometry("1920x1080+1920+0"), Some((1920, 1080, 1920, 0)));
        assert_eq!(parse_geometry("2560x1440-100-50"), Some((2560, 1440, -100, -50)));
        assert_eq!(parse_geometry("1x1+0+0"), Some((1, 1, 0, 0)));
    }

    #[test]
    fn parse_geometry_rejects_non_geometry_tokens() {
        for tok in ["primary", "1920", "", "1920x+0+0", "x1080+0+0", "1920x1080", "1920x1080+"] {
            assert_eq!(parse_geometry(tok), None, "token {tok:?}");
        }
    }

    #[test]
    fn parse_two_signed_handles_signs() {
        assert_eq!(parse_two_signed("+0+0"), Some((0, 0)));
        assert_eq!(parse_two_signed("-5+3"), Some((-5, 3)));
        assert_eq!(parse_two_signed("+10-20"), Some((10, -20)));
        assert_eq!(parse_two_signed("-0-0"), Some((0, 0)));
        assert_eq!(parse_two_signed("+"), None);
        assert_eq!(parse_two_signed("0+0"), None);
        assert_eq!(parse_two_signed("+1+"), None);
    }

    #[test]
    fn parse_outputs_skips_disconnected_entries() {
        let sample = "\
Screen 0: minimum 320 x 200, current 4480 x 1440, maximum 16384 x 16384
HDMI-1 connected primary 1920x1080+0+0 (normal left inverted right x axis y axis) 527mm x 296mm
DP-1 connected 2560x1440+1920+0 (normal left inverted right x axis y axis) 597mm x 336mm
DP-2 disconnected (normal left inverted right x axis y axis)
";
        let outs = parse_outputs(sample);
        assert_eq!(outs.len(), 2);
        assert_eq!(outs[0].name, "HDMI-1");
        assert_eq!((outs[0].w, outs[0].h, outs[0].x, outs[0].y), (1920, 1080, 0, 0));
        assert_eq!(outs[1].name, "DP-1");
        assert_eq!((outs[1].w, outs[1].h, outs[1].x, outs[1].y), (2560, 1440, 1920, 0));
    }

    #[test]
    fn parse_outputs_ignores_lines_without_geometry() {
        let outs = parse_outputs("HDMI-1 connected primary\n");
        assert!(outs.is_empty());
    }

    #[test]
    fn signature_sorts_and_joins_outputs() {
        let a = Output { name: "B".into(), w: 1920, h: 1080, x: 0, y: 0 };
        let b = Output { name: "A".into(), w: 2560, h: 1440, x: 1920, y: 0 };
        assert_eq!(signature_of(&[a, b]), "A:2560x1440+1920+0|B:1920x1080+0+0");
        assert_eq!(signature_of(&[]), "");
    }
}

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
    let mut parts: Vec<String> = outputs()
        .iter()
        .map(|o| format!("{}:{}x{}+{}+{}", o.name, o.w, o.h, o.x, o.y))
        .collect();
    parts.sort();
    parts.join("|")
}

pub fn outputs() -> Vec<Output> {
    let out = match Command::new("xrandr").arg("--query").output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    let mut v = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
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

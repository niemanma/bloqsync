//! Device discovery and raw hidraw I/O.
//!
//! We talk to `/dev/hidrawN` directly (no hidapi dependency) because the device
//! uses *unnumbered* 64-byte HID reports: a command is written as exactly 64
//! bytes, no leading report-id byte. This is also the fastest path.

use crate::protocol::{self, Rgb, Seq};
use anyhow::{bail, Context, Result};
use std::ffi::CString;
use std::fs;
use std::os::unix::io::RawFd;

pub const VENDOR_ID: u16 = 0x1A86; // QinHeng / WCH
pub const PRODUCT_ID: u16 = 0xFE07; // ROBOBLOQ "LIGHT"

const VENDOR_USAGE_PAGE: [u8; 3] = [0x06, 0x00, 0xFF];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// `/dev/hidrawN`
    pub path: String,
    /// Stable identity from the USB topology (survives replug into same port).
    pub id: String,
    pub name: String,
    pub serial: String,
    /// Vendor control interface (0) vs. keyboard interface (1).
    pub interface: u8,
}

/// Enumerate all connected SyncLight control interfaces.
pub fn enumerate() -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    let entries = match fs::read_dir("/sys/class/hidraw") {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let node = entry.file_name().to_string_lossy().to_string(); // hidrawN
        let base = format!("/sys/class/hidraw/{node}");
        let uevent = fs::read_to_string(format!("{base}/device/uevent")).unwrap_or_default();
        let (mut hid_id, mut hid_name, mut hid_uniq) = (String::new(), String::new(), String::new());
        for line in uevent.lines() {
            if let Some(v) = line.strip_prefix("HID_ID=") {
                hid_id = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("HID_NAME=") {
                hid_name = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("HID_UNIQ=") {
                hid_uniq = v.trim().to_string();
            }
        }
        // HID_ID = 0003:00001A86:0000FE07  (bus:vendor:product as hex)
        let parts: Vec<&str> = hid_id.split(':').collect();
        if parts.len() != 3 {
            continue;
        }
        let vid = u32::from_str_radix(parts[1], 16).unwrap_or(0) as u16;
        let pid = u32::from_str_radix(parts[2], 16).unwrap_or(0) as u16;
        if vid != VENDOR_ID || pid != PRODUCT_ID {
            continue;
        }

        // Only the vendor interface (usage page 0xFF00) accepts colour commands.
        let desc = fs::read(format!("{base}/device/report_descriptor")).unwrap_or_default();
        if desc.len() < 3 || desc[..3] != VENDOR_USAGE_PAGE {
            continue;
        }

        // Stable id + interface number from the usb topology.
        // e.g. /sys/devices/.../usb1/1-2/1-2:1.0/0003:1A86:FE07.0014
        let real = fs::canonicalize(format!("{base}/device"))
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let iface_seg = real
            .split('/')
            .find(|s| s.starts_with("1-") && s.contains(':'))
            .unwrap_or("");
        // USB sysfs iface segment is "BUS-PORT:CONFIG.INTERFACE", e.g. "1-2:1.0".
        let interface = iface_seg
            .rsplit(':')
            .next()
            .and_then(|s| s.split('.').nth(1))
            .and_then(|s| s.parse::<u8>().ok())
            .unwrap_or(0);
        // USB port segment, e.g. "1-2" (dropping the ":1.0" config suffix).
        let id = iface_seg
            .split(':')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(&real)
            .to_string();

        out.push(DeviceInfo {
            path: format!("/dev/{node}"),
            id,
            name: hid_name,
            serial: hid_uniq,
            interface,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Find a bar by any stable identity: device UUID (port independent), USB-port
/// id (e.g. "1-2"), or `/dev/hidrawN` path.
pub fn find_by_identity(identity: &str) -> Option<DeviceInfo> {
    let infos = enumerate();
    if let Some(i) = infos
        .iter()
        .find(|i| i.path == identity || i.id == identity)
    {
        return Some(i.clone());
    }
    // Match the per-device UUID (survives moving to another USB port).
    for info in &infos {
        if let Ok(d) = Device::open(info) {
            if d.uuid == identity {
                return Some(info.clone());
            }
        }
    }
    None
}

pub struct Device {    pub info: DeviceInfo,
    fd: RawFd,
    seq: Seq,
    pub led_count: usize,
    pub firmware: String,
    pub uuid: String,
    pub device_id: String,
    pub display_size: u8,
}

impl Device {
    pub fn open(info: &DeviceInfo) -> Result<Device> {
        let c = CString::new(info.path.clone())?;
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDWR | libc::O_NONBLOCK) };
        if fd < 0 {
            bail!(
                "open {}: {}",
                info.path,
                std::io::Error::last_os_error()
            );
        }
        let mut dev = Device {
            info: info.clone(),
            fd,
            seq: Seq::new(),
            led_count: 0,
            firmware: String::new(),
            uuid: String::new(),
            device_id: String::new(),
            display_size: 0,
        };
        if let Err(e) = dev.identify() {
            dev.close_fd();
            return Err(e);
        }
        Ok(dev)
    }

    fn close_fd(&mut self) {
        if self.fd >= 0 {
            unsafe { libc::close(self.fd) };
            self.fd = -1;
        }
    }

    /// Query firmware version, LED count, UUID, board id.
    pub fn identify(&mut self) -> Result<()> {
        let frame = protocol::read_device_info(self.seq.next());
        self.write_raw(&frame)?;
        let r = self.read_response(500).context("device info response")?;
        if r.len() < 24 || &r[0..2] != b"RB" {
            bail!("malformed device info response ({} bytes)", r.len());
        }
        self.device_id = hex(&r[5..8]);
        self.display_size = r[8];
        self.led_count = r[11] as usize;
        self.uuid = hex(&r[12..20]);
        self.firmware = format!("{}.{}.{}", r[21], r[22], r[23]);
        if self.led_count == 0 {
            self.led_count = 54; // sane fallback
        }
        Ok(())
    }

    /// Write one command frame. Must be <= one 64-byte report.
    pub fn write_raw(&self, frame: &[u8]) -> Result<()> {
        if frame.len() > protocol::REPORT_SIZE {
            bail!(
                "frame of {} bytes exceeds the single-report limit of {}",
                frame.len(),
                protocol::REPORT_SIZE
            );
        }
        let mut buf = [0u8; protocol::REPORT_SIZE];
        buf[..frame.len()].copy_from_slice(frame);
        let n = unsafe { libc::write(self.fd, buf.as_ptr() as *const libc::c_void, buf.len()) };
        if n < 0 {
            bail!("hidraw write: {}", std::io::Error::last_os_error());
        }
        Ok(())
    }

    fn read_response(&self, timeout_ms: i32) -> Result<Vec<u8>> {
        let mut pfd = libc::pollfd {
            fd: self.fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let r = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if r <= 0 {
            bail!("no response within {timeout_ms} ms");
        }
        let mut buf = [0u8; protocol::REPORT_SIZE];
        let n = unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n < 0 {
            bail!("hidraw read: {}", std::io::Error::last_os_error());
        }
        Ok(buf[..n as usize].to_vec())
    }

    /// Stream a full-strip colour buffer via `setSyncScreen`. Frames are sent
    /// back-to-back, each within the single-report limit.
    pub fn send_colors(&self, colors: &[Rgb]) -> Result<()> {
        for frame in protocol::sc_frames(&self.seq, colors, self.led_count, 0) {
            self.write_raw(&frame)?;
        }
        Ok(())
    }

    /// Like [`send_colors`], but inserts `gap` between consecutive frames of
    /// the same update. `tol` merges adjacent LEDs whose colour is within
    /// `tol` per channel (fewer frames -> less tearing).
    pub fn send_colors_paced(&self, colors: &[Rgb], gap: std::time::Duration, tol: u8) -> Result<()> {
        let frames = protocol::sc_frames(&self.seq, colors, self.led_count, tol);
        self.write_frames_paced(&frames, gap)
    }

    /// Send an update constrained to at most `max_frames` reports. `max_frames
    /// = 1` yields a single atomic report (no tearing); `0` = unlimited.
    pub fn send_colors_fit(
        &self,
        colors: &[Rgb],
        gap: std::time::Duration,
        max_frames: usize,
    ) -> Result<()> {
        let frames = if max_frames == 0 {
            protocol::sc_frames(&self.seq, colors, self.led_count, 0)
        } else {
            protocol::sc_frames_fit(&self.seq, colors, self.led_count, max_frames)
        };
        self.write_frames_paced(&frames, gap)
    }

    fn write_frames_paced(&self, frames: &[Vec<u8>], gap: std::time::Duration) -> Result<()> {
        let last = frames.len().saturating_sub(1);
        for (i, frame) in frames.iter().enumerate() {
            self.write_raw(frame)?;
            if i < last {
                std::thread::sleep(gap);
            }
        }
        Ok(())
    }

    /// Set one persistent single colour (survives when no stream runs).
    pub fn set_persistent_color(&self, color: Rgb) -> Result<()> {
        self.write_raw(&protocol::persistent_color(self.seq.next(), color))
    }

    pub fn set_brightness(&self, value: u8) -> Result<()> {
        self.write_raw(&protocol::set_brightness(self.seq.next(), value))
    }

    pub fn turn_off(&self) -> Result<()> {
        // 0x97 is a no-op on some firmware; also paint black persistently.
        let _ = self.write_raw(&protocol::turn_off(self.seq.next()));
        self.set_persistent_color(protocol::BLACK)
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        self.close_fd();
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

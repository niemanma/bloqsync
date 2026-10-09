//! Wayland screen capture via `xdg-desktop-portal` ScreenCast (PipeWire).
//!
//! This is the high-performance path used on GNOME/Wayland: the portal returns
//! one PipeWire node per selected monitor; we consume frames with a single
//! memcpy into a shared "latest frame" slot per monitor.

use anyhow::{Context as _, Result};
use ashpd::desktop::screencast::{
    CursorMode, Screencast, SelectSourcesOptions, SourceType, Stream as PortalStream,
};
use ashpd::desktop::{PersistMode, Session};
use ashpd::enumflags2::BitFlags;
use pipewire as pw;
use pw::{properties::properties, spa};
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgba,
    Bgra,
    Rgbx,
    Bgrx,
    Rgb,
    Bgr,
    Unknown,
}

impl PixelFormat {
    fn from_spa(f: spa::param::video::VideoFormat) -> Self {
        use spa::param::video::VideoFormat as V;
        match f {
            V::RGBA => Self::Rgba,
            V::BGRA => Self::Bgra,
            V::RGBx => Self::Rgbx,
            V::BGRx => Self::Bgrx,
            V::RGB => Self::Rgb,
            V::BGR => Self::Bgr,
            _ => Self::Unknown,
        }
    }

    /// Bytes per pixel.
    pub fn channels(self) -> usize {
        match self {
            Self::Rgb | Self::Bgr => 3,
            _ => 4,
        }
    }
}

#[derive(Clone)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub format: PixelFormat,
    pub data: Vec<u8>,
    /// Monotonic frame counter for this stream (change detection).
    pub seq: u64,
}

impl Frame {
    #[inline]
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let bpp = self.format.channels();
        let idx = y * self.stride + x * bpp;
        if idx + 2 >= self.data.len() {
            return [0, 0, 0];
        }
        let (a, b, c) = (self.data[idx], self.data[idx + 1], self.data[idx + 2]);
        match self.format {
            PixelFormat::Bgra | PixelFormat::Bgrx | PixelFormat::Bgr => [c, b, a],
            _ => [a, b, c],
        }
    }
}

/// Shared latest-frame slot for one stream.
///
/// `consumed` lets the capture side skip work when the previous frame has not
/// been read yet (pacing capture to the consumer, saving CPU).
pub struct Slot {
    pub frame: Mutex<Option<Arc<Frame>>>,
    pub consumed: AtomicU64,
}

impl Default for Slot {
    fn default() -> Self {
        Slot {
            frame: Mutex::new(None),
            consumed: AtomicU64::new(0),
        }
    }
}

pub type FrameSlot = Arc<Slot>;

/// Metadata for one captured monitor stream.
#[derive(Clone, Debug, serde::Serialize)]
pub struct StreamInfo {
    pub index: usize,
    pub node_id: u32,
    pub position: Option<(i32, i32)>,
    pub size: Option<(i32, i32)>,
    pub id: Option<String>,
}

/// A running capture session. Dropping it stops the PipeWire thread.
pub struct Capture {
    /// Latest frame per captured monitor, in portal order.
    pub slots: Vec<FrameSlot>,
    pub streams: Vec<StreamInfo>,
    /// Total frames received across all streams (for fps metering).
    pub counter: Arc<AtomicU64>,
    /// Persistable token for restoring this selection without a dialog.
    pub restore_token: Option<String>,
    _thread: std::thread::JoinHandle<()>,
}

struct PortalSession {
    _proxy: Screencast,
    _session: Session<Screencast>,
    streams: Vec<PortalStream>,
    fd: OwnedFd,
    restore_token: Option<String>,
}

/// Open the portal (this shows GNOME's picker), then stream the selected
/// monitors on a dedicated thread. `start()` blocks until the picker is
/// answered.
pub fn start(multiple: bool, restore_token: Option<String>) -> Result<Capture> {
    let (tx, rx) = std::sync::mpsc::channel();
    let counter = Arc::new(AtomicU64::new(0));
    let counter_thread = counter.clone();

    let handle = std::thread::Builder::new()
        .name("pw-capture".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(Err(anyhow::anyhow!("tokio runtime: {e}")));
                    return;
                }
            };
            let session = match rt.block_on(portal_open(multiple, restore_token)) {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            };

            let node_ids: Vec<u32> = session
                .streams
                .iter()
                .map(|s| s.pipe_wire_node_id())
                .collect();
            let infos: Vec<StreamInfo> = session
                .streams
                .iter()
                .enumerate()
                .map(|(index, s)| StreamInfo {
                    index,
                    node_id: s.pipe_wire_node_id(),
                    position: s.position(),
                    size: s.size(),
                    id: s.id().map(str::to_string),
                })
                .collect();
            let slots: Vec<FrameSlot> = node_ids.iter().map(|_| Arc::new(Slot::default())).collect();
            let restore_token = session.restore_token.clone();

            if tx.send(Ok((infos.clone(), slots.clone(), restore_token))).is_err() {
                return;
            }

            if let Err(e) = run_pipewire(session, &node_ids, &slots, counter_thread) {
                eprintln!("bloqsync: capture stopped: {e:#}");
            }
            drop(rt);
        })
        .context("spawn capture thread")?;

    match rx.recv().context("capture thread died before starting")? {
        Ok((streams, slots, restore_token)) => Ok(Capture {
            slots,
            streams,
            counter,
            restore_token,
            _thread: handle,
        }),
        Err(e) => Err(e),
    }
}

async fn portal_open(multiple: bool, restore_token: Option<String>) -> Result<PortalSession> {
    let proxy = Screencast::new().await.context("create ScreenCast proxy")?;
    let session = proxy.create_session(Default::default()).await?;
    proxy
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::Hidden)
                .set_sources(BitFlags::from_flag(SourceType::Monitor))
                .set_multiple(multiple)
                .set_restore_token(restore_token.as_deref())
                .set_persist_mode(PersistMode::ExplicitlyRevoked),
        )
        .await?;
    let response = proxy
        .start(&session, None, Default::default())
        .await?
        .response()?;
    let streams = response.streams().to_vec();
    let restore_token = response.restore_token().map(str::to_string);
    if streams.is_empty() {
        anyhow::bail!("portal returned no streams");
    }
    let fd = proxy
        .open_pipe_wire_remote(&session, Default::default())
        .await?;
    Ok(PortalSession {
        _proxy: proxy,
        _session: session,
        streams,
        fd,
        restore_token,
    })
}

struct StreamUser {
    format: PixelFormat,
    width: usize,
    height: usize,
    slot: FrameSlot,
    frames: Arc<AtomicU64>,
    last_produced: u64,
}

fn run_pipewire(
    portal: PortalSession,
    node_ids: &[u32],
    slots: &[FrameSlot],
    counter: Arc<AtomicU64>,
) -> Result<()> {
    // Destructure so the portal proxy/session stay alive for the whole loop.
    let PortalSession {
        _proxy,
        _session,
        streams: _streams,
        fd,
        restore_token: _,
    } = portal;

    pw::init();
    let mainloop = pw::main_loop::MainLoopBox::new(None)?;
    let context = pw::context::ContextBox::new(mainloop.loop_(), None)?;
    let core = context.connect_fd(fd, None)?;

    let mut listeners = Vec::new();
    for (node_id, slot) in node_ids.iter().zip(slots.iter()) {
        let user = StreamUser {
            format: PixelFormat::Unknown,
            width: 0,
            height: 0,
            slot: slot.clone(),
            frames: counter.clone(),
            last_produced: 0,
        };
        let stream = pw::stream::StreamBox::new(
            &core,
            "bloqsync-capture",
            properties! {
                *pw::keys::MEDIA_TYPE => "Video",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Screen",
            },
        )?;

        listeners.push(
            stream
                .add_local_listener_with_user_data(user)
                .param_changed(|_, user, id, param| {
                    let Some(param) = param else { return };
                    if id != spa::param::ParamType::Format.as_raw() {
                        return;
                    }
                    if let Ok((mt, ms)) = spa::param::format_utils::parse_format(param) {
                        if mt == spa::param::format::MediaType::Video
                            && ms == spa::param::format::MediaSubtype::Raw
                        {
                            let mut info = spa::param::video::VideoInfoRaw::default();
                            if info.parse(param).is_ok() {
                                user.format = PixelFormat::from_spa(info.format());
                                user.width = info.size().width as usize;
                                user.height = info.size().height as usize;
                            }
                        }
                    }
                })
                .process(|stream, user| {
                    let Some(mut buffer) = stream.dequeue_buffer() else {
                        return;
                    };
                    let datas = buffer.datas_mut();
                    if datas.is_empty() || user.width == 0 {
                        return;
                    }
                    // Skip work if the consumer has not read our last frame yet.
                    // This paces capture to the engine and saves CPU.
                    if user.last_produced != 0
                        && user.slot.consumed.load(Ordering::Relaxed) < user.last_produced
                    {
                        return;
                    }
                    let d = &mut datas[0];
                    let size = d.chunk().size() as usize;
                    let raw_stride = d.chunk().stride();
                    let Some(data) = d.data() else { return };
                    let ch = user.format.channels();
                    let src_stride = if raw_stride > 0 {
                        raw_stride as usize
                    } else {
                        user.width * ch
                    };
                    let len = size.min(data.len());
                    let seq = user.frames.fetch_add(1, Ordering::Relaxed);
                    let mut frame =
                        downsample(data, src_stride, len, user.width, user.height, user.format, ch);
                    frame.seq = seq;
                    if let Ok(mut guard) = user.slot.frame.lock() {
                        *guard = Some(Arc::new(frame));
                    }
                    user.last_produced = seq;
                })
                .register()?,
        );

        let values = enum_format_pod()?;
        let mut params = [spa::pod::Pod::from_bytes(&values).unwrap()];
        stream.connect(
            spa::utils::Direction::Input,
            Some(*node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )?;
        // keep the stream + listener alive for the whole mainloop
        std::mem::forget(stream);
    }
    std::mem::forget(listeners);

    mainloop.run();
    Ok(())
}

/// Downsample a full-resolution frame to at most ~160px wide by point sampling.
/// Avoids copying ~8 MB per frame and makes border sampling cheap.
fn downsample(
    data: &[u8],
    src_stride: usize,
    len: usize,
    width: usize,
    height: usize,
    format: PixelFormat,
    ch: usize,
) -> Frame {
    const MAX_W: usize = 160;
    let scale = (width / MAX_W).max(1);
    let sw = (width / scale).max(1);
    let sh = (height / scale).max(1);
    // Rows we can safely read from the mapped chunk.
    let safe_h = if src_stride > 0 { (len / src_stride).min(height) } else { 0 };
    let mut out = vec![0u8; sw * sh * ch];

    if ch == 4 {
        // Fast path: one unaligned 32-bit load/store per sampled pixel.
        unsafe {
            let src = data.as_ptr();
            let dst = out.as_mut_ptr() as *mut u32;
            for y in 0..sh {
                let sy = (y * scale).min(safe_h.saturating_sub(1));
                if safe_h == 0 {
                    break;
                }
                let srow = src.add(sy * src_stride) as *const u32;
                let orow = y * sw;
                for x in 0..sw {
                    let sx = (x * scale).min(width - 1);
                    let v = std::ptr::read_unaligned(srow.add(sx));
                    std::ptr::write_unaligned(dst.add(orow + x), v);
                }
            }
        }
    } else {
        for y in 0..sh {
            let sy = (y * scale).min(safe_h.saturating_sub(1));
            if safe_h == 0 {
                break;
            }
            let srow = sy * src_stride;
            let orow = y * sw * ch;
            for x in 0..sw {
                let sx = (x * scale).min(width - 1);
                let sidx = srow + sx * ch;
                let oidx = orow + x * ch;
                if sidx + ch <= len && oidx + ch <= out.len() {
                    out[oidx..oidx + ch].copy_from_slice(&data[sidx..sidx + ch]);
                }
            }
        }
    }

    Frame {
        width: sw,
        height: sh,
        stride: sw * ch,
        format,
        data: out,
        seq: 0,
    }
}

fn enum_format_pod() -> Result<Vec<u8>> {
    let obj = pw::spa::pod::object!(
        pw::spa::utils::SpaTypes::ObjectParamFormat,
        pw::spa::param::ParamType::EnumFormat,
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::MediaType,
            Id,
            pw::spa::param::format::MediaType::Video
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::MediaSubtype,
            Id,
            pw::spa::param::format::MediaSubtype::Raw
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            pw::spa::param::video::VideoFormat::BGRx,
            pw::spa::param::video::VideoFormat::BGRx,
            pw::spa::param::video::VideoFormat::RGBA,
            pw::spa::param::video::VideoFormat::RGBx,
            pw::spa::param::video::VideoFormat::BGRA,
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            pw::spa::utils::Rectangle { width: 1920, height: 1080 },
            pw::spa::utils::Rectangle { width: 1, height: 1 },
            pw::spa::utils::Rectangle { width: 7680, height: 4320 }
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            pw::spa::utils::Fraction { num: 60, denom: 1 },
            pw::spa::utils::Fraction { num: 0, denom: 1 },
            pw::spa::utils::Fraction { num: 240, denom: 1 }
        ),
    );
    let values = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .map_err(|e| anyhow::anyhow!("serialize pod: {e:?}"))?
    .0
    .into_inner();
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_rgb(w: usize, h: usize, px: impl Fn(usize, usize) -> [u8; 3]) -> Frame {
        let mut data = vec![0u8; w * h * 3];
        for y in 0..h {
            for x in 0..w {
                let p = px(x, y);
                let i = y * (w * 3) + x * 3;
                data[i..i + 3].copy_from_slice(&p);
            }
        }
        Frame { width: w, height: h, stride: w * 3, format: PixelFormat::Rgb, data, seq: 0 }
    }

    #[test]
    fn channels_match_pixel_layout() {
        assert_eq!(PixelFormat::Rgb.channels(), 3);
        assert_eq!(PixelFormat::Bgr.channels(), 3);
        for f in [
            PixelFormat::Rgba,
            PixelFormat::Bgra,
            PixelFormat::Rgbx,
            PixelFormat::Bgrx,
            PixelFormat::Unknown,
        ] {
            assert_eq!(f.channels(), 4, "{f:?}");
        }
    }

    #[test]
    fn pixel_reorders_bgr_to_rgb() {
        let bgr = Frame {
            width: 1,
            height: 1,
            stride: 3,
            format: PixelFormat::Bgr,
            data: vec![10, 20, 30],
            seq: 0,
        };
        assert_eq!(bgr.pixel(0, 0), [30, 20, 10]);
        let rgb = Frame { format: PixelFormat::Rgb, ..bgr };
        assert_eq!(rgb.pixel(0, 0), [10, 20, 30]);
    }

    #[test]
    fn pixel_out_of_range_is_black() {
        let f = frame_rgb(2, 2, |x, y| [x as u8, y as u8, 9]);
        assert_eq!(f.pixel(1, 1), [1, 1, 9]);
        assert_eq!(f.pixel(2, 1), [0, 0, 0]); // x beyond the row
        assert_eq!(f.pixel(0, 5), [0, 0, 0]); // y beyond the data
    }

    #[test]
    fn downsample_is_identity_when_already_small() {
        let data: Vec<u8> = (0..(4 * 2 * 3)).map(|i| i as u8).collect();
        let f = downsample(&data, 12, data.len(), 4, 2, PixelFormat::Rgb, 3);
        assert_eq!((f.width, f.height, f.stride), (4, 2, 12));
        assert_eq!(f.data, data);
    }

    #[test]
    fn downsample_caps_width_at_160() {
        let data = vec![7u8; 320 * 2 * 3];
        let f = downsample(&data, 320 * 3, data.len(), 320, 2, PixelFormat::Rgb, 3);
        assert_eq!((f.width, f.height), (160, 1));
        assert_eq!(f.data.len(), 160 * 3);
        assert!(f.data.iter().all(|&b| b == 7));
    }

    #[test]
    fn downsample_fast_path_copies_four_bytes() {
        let data: Vec<u8> = (0..(2 * 2 * 4)).map(|i| i as u8).collect();
        let f = downsample(&data, 8, data.len(), 2, 2, PixelFormat::Bgrx, 4);
        assert_eq!((f.width, f.height, f.stride), (2, 2, 8));
        assert_eq!(f.data, data);
    }

    #[test]
    fn downsample_clamps_to_available_rows() {
        // Stride claims four rows but only one row's worth of bytes is mapped;
        // the sampler must never read past `len` and clamps to the last row.
        let data = vec![5u8; 12];
        let f = downsample(&data, 12, data.len(), 4, 4, PixelFormat::Rgb, 3);
        assert_eq!((f.width, f.height), (4, 4));
        assert!(f.data.iter().all(|&b| b == 5));
    }

    #[test]
    fn downsample_zero_size_does_not_panic() {
        let f = downsample(&[], 0, 0, 0, 0, PixelFormat::Unknown, 4);
        assert_eq!(f.data.len(), 4);
        assert!(f.data.iter().all(|&b| b == 0));
    }
}

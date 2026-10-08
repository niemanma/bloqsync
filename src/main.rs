use anyhow::{bail, Result};
use bloqsync::device::{enumerate, Device};
use bloqsync::protocol::Rgb;
use clap::{Parser, Subcommand};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "bloqsync", about = "ROBOBLOQ SyncLight controller / test CLI")]
struct Cli {
    /// Device path or stable id (defaults to the first bar found).
    #[arg(long, global = true)]
    device: Option<String>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List connected bars.
    List,
    /// Show firmware / LED count / uuid.
    Info,
    /// Fill the whole strip with one colour.
    Fill { r: u8, g: u8, b: u8 },
    /// Turn the strip off.
    Off,
    /// Set brightness (0-255).
    Brightness { value: u8 },
    /// Static rainbow gradient (one colour per LED).
    Gradient,
    /// Three zones: first/middle/last.
    Pattern,
    /// Animated moving gradient for N seconds.
    Stream {
        #[arg(default_value_t = 10)]
        seconds: u64,
    },
    /// Measure achievable frame rate for N seconds.
    Bench {
        #[arg(default_value_t = 5)]
        seconds: u64,
    },
    /// Test the Wayland/PipeWire screen capture (shows a picker dialog).
    CaptureTest {
        #[arg(default_value_t = 6)]
        seconds: u64,
    },
    /// Full screen-sync: capture a monitor and drive the bar.
    Sync {
        #[arg(default_value_t = 20)]
        seconds: u64,
        #[arg(long, default_value_t = 45)]
        fps: u32,
        #[arg(long, default_value_t = 0.55)]
        smooth: f32,
        /// Mirror the strip (fixes swapped left/right).
        #[arg(long)]
        reverse: bool,
    },
    /// Show 3 colour zones to determine physical orientation.
    Calibrate {
        #[arg(default_value_t = 10)]
        seconds: u64,
    },
    /// Capture once, then stream that frozen frame continuously (flicker test).
    Freeze {
        #[arg(default_value_t = 15)]
        seconds: u64,
        #[arg(long, default_value_t = 60)]
        fps: u32,
    },
    /// Print audio features (cinema mode) for N seconds.
    AudioTest {
        #[arg(default_value_t = 15)]
        seconds: u64,
    },
    /// Audio-reactive "cinema" light (works for DRM/Netflix).
    AudioSync {
        #[arg(default_value_t = 60)]
        seconds: u64,
        #[arg(long, default_value_t = 1.0)]
        sensitivity: f32,
        #[arg(long, default_value_t = 0.7)]
        brightness: f32,
    },
}

fn pick(target: &Option<String>) -> Result<Device> {
    let infos = enumerate();
    if infos.is_empty() {
        bail!("no SyncLight bar found (is it plugged in and do you have hidraw access?)");
    }
    let info = match target {
        Some(t) => infos
            .iter()
            .find(|i| &i.path == t || &i.id == t)
            .ok_or_else(|| anyhow::anyhow!("device '{t}' not found"))?,
        None => &infos[0],
    };
    Device::open(info)
}

fn gradient(n: usize) -> Vec<Rgb> {
    (0..n)
        .map(|i| {
            let t = (i as f32 / n.max(1) as f32 * 6.0) % 6.0;
            let x = (t.fract() * 255.0) as u8;
            let q = 255 - x;
            let seg = t as usize;
            match seg {
                0 => [255, x, 0],
                1 => [q, 255, 0],
                2 => [0, 255, x],
                3 => [0, q, 255],
                4 => [x, 0, 255],
                _ => [255, 0, q],
            }
        })
        .collect()
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::List => {
            for i in enumerate() {
                println!(
                    "{}  id={} iface={} name={} serial={}",
                    i.path, i.id, i.interface, i.name, i.serial
                );
            }
        }
        Cmd::Info => {
            let d = pick(&cli.device)?;
            println!(
                "{}  id={}  firmware={}  leds={}  uuid={}  display_size={}  device_id={}",
                d.info.path,
                d.info.id,
                d.firmware,
                d.led_count,
                d.uuid,
                d.display_size,
                d.device_id
            );
        }
        Cmd::Fill { r, g, b } => {
            let d = pick(&cli.device)?;
            d.set_persistent_color([r, g, b])?;
        }
        Cmd::Off => {
            let d = pick(&cli.device)?;
            d.turn_off()?;
        }
        Cmd::Brightness { value } => {
            let d = pick(&cli.device)?;
            d.set_brightness(value)?;
        }
        Cmd::Gradient => {
            let d = pick(&cli.device)?;
            let colors = gradient(d.led_count);
            d.send_colors(&colors)?;
            std::thread::sleep(Duration::from_millis(50));
        }
        Cmd::Pattern => {
            let d = pick(&cli.device)?;
            let n = d.led_count;
            let colors: Vec<Rgb> = (0..n)
                .map(|i| {
                    if i < n / 3 {
                        [255, 0, 0]
                    } else if i < 2 * n / 3 {
                        [0, 255, 0]
                    } else {
                        [0, 0, 255]
                    }
                })
                .collect();
            d.send_colors(&colors)?;
            std::thread::sleep(Duration::from_millis(50));
        }
        Cmd::Stream { seconds } => {
            let d = pick(&cli.device)?;
            let n = d.led_count;
            let start = Instant::now();
            let mut offset = 0f32;
            while start.elapsed() < Duration::from_secs(seconds) {
                let colors: Vec<Rgb> = (0..n)
                    .map(|i| {
                        let t = ((i as f32 / n as f32) + offset) * 6.0 % 6.0;
                        let x = (t.fract() * 255.0) as u8;
                        let q = 255 - x;
                        match t as usize {
                            0 => [255, x, 0],
                            1 => [q, 255, 0],
                            2 => [0, 255, x],
                            3 => [0, q, 255],
                            4 => [x, 0, 255],
                            _ => [255, 0, q],
                        }
                    })
                    .collect();
                d.send_colors(&colors)?;
                offset += 0.02;
                std::thread::sleep(Duration::from_millis(8));
            }
            d.set_persistent_color([255, 200, 100])?;
        }
        Cmd::Bench { seconds } => {
            let d = pick(&cli.device)?;
            let n = d.led_count;
            let colors = gradient(n);
            let start = Instant::now();
            let mut count = 0u64;
            while start.elapsed() < Duration::from_secs(seconds) {
                d.send_colors(&colors)?;
                count += 1;
            }
            let dt = start.elapsed().as_secs_f64();
            println!(
                "{} frames in {:.2}s = {:.1} FPS ({} LEDs)",
                count,
                dt,
                count as f64 / dt,
                n
            );
        }
        Cmd::CaptureTest { seconds } => {
            use bloqsync::sampling::{sample_border, Layout};
            use std::sync::atomic::Ordering;
            println!("ScreenCast anfordern – bitte den Monitor im Dialog auswählen …");
            let cap = bloqsync::capture::start(false, None)?;
            println!(
                "Capture läuft: {} Stream(s), nodes={:?}",
                cap.slots.len(),
                cap.streams.iter().map(|s| s.node_id).collect::<Vec<_>>()
            );
            let layout = Layout::default();
            let start = Instant::now();
            let mut last = 0u64;
            while start.elapsed() < Duration::from_secs(seconds) {
                std::thread::sleep(Duration::from_millis(500));
                let c = cap.counter.load(Ordering::Relaxed);
                if let Some(slot) = cap.slots.first() {
                    if let Some(f) = slot.frame.lock().unwrap().clone() {
                        let cols = sample_border(&f, &layout);
                        println!(
                            "{:>4.1}s  frames={:<6} (+{:<4}) {}x{} {:?}  first_zone={:?}",
                            start.elapsed().as_secs_f32(),
                            c,
                            c - last,
                            f.width,
                            f.height,
                            f.format,
                            cols.first()
                        );
                    }
                }
                last = c;
            }
            let dt = start.elapsed().as_secs_f64();
            println!(
                "~{:.1} FPS über {} Stream(s)",
                cap.counter.load(Ordering::Relaxed) as f64 / dt,
                cap.slots.len()
            );
        }
        Cmd::Sync {
            seconds,
            fps,
            smooth,
            reverse,
        } => {
            use bloqsync::engine::{spawn, SyncConfig};
            use std::sync::atomic::Ordering;
            let device = Arc::new(pick(&cli.device)?);
            println!("ScreenCast anfordern – bitte Monitor auswählen …");
            let cap = bloqsync::capture::start(false, None)?;
            let slot = cap
                .slots
                .first()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("no capture stream"))?;
            let cfg = SyncConfig {
                fps,
                smoothing: smooth,
                reverse,
                ..Default::default()
            };
            let sync = spawn(device.clone(), slot, cfg)?;
            println!("Sync läuft ({}s, {} fps, smooth={})", seconds, fps, smooth);
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(seconds) {
                std::thread::sleep(Duration::from_secs(1));
                println!(
                    "  {:>3}s  captures={}  sent={}",
                    start.elapsed().as_secs(),
                    cap.counter.load(Ordering::Relaxed),
                    sync.sent.load(Ordering::Relaxed)
                );
            }
            sync.stop();
            device.set_persistent_color([255, 200, 100])?;
            println!("fertig");
        }
        Cmd::Calibrate { seconds } => {
            use bloqsync::protocol::Rgb;
            let d = pick(&cli.device)?;
            let n = d.led_count;
            let colors: Vec<Rgb> = (0..n)
                .map(|i| {
                    if i < n / 3 {
                        [255, 0, 0]
                    } else if i < 2 * n / 3 {
                        [0, 255, 0]
                    } else {
                        [0, 0, 255]
                    }
                })
                .collect();
            println!(
                "KALIBRIERUNG: LED-Software 1-{} = ROT, {}-{} = GRÜN, {}-{} = BLAU.",
                n / 3,
                n / 3 + 1,
                2 * n / 3,
                2 * n / 3 + 1,
                n
            );
            println!("Bitte schauen: liegt ROT auf der linken oder rechten Seite?");
            let period = Duration::from_millis(33);
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(seconds) {
                let t = Instant::now();
                d.send_colors_paced(&colors, Duration::from_millis(3), 0)?;
                let e = t.elapsed();
                if e < period {
                    std::thread::sleep(period - e);
                }
            }
            d.set_persistent_color([255, 200, 100])?;
        }
        Cmd::Freeze { seconds, fps } => {
            use bloqsync::sampling::{sample_border, Layout};
            let device = Arc::new(pick(&cli.device)?);
            println!("ScreenCast anfordern – bitte Monitor auswählen …");
            let cap = bloqsync::capture::start(false, None)?;
            let slot = cap.slots.first().cloned().unwrap();
            let frame = loop {
                if let Some(f) = slot.frame.lock().unwrap().clone() {
                    break f;
                }
                std::thread::sleep(Duration::from_millis(20));
            };
            let cols = sample_border(&frame, &Layout::default());
            println!("Frozen frame {}x{}, streame {} LEDs @ {} fps", frame.width, frame.height, cols.len(), fps);
            let period = Duration::from_secs_f64(1.0 / fps as f64);
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(seconds) {
                let t = Instant::now();
                device.send_colors_paced(&cols, Duration::from_millis(3), 0)?;
                let e = t.elapsed();
                if e < period {
                    std::thread::sleep(period - e);
                }
            }
            device.set_persistent_color([255, 200, 100])?;
            println!("fertig");
        }
        Cmd::AudioTest { seconds } => {
            let h = bloqsync::audio::start(None)?;
            println!("audio source: {}", h.source);
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(seconds) {
                std::thread::sleep(Duration::from_millis(200));
                if let Some(s) = *h.spectrum.lock().unwrap() {
                    println!(
                        "e={:.2} centroid={:.2} flat={:.2} onset={:.2} stereo={:+.2} lvl={:.3} bands=[{:.2} {:.2} {:.2} {:.2} {:.2} {:.2}]",
                        s.energy, s.centroid, s.flatness, s.onset, s.stereo, s.rms,
                        s.bands[0], s.bands[1], s.bands[2], s.bands[3], s.bands[4], s.bands[5]
                    );
                }
            }
            h.stop();
        }
        Cmd::AudioSync {
            seconds,
            sensitivity,
            brightness,
        } => {
            use bloqsync::cinema::{Cinema, CinemaParams};
            let d = pick(&cli.device)?;
            let h = bloqsync::audio::start(None)?;
            println!("Cinema-Sync auf {} (source {})", d.info.path, h.source);
            let mut cinema = Cinema::new();
            let params = CinemaParams {
                sensitivity,
                master: brightness,
                ..Default::default()
            };
            let period = Duration::from_millis(20);
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(seconds) {
                let t = Instant::now();
                if let Some(s) = *h.spectrum.lock().unwrap() {
                    let cols = cinema.render(&s, d.led_count, &params);
                    d.send_colors_paced(&cols, Duration::from_millis(3), 8)?;
                }
                let e = t.elapsed();
                if e < period {
                    std::thread::sleep(period - e);
                }
            }
            h.stop();
            d.set_persistent_color([255, 200, 100])?;
        }
    }
    Ok(())
}


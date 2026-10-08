#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bloqsync::audio::AudioHandle;
use bloqsync::capture::{Capture, StreamInfo};
use bloqsync::cinema::{Cinema, CinemaParams};
use bloqsync::device::{enumerate, find_by_identity, Device};
use bloqsync::engine::{spawn, SyncConfig, SyncHandle};
use bloqsync::filters::FilterKind;
use bloqsync::sampling::Layout;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};

mod monitors;

/// Append a line to ~/.cache/bloqsync/bloqsync.log (and stderr), so autostart
/// runs can be diagnosed after a reboot.
fn log(msg: &str) {
    if let Some(home) = std::env::var_os("HOME") {
        let dir = std::path::Path::new(&home).join(".cache/bloqsync");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("bloqsync.log"))
        {
            use std::io::Write;
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "[{secs}] {msg}");
        }
    }
    eprintln!("{msg}");
}

struct BarRuntime {
    device: Arc<Device>,
    sync: SyncHandle,
}

struct CinemaRuntime {
    audio: AudioHandle,
    running: Arc<std::sync::atomic::AtomicBool>,
    devices: Vec<Arc<Device>>,
    thread: std::thread::JoinHandle<()>,
}

impl CinemaRuntime {
    fn stop(self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.audio.stop();
        let _ = self.thread.join();
        for d in &self.devices {
            let _ = d.set_persistent_color([255, 200, 100]);
        }
    }
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct Ident {
    uuid: String,
    leds: usize,
    firmware: String,
}

#[derive(Default)]
struct AppState {
    capture: Mutex<Option<Arc<Capture>>>,
    bars: Mutex<HashMap<String, BarRuntime>>,
    last_signature: Mutex<Option<String>>,
    capture_signature: Mutex<Option<String>>,
    ident_cache: Mutex<HashMap<String, Ident>>,
    /// True while the user shows a static colour / cinema (screen sync paused).
    sync_paused: Mutex<bool>,
    cinema: Mutex<Option<CinemaRuntime>>,
}

#[derive(serde::Serialize)]
struct DevDto {
    path: String,
    id: String,
    leds: usize,
    firmware: String,
    uuid: String,
}

#[tauri::command]
fn list_devices(state: State<AppState>) -> Vec<DevDto> {
    let mut cache = state.ident_cache.lock().unwrap();
    let list: Vec<DevDto> = enumerate()
        .into_iter()
        .map(|info| match Device::open(&info) {
            Ok(d) => {
                cache.insert(
                    d.info.id.clone(),
                    Ident {
                        uuid: d.uuid.clone(),
                        leds: d.led_count,
                        firmware: d.firmware.clone(),
                    },
                );
                DevDto {
                    path: d.info.path.clone(),
                    id: d.info.id.clone(),
                    leds: d.led_count,
                    firmware: d.firmware.clone(),
                    uuid: d.uuid.clone(),
                }
            }
            Err(_) => {
                // Device is busy (e.g. streaming) - fall back to the cache so
                // the UI keeps the same identity (no duplicate rows).
                let c = cache.get(&info.id).cloned().unwrap_or_default();
                DevDto {
                    path: info.path.clone(),
                    id: info.id.clone(),
                    leds: c.leds,
                    firmware: c.firmware,
                    uuid: c.uuid,
                }
            }
        })
        .collect();
    drop(cache);
    persist_identities(&state);
    list
}

/// Open the ScreenCast session (multiple monitors). Uses the stored restore
/// token if present, so after the first selection no dialog is shown.
#[tauri::command]
fn open_capture(state: State<AppState>) -> Result<Vec<StreamInfo>, String> {
    open_capture_inner(&state)
}

fn open_capture_inner(state: &AppState) -> Result<Vec<StreamInfo>, String> {
    open_capture_with(state, None)
}

/// Current capture streams without opening anything (UI polling).
#[tauri::command]
fn capture_info(state: State<AppState>) -> Vec<StreamInfo> {
    state
        .capture
        .lock()
        .unwrap()
        .as_ref()
        .map(|c| c.streams.clone())
        .unwrap_or_default()
}

fn open_capture_with(
    state: &AppState,
    token_override: Option<String>,
) -> Result<Vec<StreamInfo>, String> {
    let mut cap = state.capture.lock().unwrap();
    if cap.is_none() {
        let sig = monitors::signature();
        let token = token_override.or_else(|| {
            let cfg = read_config();
            let mut t = cfg.active_profile(&sig).and_then(|p| p.restore_token.clone());
            if t.is_none() {
                t = cfg.restore_token.clone();
            }
            t
        });
        let c = bloqsync::capture::start(true, token).map_err(|e| e.to_string())?;
        log(&format!(
            "capture opened: {} stream(s) {:?}, token={}, setup={}",
            c.streams.len(),
            c.streams.iter().map(|s| (s.index, s.size)).collect::<Vec<_>>(),
            c.restore_token.is_some(),
            sig
        ));
        if let Some(tok) = &c.restore_token {
            let mut cfg = read_config();
            cfg.upsert_profile_token(&sig, tok);
            cfg.restore_token = Some(tok.clone());
            write_config(&cfg);
        }
        *state.capture_signature.lock().unwrap() = Some(sig);
        *cap = Some(Arc::new(c));
    }
    Ok(cap.as_ref().unwrap().streams.clone())
}

#[tauri::command]
fn close_capture(state: State<AppState>) -> Result<(), String> {
    stop_all_inner(&state);
    *state.capture.lock().unwrap() = None;
    *state.capture_signature.lock().unwrap() = None;
    Ok(())
}

fn stop_all_inner(state: &AppState) {
    let mut bars = state.bars.lock().unwrap();
    for (_, b) in bars.drain() {
        b.sync.stop();
        let _ = b.device.set_persistent_color([255, 200, 100]);
    }
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
fn start_bar(
    state: State<AppState>,
    bar_path: String,
    stream_index: usize,
    fps: u32,
    smooth: f32,
    reverse: bool,
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
    brightness: Option<u8>,
    filter: Option<String>,
    filter_strength: Option<u8>,
    max_frames: Option<usize>,
) -> Result<(), String> {
    cinema_stop_inner(&state);
    *state.sync_paused.lock().unwrap() = false;
    start_bar_inner(
        &state,
        &bar_path,
        stream_index,
        fps,
        smooth,
        reverse,
        Layout { left, top, right, bottom },
        brightness,
        filter.as_deref(),
        filter_strength.unwrap_or(8),
        max_frames.unwrap_or(0),
    )
}

#[allow(clippy::too_many_arguments)]
fn start_bar_inner(
    state: &AppState,
    bar_path: &str,
    stream_index: usize,
    fps: u32,
    smooth: f32,
    reverse: bool,
    layout: Layout,
    brightness: Option<u8>,
    filter: Option<&str>,
    filter_strength: u8,
    max_frames: usize,
) -> Result<(), String> {
    let cap = state
        .capture
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "Aufnahme nicht gestartet".to_string())?;
    let slot = cap
        .slots
        .get(stream_index)
        .cloned()
        .ok_or_else(|| format!("Stream {stream_index} existiert nicht"))?;

    if let Some(prev) = state.bars.lock().unwrap().remove(bar_path) {
        prev.sync.stop();
    }

    let info = find_by_identity(bar_path)
        .ok_or_else(|| "Leiste nicht gefunden".to_string())?;
    let device = Arc::new(Device::open(&info).map_err(|e| e.to_string())?);
    state.ident_cache.lock().unwrap().insert(
        device.info.id.clone(),
        Ident {
            uuid: device.uuid.clone(),
            leds: device.led_count,
            firmware: device.firmware.clone(),
        },
    );
    persist_identities(state);
    if let Some(b) = brightness {
        let _ = device.set_brightness(b);
    }

    let cfg = SyncConfig {
        fps,
        smoothing: smooth,
        reverse,
        filter: match filter {
            Some("deadband") => FilterKind::Deadband,
            Some("quantize") => FilterKind::Quantize,
            Some("quantize_smooth") => FilterKind::QuantizeSmooth,
            Some("smooth") => FilterKind::Smooth,
            Some("median3") => FilterKind::Median3,
            Some("median5") => FilterKind::Median5,
            Some("mean4") => FilterKind::Mean4,
            Some("test") => FilterKind::Test,
            _ => FilterKind::None,
        },
        filter_strength,
        max_frames,
        layout,
        ..Default::default()
    };
    let handle = spawn(device.clone(), slot, cfg).map_err(|e| e.to_string())?;
    state
        .bars
        .lock()
        .unwrap()
        .insert(bar_path.to_string(), BarRuntime { device, sync: handle });
    // Remember this mapping for the current monitor setup.
    let sig = monitors::signature();
    let mut c = read_config();
    c.upsert_profile_bar(
        &sig,
        BarConfig {
            bar_path: bar_path.to_string(),
            stream_index,
            reverse,
        },
    );
    write_config(&c);
    Ok(())
}

#[tauri::command]
fn stop_bar(state: State<AppState>, bar_path: String) -> Result<(), String> {
    if let Some(b) = state.bars.lock().unwrap().remove(&bar_path) {
        b.sync.stop();
        let _ = b.device.set_persistent_color([255, 200, 100]);
    }
    Ok(())
}

#[tauri::command]
fn stop_all(state: State<AppState>) -> Result<(), String> {
    stop_all_inner(&state);
    Ok(())
}

#[derive(serde::Serialize)]
struct BarStatus {
    bar: String,
    running: bool,
    sent: u64,
}

#[derive(serde::Serialize)]
struct StatusDto {
    bars: Vec<BarStatus>,
    paused: bool,
    cinema: bool,
}

#[tauri::command]
fn status(state: State<AppState>) -> StatusDto {
    let paused = *state.sync_paused.lock().unwrap();
    let cinema = state.cinema.lock().unwrap().is_some();
    let bars = state
        .bars
        .lock()
        .unwrap()
        .iter()
        .map(|(path, b)| BarStatus {
            bar: path.clone(),
            running: b.sync.is_running(),
            sent: b.sync.sent.load(std::sync::atomic::Ordering::Relaxed),
        })
        .collect();
    StatusDto { bars, paused, cinema }
}

#[tauri::command]
fn set_brightness(state: State<AppState>, value: u8) -> Result<(), String> {
    for b in state.bars.lock().unwrap().values() {
        let _ = b.device.set_brightness(value);
    }
    Ok(())
}

/// Show a static colour on all bars and pause the sync so it is not
/// overwritten. Restart with [`resume_sync`].
#[tauri::command]
fn set_all_color(state: State<AppState>, r: u8, g: u8, b: u8) -> Result<(), String> {
    *state.sync_paused.lock().unwrap() = true;
    stop_all_inner(&state);
    for info in enumerate() {
        if let Ok(d) = Device::open(&info) {
            let _ = d.set_persistent_color([r, g, b]);
        }
    }
    Ok(())
}

/// Resume screen sync after a static colour was shown.
#[tauri::command]
fn resume_sync(state: State<AppState>) -> Result<(), String> {
    cinema_stop_inner(&state);
    *state.sync_paused.lock().unwrap() = false;
    autostart_run_inner(&state)
}

#[tauri::command]
fn cinema_start(
    state: State<AppState>,
    sensitivity: Option<f32>,
    brightness: Option<f32>,
    r: Option<u8>,
    g: Option<u8>,
    b: Option<u8>,
    onset: Option<f32>,
    floor: Option<f32>,
    smooth: Option<f32>,
    contrast: Option<f32>,
) -> Result<(), String> {
    cinema_start_inner(
        &state,
        sensitivity.unwrap_or(1.0),
        brightness.unwrap_or(0.7),
        [r.unwrap_or(255), g.unwrap_or(160), b.unwrap_or(60)],
        onset.unwrap_or(0.0),
        floor.unwrap_or(0.35),
        smooth.unwrap_or(0.6),
        contrast.unwrap_or(0.5),
    )
}

fn cinema_stop_inner(state: &AppState) {
    if let Some(c) = state.cinema.lock().unwrap().take() {
        c.stop();
    }
}

#[tauri::command]
fn cinema_stop(state: State<AppState>) -> Result<(), String> {
    cinema_stop_inner(&state);
    Ok(())
}

/// Audio-reactive "cinema" light for DRM content (Netflix etc.).
fn cinema_start_inner(
    state: &AppState,
    sensitivity: f32,
    brightness: f32,
    base: [u8; 3],
    onset_gain: f32,
    floor: f32,
    smooth: f32,
    contrast: f32,
) -> Result<(), String> {
    cinema_stop_inner(state);
    *state.sync_paused.lock().unwrap() = true;
    stop_all_inner(state);
    // Free the screen capture while in cinema mode.
    *state.capture.lock().unwrap() = None;
    *state.capture_signature.lock().unwrap() = None;

    let audio = bloqsync::audio::start(None).map_err(|e| e.to_string())?;
    let spectrum = audio.spectrum.clone();
    let mut devices = Vec::new();
    for info in enumerate() {
        if let Ok(d) = Device::open(&info) {
            devices.push(Arc::new(d));
        }
    }
    if devices.is_empty() {
        return Err("keine Leiste gefunden".into());
    }
    log(&format!("cinema started on {} bar(s), source {}", devices.len(), audio.source));
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let running_t = running.clone();
    let params = CinemaParams {
        base,
        sensitivity,
        master: brightness,
        onset_gain,
        floor,
        smooth_secs: smooth,
        contrast_gain: contrast,
        ..Default::default()
    };
    let devices_thread = devices.clone();
    let thread = std::thread::Builder::new()
        .name("bloqsync-cinema".into())
        .spawn(move || {
            let mut states: Vec<Cinema> = (0..devices_thread.len()).map(|_| Cinema::new()).collect();
            let period = Duration::from_millis(20);
            while running_t.load(std::sync::atomic::Ordering::Relaxed) {
                let t = Instant::now();
                let s = spectrum.lock().unwrap().unwrap_or_default();
                for (i, d) in devices_thread.iter().enumerate() {
                    let cols = states[i].render(&s, d.led_count, &params);
                    let _ = d.send_colors_paced(&cols, Duration::from_millis(3), 8);
                }
                let e = t.elapsed();
                if e < period {
                    std::thread::sleep(period - e);
                }
            }
        })
        .map_err(|e| e.to_string())?;
    *state.cinema.lock().unwrap() = Some(CinemaRuntime {
        audio,
        running,
        devices,
        thread,
    });
    Ok(())
}

// ── Config ──────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
struct BarConfig {
    bar_path: String,
    stream_index: usize,
    reverse: bool,
}

/// A saved mapping for one monitor setup (identified by `signature`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
struct Profile {
    signature: String,
    #[serde(default)]
    restore_token: Option<String>,
    #[serde(default)]
    bars: Vec<BarConfig>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Config {
    bars: Vec<BarConfig>,
    fps: u32,
    smooth: f32,
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
    brightness: u8,
    #[serde(default = "default_filter")]
    filter: String,
    #[serde(default = "default_filter_strength")]
    filter_strength: u8,
    #[serde(default = "default_max_frames")]
    max_frames: u32,
    #[serde(default)]
    restore_token: Option<String>,
    #[serde(default)]
    autostart: bool,
    #[serde(default)]
    autostart_sync: bool,
    /// Per monitor-setup profiles (auto-created, auto-restored).
    #[serde(default)]
    profiles: Vec<Profile>,
    /// Persistent mapping USB-port id -> device UUID + info.
    #[serde(default)]
    bar_uuids: std::collections::HashMap<String, Ident>,
    #[serde(default = "default_custom_color")]
    custom_color: String,
    #[serde(default = "default_cinema_color")]
    cinema_color: String,
    #[serde(default = "default_cin_sens")]
    cinema_sensitivity: f32,
    #[serde(default = "default_cin_bri")]
    cinema_brightness: f32,
    #[serde(default = "default_cin_floor")]
    cinema_floor: f32,
    #[serde(default = "default_cin_smooth")]
    cinema_smooth: f32,
    #[serde(default = "default_cin_contrast")]
    cinema_contrast: f32,
    #[serde(default)]
    cinema_pulse: f32,
}

fn default_custom_color() -> String {
    "#ff8800".to_string()
}
fn default_cinema_color() -> String {
    "#5a2882".to_string()
}
fn default_cin_sens() -> f32 {
    1.0
}
fn default_cin_bri() -> f32 {
    0.7
}
fn default_cin_floor() -> f32 {
    0.35
}
fn default_cin_smooth() -> f32 {
    0.6
}
fn default_cin_contrast() -> f32 {
    1.0
}

fn default_filter() -> String {
    "none".to_string()
}
fn default_filter_strength() -> u8 {
    8
}
fn default_max_frames() -> u32 {
    0
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bars: Vec::new(),
            fps: 24,
            smooth: 0.22,
            left: 18,
            top: 18,
            right: 18,
            bottom: 0,
            brightness: 200,
            filter: default_filter(),
            filter_strength: 8,
            max_frames: 0,
            restore_token: None,
            autostart: false,
            autostart_sync: false,
            profiles: Vec::new(),
            bar_uuids: std::collections::HashMap::new(),
            custom_color: default_custom_color(),
            cinema_color: default_cinema_color(),
            cinema_sensitivity: 1.0,
            cinema_brightness: 0.7,
            cinema_floor: 0.35,
            cinema_smooth: 0.6,
            cinema_contrast: 1.0,
            cinema_pulse: 0.0,
        }
    }
}

impl Config {
    fn persist_uuids(&mut self, cache: &std::collections::HashMap<String, Ident>) {
        for (id, ident) in cache {
            if !ident.uuid.is_empty() {
                self.bar_uuids.insert(id.clone(), ident.clone());
            }
        }
    }
}

/// Persist the in-memory device identities into the config.
fn persist_identities(state: &AppState) {
    let snap = state.ident_cache.lock().unwrap().clone();
    let mut cfg = read_config();
    cfg.persist_uuids(&snap);
    write_config(&cfg);
}

impl Config {
    fn active_profile(&self, sig: &str) -> Option<&Profile> {
        if sig.is_empty() {
            return None;
        }
        self.profiles.iter().find(|p| p.signature == sig)
    }

    fn profile_mut(&mut self, sig: &str) -> &mut Profile {
        if !self.profiles.iter().any(|p| p.signature == sig) {
            self.profiles.push(Profile {
                signature: sig.to_string(),
                restore_token: None,
                bars: Vec::new(),
            });
        }
        self.profiles
            .iter_mut()
            .find(|p| p.signature == sig)
            .unwrap()
    }

    fn upsert_profile_token(&mut self, sig: &str, token: &str) {
        if sig.is_empty() {
            return;
        }
        self.profile_mut(sig).restore_token = Some(token.to_string());
    }

    fn upsert_profile_bar(&mut self, sig: &str, bar: BarConfig) {
        if sig.is_empty() {
            return;
        }
        let p = self.profile_mut(sig);
        if let Some(b) = p.bars.iter_mut().find(|b| b.bar_path == bar.bar_path) {
            *b = bar;
        } else {
            p.bars.push(bar);
        }
    }

    fn upsert_profile_bars(&mut self, sig: &str, bars: &[BarConfig]) {
        if sig.is_empty() {
            return;
        }
        self.profile_mut(sig).bars = bars.to_vec();
    }
}

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".config/bloqsync/config.json")
}

fn read_config() -> Config {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_config(cfg: &Config) {
    if let Some(dir) = config_path().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(config_path(), s);
    }
}

#[tauri::command]
fn get_config() -> Config {
    read_config()
}

#[tauri::command]
fn save_config(mut cfg: Config) {
    let existing = read_config();
    // Preserve the restore token (managed by Rust, not the UI).
    if cfg.restore_token.is_none() {
        cfg.restore_token = existing.restore_token.clone();
    }
    // Preserve profiles (the UI does not send them).
    if cfg.profiles.is_empty() {
        cfg.profiles = existing.profiles.clone();
    }
    // Preserve configured bars that are not currently reported by the UI
    // (e.g. a bar that is temporarily unplugged or on another port).
    for b in existing.bars.clone() {
        if !cfg.bars.iter().any(|n| n.bar_path == b.bar_path) {
            cfg.bars.push(b);
        }
    }
    // Mirror the current mapping into the profile for the active setup.
    let sig = monitors::signature();
    let bars = cfg.bars.clone();
    cfg.upsert_profile_bars(&sig, &bars);
    write_config(&cfg);
}

// ── Autostart (login) ───────────────────────────────────────────────

fn autostart_file() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".config/autostart/bloqsync.desktop")
}

#[tauri::command]
fn autostart_enabled() -> bool {
    autostart_file().exists()
}

#[tauri::command]
fn set_autostart(enabled: bool) -> Result<(), String> {
    let file = autostart_file();
    if enabled {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let contents = format!(
            "[Desktop Entry]\nType=Application\nName=bloqsync\nComment=Ambient light sync\nExec={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        );
        std::fs::write(&file, contents).map_err(|e| e.to_string())?;
    } else if file.exists() {
        std::fs::remove_file(&file).map_err(|e| e.to_string())?;
    }
    let mut cfg = read_config();
    cfg.autostart = enabled;
    write_config(&cfg);
    Ok(())
}

/// Auto-start the configured sync (used at launch and on hotplug).
#[tauri::command]
fn autostart_run(state: State<AppState>) -> Result<(), String> {
    autostart_run_inner(&state)
}

fn autostart_run_inner(state: &AppState) -> Result<(), String> {
    let cfg = read_config();
    let sig = monitors::signature();
    let prof = match cfg.active_profile(&sig) {
        Some(p) => p.clone(),
        None => {
            log(&format!("autostart: no profile for setup {sig} - waiting"));
            return Ok(());
        }
    };
    let (token, bars) = (prof.restore_token.clone(), prof.bars.clone());
    if bars.is_empty() {
        return Ok(());
    }
    log(&format!("autostart: setup={sig} bars={}", bars.len()));
    // Ensure capture is open (uses profile/top-level restore token).
    open_capture_with(state, token)?;
    for b in &bars {
        let _ = start_bar_inner(
            state,
            &b.bar_path,
            b.stream_index,
            cfg.fps,
            cfg.smooth,
            b.reverse,
            Layout {
                left: cfg.left,
                top: cfg.top,
                right: cfg.right,
                bottom: cfg.bottom,
            },
            Some(cfg.brightness),
            Some(&cfg.filter),
            cfg.filter_strength,
            cfg.max_frames as usize,
        );
    }
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            list_devices,
            open_capture,
            capture_info,
            close_capture,
            start_bar,
            stop_bar,
            stop_all,
            status,
            set_brightness,
            set_all_color,
            resume_sync,
            cinema_start,
            cinema_stop,
            get_config,
            save_config,
            autostart_enabled,
            set_autostart,
            autostart_run
        ])
        .setup(|app| {
            // Seed the device-identity cache from the config so the UI keeps
            // stable keys even before/without a successful identify.
            {
                let st = app.state::<AppState>();
                let cfg = read_config();
                let mut cache = st.ident_cache.lock().unwrap();
                for (id, ident) in cfg.bar_uuids {
                    cache.insert(id, ident);
                }
            }

            // Auto-start sync once at launch (done in Rust, not via the UI).
            if read_config().autostart_sync {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(2000));
                    let state = handle.state::<AppState>();
                    match autostart_run_inner(&state) {
                        Ok(()) => log("bloqsync: autostart ok"),
                        Err(e) => log(&format!("bloqsync: autostart failed: {e}")),
                    }
                    let _ = handle.emit("autostart", ());
                });
            }

            // Hotplug: watch for bar add/remove.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let ids = |v: &[bloqsync::device::DeviceInfo]| {
                    v.iter().map(|i| i.id.clone()).collect::<Vec<_>>()
                };
                let mut last = ids(&enumerate());
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    let now = ids(&enumerate());
                    if now != last {
                        last = now.clone();
                        let _ = handle.emit("devices-changed", &now);
                    }
                }
            });

            // Watchdog: keep the capture in sync with the current monitor setup
            // and the configured bars running (profile switching + reconnect).
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let cfg = read_config();
                if !cfg.autostart_sync {
                    continue;
                }
                let state = handle.state::<AppState>();
                if *state.sync_paused.lock().unwrap() {
                    continue;
                }
                let sig = monitors::signature();
                if sig.is_empty() {
                    continue;
                }
                // Does the open capture still match the current setup?
                let stale = {
                    let cap_open = state.capture.lock().unwrap().is_some();
                    let cap_sig = state.capture_signature.lock().unwrap().clone();
                    cap_open && cap_sig.as_deref() != Some(sig.as_str())
                };
                if stale {
                    log(&format!("setup changed -> {sig}: restarting capture"));
                    stop_all_inner(&state);
                    *state.capture.lock().unwrap() = None;
                    *state.capture_signature.lock().unwrap() = None;
                }
                // Only act on setups we have a profile for (no dialog surprises).
                let prof = match cfg.active_profile(&sig).cloned() {
                    Some(p) => p,
                    None => continue,
                };
                if state.capture.lock().unwrap().is_none() {
                    if let Err(e) = open_capture_with(&state, prof.restore_token.clone()) {
                        log(&format!("bloqsync: watchdog capture: {e}"));
                        continue;
                    }
                }
                for b in &prof.bars {
                    let need = {
                        let bars = state.bars.lock().unwrap();
                        match bars.get(&b.bar_path) {
                            Some(rt) => !rt.sync.is_running(),
                            None => true,
                        }
                    };
                    if need {
                        match start_bar_inner(
                            &state,
                            &b.bar_path,
                            b.stream_index,
                            cfg.fps,
                            cfg.smooth,
                            b.reverse,
                            Layout {
                                left: cfg.left,
                                top: cfg.top,
                                right: cfg.right,
                                bottom: cfg.bottom,
                            },
                            Some(cfg.brightness),
                            Some(&cfg.filter),
                            cfg.filter_strength,
                            cfg.max_frames as usize,
                        ) {
                            Ok(()) => log(&format!("bloqsync: watchdog started {}", b.bar_path)),
                            Err(e) => log(&format!("bloqsync: watchdog {}: {e}", b.bar_path)),
                        }
                    }
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running bloqsync");
}

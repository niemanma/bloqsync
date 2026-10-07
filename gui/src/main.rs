#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bloqsync::capture::{Capture, StreamInfo};
use bloqsync::device::{enumerate, find_by_identity, Device};
use bloqsync::engine::{spawn, SyncConfig, SyncHandle};
use bloqsync::filters::FilterKind;
use bloqsync::sampling::Layout;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};

struct BarRuntime {
    device: Arc<Device>,
    sync: SyncHandle,
}

#[derive(Default)]
struct AppState {
    capture: Mutex<Option<Arc<Capture>>>,
    bars: Mutex<HashMap<String, BarRuntime>>,
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
fn list_devices() -> Vec<DevDto> {
    enumerate()
        .into_iter()
        .filter_map(|info| {
            Device::open(&info).ok().map(|d| DevDto {
                path: d.info.path.clone(),
                id: d.info.id.clone(),
                leds: d.led_count,
                firmware: d.firmware.clone(),
                uuid: d.uuid.clone(),
            })
        })
        .collect()
}

/// Open the ScreenCast session (multiple monitors). Uses the stored restore
/// token if present, so after the first selection no dialog is shown.
#[tauri::command]
fn open_capture(state: State<AppState>) -> Result<Vec<StreamInfo>, String> {
    open_capture_inner(&state)
}

fn open_capture_inner(state: &AppState) -> Result<Vec<StreamInfo>, String> {
    let mut cap = state.capture.lock().unwrap();
    if cap.is_none() {
        let cfg = read_config();
        let c = bloqsync::capture::start(true, cfg.restore_token.clone())
            .map_err(|e| e.to_string())?;
        if let Some(token) = &c.restore_token {
            let mut cfg = read_config();
            cfg.restore_token = Some(token.clone());
            write_config(&cfg);
        }
        *cap = Some(Arc::new(c));
    }
    Ok(cap.as_ref().unwrap().streams.clone())
}

#[tauri::command]
fn close_capture(state: State<AppState>) -> Result<(), String> {
    stop_all_inner(&state);
    *state.capture.lock().unwrap() = None;
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

#[tauri::command]
fn status(state: State<AppState>) -> Vec<BarStatus> {
    state
        .bars
        .lock()
        .unwrap()
        .iter()
        .map(|(path, b)| BarStatus {
            bar: path.clone(),
            running: b.sync.is_running(),
            sent: b.sync.sent.load(std::sync::atomic::Ordering::Relaxed),
        })
        .collect()
}

#[tauri::command]
fn set_brightness(state: State<AppState>, value: u8) -> Result<(), String> {
    for b in state.bars.lock().unwrap().values() {
        let _ = b.device.set_brightness(value);
    }
    Ok(())
}

#[tauri::command]
fn set_color(state: State<AppState>, bar_path: String, r: u8, g: u8, b: u8) -> Result<(), String> {
    if let Some(rt) = state.bars.lock().unwrap().get(&bar_path) {
        rt.device.set_persistent_color([r, g, b]).map_err(|e| e.to_string())?;
    } else if let Some(info) = find_by_identity(&bar_path) {
        if let Ok(d) = Device::open(&info) {
            d.set_persistent_color([r, g, b]).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ── Config ──────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
struct BarConfig {
    bar_path: String,
    stream_index: usize,
    reverse: bool,
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
        }
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
    // Preserve the restore token (managed by Rust, not the UI).
    if cfg.restore_token.is_none() {
        cfg.restore_token = read_config().restore_token;
    }
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
    if cfg.bars.is_empty() {
        return Ok(());
    }
    // Ensure capture is open (uses restore token).
    let _ = open_capture_inner(&state)?;
    for b in &cfg.bars {
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
            close_capture,
            start_bar,
            stop_bar,
            stop_all,
            status,
            set_brightness,
            set_color,
            get_config,
            save_config,
            autostart_enabled,
            set_autostart,
            autostart_run
        ])
        .setup(|app| {
            // Auto-start sync once at launch (done in Rust, not via the UI).
            if read_config().autostart_sync {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(2000));
                    let state = handle.state::<AppState>();
                    match autostart_run_inner(&state) {
                        Ok(()) => eprintln!("bloqsync: autostart ok"),
                        Err(e) => eprintln!("bloqsync: autostart failed: {e}"),
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

            // Watchdog: keep configured bars running (reconnect after replug).
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let cfg = read_config();
                if !cfg.autostart_sync {
                    continue;
                }
                let state = handle.state::<AppState>();
                if state.capture.lock().unwrap().is_none() {
                    if let Err(e) = open_capture_inner(&state) {
                        eprintln!("bloqsync: watchdog capture: {e}");
                        continue;
                    }
                }
                for b in &cfg.bars {
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
                            Ok(()) => eprintln!("bloqsync: watchdog started {}", b.bar_path),
                            Err(e) => eprintln!("bloqsync: watchdog {}: {e}", b.bar_path),
                        }
                    }
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running bloqsync");
}

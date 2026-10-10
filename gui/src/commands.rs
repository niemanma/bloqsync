//! Tauri commands exposed to the UI. Thin adapters over [`crate::runtime`].

use crate::animations;
use crate::bar_models::{self, BarModel};
use crate::config::{read_config, update_config, Config, Ident, Preset};
use crate::logging::log;
use crate::monitors;
use crate::runtime::{self, BarSettings, parse_filter};
use crate::state::{AppState, IdentifyRuntime};
use bloqsync::anim::Animation;
use bloqsync::capture::StreamInfo;
use bloqsync::device::{enumerate, Device};
use bloqsync::sampling::Layout;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{Manager, State};

#[derive(serde::Serialize)]
pub(crate) struct DevDto {
    path: String,
    id: String,
    leds: usize,
    firmware: String,
    uuid: String,
}

#[tauri::command]
pub(crate) fn list_devices(state: State<AppState>) -> Vec<DevDto> {
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
    runtime::persist_identities(&state);
    list
}

/// Open the ScreenCast session (multiple monitors). Uses the stored restore
/// token if present, so after the first selection no dialog is shown.
#[tauri::command]
pub(crate) fn open_capture(state: State<AppState>) -> Result<Vec<StreamInfo>, String> {
    runtime::open_capture_inner(&state)
}

/// Current capture streams without opening anything (UI polling).
#[tauri::command]
pub(crate) fn capture_info(state: State<AppState>) -> Vec<StreamInfo> {
    state
        .capture
        .lock()
        .unwrap()
        .as_ref()
        .map(|c| c.streams.clone())
        .unwrap_or_default()
}

#[tauri::command]
pub(crate) fn close_capture(state: State<AppState>) -> Result<(), String> {
    runtime::stop_all_inner(&state);
    *state.capture.lock().unwrap() = None;
    *state.capture_signature.lock().unwrap() = None;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub(crate) fn start_bar(
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
    model_id: Option<String>,
) -> Result<(), String> {
    runtime::stop_identify_inner(&state);
    runtime::cinema_stop_inner(&state);
    runtime::animation_stop_inner(&state);
    *state.sync_paused.lock().unwrap() = false;
    let settings = BarSettings {
        fps,
        smooth,
        layout: Layout { left, top, right, bottom },
        brightness,
        filter: parse_filter(filter.as_deref()),
        filter_strength: filter_strength.unwrap_or(8),
        max_frames: max_frames.unwrap_or(0),
    };
    runtime::start_bar_inner(&state, &bar_path, stream_index, reverse, model_id, &settings)
}

#[tauri::command]
pub(crate) fn stop_bar(state: State<AppState>, bar_path: String) -> Result<(), String> {
    if let Some(b) = state.bars.lock().unwrap().remove(&bar_path) {
        b.sync.stop();
        let _ = b.device.set_persistent_color([255, 200, 100]);
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn stop_all(state: State<AppState>) -> Result<(), String> {
    runtime::stop_all_inner(&state);
    Ok(())
}

#[derive(serde::Serialize)]
pub(crate) struct BarStatus {
    bar: String,
    running: bool,
    sent: u64,
}

#[derive(serde::Serialize)]
pub(crate) struct StatusDto {
    bars: Vec<BarStatus>,
    paused: bool,
    cinema: bool,
    animation: bool,
}

#[tauri::command]
pub(crate) fn status(state: State<AppState>) -> StatusDto {
    let paused = *state.sync_paused.lock().unwrap();
    let cinema = state.cinema.lock().unwrap().is_some();
    let animation = state.animation.lock().unwrap().is_some();
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
    StatusDto { bars, paused, cinema, animation }
}

#[tauri::command]
pub(crate) fn set_brightness(state: State<AppState>, value: u8) -> Result<(), String> {
    for b in state.bars.lock().unwrap().values() {
        let _ = b.device.set_brightness(value);
    }
    Ok(())
}

/// Show a static colour on all bars and pause the sync so it is not
/// overwritten. Restart with [`resume_sync`].
#[tauri::command]
pub(crate) fn set_all_color(state: State<AppState>, r: u8, g: u8, b: u8) -> Result<(), String> {
    runtime::stop_identify_inner(&state);
    runtime::cinema_stop_inner(&state);
    runtime::animation_stop_inner(&state);
    *state.sync_paused.lock().unwrap() = true;
    runtime::stop_all_inner(&state);
    for info in enumerate() {
        if let Ok(d) = Device::open(&info) {
            let _ = d.set_persistent_color([r, g, b]);
        }
    }
    Ok(())
}

/// Resume screen sync after a static colour was shown.
#[tauri::command]
pub(crate) fn resume_sync(state: State<AppState>) -> Result<(), String> {
    runtime::stop_identify_inner(&state);
    runtime::cinema_stop_inner(&state);
    runtime::animation_stop_inner(&state);
    *state.sync_paused.lock().unwrap() = false;
    runtime::autostart_run_inner(&state)
}

#[tauri::command]
pub(crate) fn cinema_start(
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
    runtime::stop_identify_inner(&state);
    runtime::cinema_start_inner(
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

#[tauri::command]
pub(crate) fn cinema_stop(state: State<AppState>) -> Result<(), String> {
    runtime::cinema_stop_inner(&state);
    Ok(())
}

/// Light ONLY the given bar (blinking) and turn the others off, then restore.
#[tauri::command]
pub(crate) fn identify_bar(
    app: tauri::AppHandle,
    state: State<AppState>,
    bar_path: String,
) -> Result<(), String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    if let Some(prev) = state.identify.lock().unwrap().take() {
        prev.running.store(false, Ordering::Relaxed);
        let _ = prev.thread.join();
    }
    let was_running = !state.bars.lock().unwrap().is_empty();
    runtime::animation_stop_inner(&state);
    *state.sync_paused.lock().unwrap() = true;
    runtime::stop_all_inner(&state);

    let mut devices: Vec<(String, String, Arc<Device>)> = Vec::new();
    for info in enumerate() {
        if let Ok(d) = Device::open(&info) {
            devices.push((d.info.id.clone(), d.uuid.clone(), Arc::new(d)));
        }
    }
    if devices.is_empty() {
        return Err("keine Leiste gefunden".into());
    }
    log(&format!("identify: {bar_path}"));
    let running = Arc::new(AtomicBool::new(true));
    let running_t = running.clone();
    let handle = app.clone();
    let thread = std::thread::spawn(move || {
        let start = Instant::now();
        let mut on = true;
        while running_t.load(Ordering::Relaxed) && start.elapsed() < Duration::from_secs(8) {
            for (id, uuid, d) in &devices {
                let is_target = *id == bar_path || *uuid == bar_path;
                let c = if is_target && on { [255, 255, 255] } else { [0, 0, 0] };
                let _ = d.set_persistent_color(c);
            }
            on = !on;
            std::thread::sleep(Duration::from_millis(450));
        }
        for (_, _, d) in &devices {
            let _ = d.set_persistent_color([0, 0, 0]);
        }
        let st = handle.state::<AppState>();
        // Only restore the previous sync when the blink finished on its own;
        // an explicit stop (starting another mode) must not fight the caller.
        if was_running && running_t.load(Ordering::Relaxed) {
            *st.sync_paused.lock().unwrap() = false;
            let _ = runtime::autostart_run_inner(&st);
        }
    });
    *state.identify.lock().unwrap() = Some(IdentifyRuntime { running, thread });
    Ok(())
}

#[tauri::command]
pub(crate) fn get_config() -> Config {
    read_config()
}

#[tauri::command]
pub(crate) fn save_config(cfg: Config) {
    // Atomic read-modify-write: parallel save_config/start_bar calls must not
    // clobber each other's bar mapping (that made swapped bars revert).
    let sig = monitors::signature();
    update_config(|existing| existing.merge_from_ui(cfg, &sig));
}

#[tauri::command]
pub(crate) fn autostart_enabled() -> bool {
    runtime::autostart_enabled()
}

#[tauri::command]
pub(crate) fn set_autostart(enabled: bool) -> Result<(), String> {
    runtime::set_autostart(enabled)
}

#[tauri::command]
pub(crate) fn autostart_run(state: State<AppState>) -> Result<(), String> {
    runtime::autostart_run_inner(&state)
}

#[tauri::command]
pub(crate) fn list_presets() -> Vec<Preset> {
    read_config().presets
}

#[tauri::command]
pub(crate) fn save_preset(preset: Preset) -> Result<(), String> {
    update_config(|cfg| cfg.upsert_preset(preset));
    Ok(())
}

#[tauri::command]
pub(crate) fn delete_preset(name: String) -> Result<(), String> {
    update_config(|cfg| cfg.remove_preset(&name));
    Ok(())
}

#[tauri::command]
pub(crate) fn rename_preset(old: String, new: String) -> Result<bool, String> {
    Ok(update_config(|cfg| cfg.rename_preset(&old, &new)))
}

#[tauri::command]
pub(crate) fn list_bar_models() -> Vec<BarModel> {
    let mut models = bar_models::builtin();
    models.extend(read_config().bar_models);
    models
}

#[tauri::command]
pub(crate) fn save_bar_model(model: BarModel) -> Result<(), String> {
    update_config(|cfg| cfg.upsert_bar_model(model));
    Ok(())
}

#[tauri::command]
pub(crate) fn delete_bar_model(id: String) -> Result<(), String> {
    update_config(|cfg| cfg.remove_bar_model(&id));
    Ok(())
}

/// Built-in animations plus the user's own ones (user ids override built-ins).
#[tauri::command]
pub(crate) fn list_animations() -> Vec<Animation> {
    let mut anims = animations::builtin();
    for user in read_config().animations {
        if !user.valid() {
            continue;
        }
        match anims.iter_mut().find(|a| a.id == user.id) {
            Some(existing) => *existing = user,
            None => anims.push(user),
        }
    }
    anims
}

#[tauri::command]
pub(crate) fn save_animation(animation: Animation) -> Result<(), String> {
    update_config(|cfg| cfg.upsert_animation(animation));
    Ok(())
}

#[tauri::command]
pub(crate) fn delete_animation(id: String) -> Result<(), String> {
    update_config(|cfg| cfg.remove_animation(&id));
    Ok(())
}

#[tauri::command]
pub(crate) fn animation_start(
    state: State<AppState>,
    animation: Animation,
    fps: Option<u32>,
    brightness: Option<u8>,
) -> Result<(), String> {
    runtime::stop_identify_inner(&state);
    runtime::animation_start_inner(&state, animation, fps.unwrap_or(30), brightness)
}

#[tauri::command]
pub(crate) fn animation_stop(state: State<AppState>) -> Result<(), String> {
    runtime::animation_stop_inner(&state);
    Ok(())
}
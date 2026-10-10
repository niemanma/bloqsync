// Central app state, config <-> form mapping and preset handling.

import { invoke } from "./api.js";
import { $, hexToRgb } from "./util.js";

export const state = {
  devices: [],
  streams: [],
  config: null,
  presets: [],
  barModels: [],
  animations: [],
  animEditId: null,   // id of the animation currently loaded in the editor
  rowState: {},       // bar key -> { stream, reverse, modelId }
  activeMode: "screen",
  expert: false,
  activePreset: null,     // name of the loaded profile (or null)
  profileSnapshot: null,  // serialized settings/bars when the profile was loaded
};

const PROFILE_KEY = "bloqsync.profile";

const round2 = (v) => Math.round(v * 100) / 100;

/// Normalise settings so float noise (f32 round-trips) never looks "dirty".
function normalizedSettings(s) {
  return {
    fps: s.fps, smooth: round2(s.smooth), brightness: s.brightness,
    left: s.left, top: s.top, right: s.right, bottom: s.bottom,
    filter: s.filter, filter_strength: s.filter_strength, max_frames: s.max_frames,
    custom_color: s.custom_color, cinema_color: s.cinema_color,
    cinema_sensitivity: round2(s.cinema_sensitivity),
    cinema_brightness: round2(s.cinema_brightness),
    cinema_floor: round2(s.cinema_floor),
    cinema_smooth: round2(s.cinema_smooth),
    cinema_contrast: round2(s.cinema_contrast),
    cinema_pulse: round2(s.cinema_pulse),
  };
}

function snapshot(settings, bars) {
  const sorted = (bars || []).slice().sort((a, b) => a.bar_path.localeCompare(b.bar_path));
  return JSON.stringify({ settings: normalizedSettings(settings), bars: sorted });
}

const DEFAULT_CONFIG = {
  bars: [],
  fps: 24,
  smooth: 0.22,
  left: 18,
  top: 18,
  right: 18,
  bottom: 0,
  brightness: 200,
};

export function cfg() {
  return state.config || DEFAULT_CONFIG;
}

const int = (id, fallback = 0) => {
  const el = $("#" + id);
  if (!el) return fallback;
  const n = parseInt(el.value, 10);
  return Number.isFinite(n) ? n : fallback;
};

/** Read every tunable setting from the form (the advanced controls always
 *  exist in the DOM, even while hidden). */
export function readSettings() {
  return {
    fps: int("fps", 24),
    smooth: int("smooth", 22) / 100,
    brightness: int("brightness", 200),
    left: int("z-left", 18),
    top: int("z-top", 18),
    right: int("z-right", 18),
    bottom: int("z-bottom", 0),
    filter: $("#filter")?.value || "none",
    filter_strength: int("filter-strength", 8),
    max_frames: int("max-frames", 0),
    custom_color: $("#custom-color")?.value || "#ff8800",
    cinema_color: $("#cin-color")?.value || "#5a2882",
    cinema_sensitivity: int("cin-sens", 100) / 100,
    cinema_brightness: int("cin-bri", 70) / 100,
    cinema_floor: int("cin-floor", 35) / 100,
    cinema_smooth: int("cin-smooth", 60) / 100,
    cinema_contrast: int("cin-contrast", 100) / 100,
    cinema_pulse: int("cin-pulse", 0) / 100,
  };
}

function put(id, value) {
  const el = $("#" + id);
  if (!el) return;
  el.value = value;
  el.dispatchEvent(new Event("input"));
}

/** Write settings back into the form (used on load and when applying a preset). */
export function setSettings(s) {
  put("fps", s.fps ?? 24);
  put("smooth", Math.round((s.smooth ?? 0.22) * 100));
  put("brightness", s.brightness ?? 200);
  put("z-left", s.left ?? 18);
  put("z-top", s.top ?? 18);
  put("z-right", s.right ?? 18);
  put("z-bottom", s.bottom ?? 0);
  const filter = $("#filter");
  if (filter) filter.value = s.filter || "none";
  put("filter-strength", s.filter_strength ?? 8);
  put("max-frames", s.max_frames ?? 0);
  const custom = $("#custom-color");
  if (custom) custom.value = s.custom_color || "#ff8800";
  const cinema = $("#cin-color");
  if (cinema) cinema.value = s.cinema_color || "#5a2882";
  put("cin-sens", Math.round((s.cinema_sensitivity ?? 1.0) * 100));
  put("cin-bri", Math.round((s.cinema_brightness ?? 0.7) * 100));
  put("cin-floor", Math.round((s.cinema_floor ?? 0.35) * 100));
  put("cin-smooth", Math.round((s.cinema_smooth ?? 0.6) * 100));
  put("cin-contrast", Math.round((s.cinema_contrast ?? 1.0) * 100));
  put("cin-pulse", Math.round((s.cinema_pulse ?? 0.0) * 100));
}

/** Arguments for the `start_bar` command. */
export function screenSettings() {
  const s = readSettings();
  return {
    fps: s.fps,
    smooth: s.smooth,
    brightness: s.brightness,
    left: s.left,
    top: s.top,
    right: s.right,
    bottom: s.bottom,
    filter: s.filter,
    filterStrength: s.filter_strength,
    maxFrames: s.max_frames,
  };
}

/** Arguments for the `cinema_start` command. */
export function cinemaSettings() {
  const s = readSettings();
  const [r, g, b] = hexToRgb(s.cinema_color);
  return {
    sensitivity: s.cinema_sensitivity,
    brightness: s.cinema_brightness,
    onset: s.cinema_pulse,
    floor: s.cinema_floor,
    smooth: s.cinema_smooth,
    contrast: s.cinema_contrast,
    r, g, b,
  };
}

/** Current bar → monitor assignment from the in-memory row state. */
export function currentBars() {
  return state.devices.map((d) => {
    const key = d.uuid || d.id;
    return {
      bar_path: key,
      stream_index: state.rowState[key]?.stream ?? 0,
      reverse: state.rowState[key]?.reverse ?? true,
      model_id: state.rowState[key]?.modelId ?? defaultModelId(),
    };
  });
}

// ── Bar models ──────────────────────────────────────────────────────

export function defaultModelId() {
  return state.barModels[0]?.id ?? null;
}

export function modelById(id) {
  return state.barModels.find((m) => m.id === id) || state.barModels[0] || null;
}

export async function loadBarModels() {
  state.barModels = await invoke("list_bar_models").catch(() => state.barModels);
  return state.barModels;
}

export async function saveBarModel(model) {
  await invoke("save_bar_model", { model });
  await loadBarModels();
}

export async function deleteBarModel(id) {
  await invoke("delete_bar_model", { id });
  await loadBarModels();
}

// ── Animations ──────────────────────────────────────────────────────

export async function loadAnimations() {
  state.animations = await invoke("list_animations").catch(() => state.animations);
  return state.animations;
}

export async function saveAnimation(animation) {
  await invoke("save_animation", { animation });
  await loadAnimations();
}

export async function deleteAnimation(id) {
  await invoke("delete_animation", { id });
  await loadAnimations();
}

export function animationById(id) {
  return state.animations.find((a) => a.id === id) || null;
}

export function currentConfig() {
  return {
    bars: currentBars(),
    ...readSettings(),
    autostart: $("#autostart")?.checked ?? false,
    autostart_sync: $("#autostart-sync")?.checked ?? false,
  };
}

export async function saveConfig() {
  try {
    await invoke("save_config", { cfg: currentConfig() });
  } catch (_) { /* best effort */ }
}

export async function loadConfig() {
  try {
    state.config = await invoke("get_config");
  } catch (_) {
    state.config = { ...DEFAULT_CONFIG };
  }
  setSettings(state.config);
  const autostart = $("#autostart");
  if (autostart) autostart.checked = !!state.config.autostart;
  const autostartSync = $("#autostart-sync");
  if (autostartSync) autostartSync.checked = !!state.config.autostart_sync;
  return state.config;
}

export async function loadDevices() {
  state.devices = await invoke("list_devices").catch(() => state.devices);
  return state.devices;
}

/** Reload the raw config (e.g. the user bar-model list) without touching the form. */
export async function reloadConfigRaw() {
  try {
    state.config = await invoke("get_config");
  } catch (_) { /* keep the current value */ }
  return state.config;
}

export async function loadPresets() {
  state.presets = await invoke("list_presets").catch(() => []);
  return state.presets;
}

export async function savePreset(name) {
  // A preset captures the settings *and* the bar → monitor assignment.
  await invoke("save_preset", { preset: { name, ...readSettings(), bars: currentBars() } });
  await loadPresets();
}

export async function deletePreset(name) {
  await invoke("delete_preset", { name });
  await loadPresets();
}

/** Apply a preset to the form and the in-memory bar assignment. */
export function applyPreset(preset) {
  setSettings(preset);
  if (Array.isArray(preset.bars)) {
    for (const b of preset.bars) {
      state.rowState[b.bar_path] = { stream: b.stream_index, reverse: !!b.reverse };
    }
  }
}

export function setMode(mode) {
  state.activeMode = mode;
  document.querySelectorAll("#mode-tabs .seg").forEach((btn) => {
    btn.classList.toggle("is-active", btn.dataset.mode === mode);
  });
  ["screen", "cinema", "color", "anim"].forEach((m) => {
    $("#panel-" + m)?.classList.toggle("hidden", m !== mode);
  });
  localStorage.setItem("bloqsync.mode", mode);
}

export function setExpert(on) {
  state.expert = on;
  document.body.classList.toggle("expert", on);
  const toggle = $("#expert-toggle");
  if (toggle) toggle.setAttribute("aria-pressed", String(on));
  localStorage.setItem("bloqsync.expert", on ? "1" : "0");
}

// ── Profile (preset) state ──────────────────────────────────────────

export function currentSnapshot() {
  return snapshot(readSettings(), currentBars());
}

/** True if the current form differs from the loaded profile. */
export function isProfileDirty() {
  return state.activePreset !== null
    && state.profileSnapshot !== null
    && currentSnapshot() !== state.profileSnapshot;
}

/** Mark `name` as the active profile and remember the current state. */
export function setActivePreset(name) {
  state.activePreset = name || null;
  state.profileSnapshot = currentSnapshot();
  if (state.activePreset) localStorage.setItem(PROFILE_KEY, state.activePreset);
  else localStorage.removeItem(PROFILE_KEY);
}

/** Restore the active profile name after a restart (without overwriting the
 *  current settings, so unsaved changes are reported as "custom"). */
export function restoreActivePreset() {
  const name = localStorage.getItem(PROFILE_KEY);
  const preset = name && state.presets.find((p) => p.name === name);
  if (!preset) {
    setActivePreset(null);
    return;
  }
  state.activePreset = name;
  state.profileSnapshot = snapshot(preset, preset.bars);
}

/** Write the current settings back into the active profile. */
export async function saveActiveProfile() {
  if (!state.activePreset) return false;
  await savePreset(state.activePreset);
  state.profileSnapshot = currentSnapshot();
  return true;
}

export async function createProfile(name) {
  await savePreset(name);
  setActivePreset(name);
}

export async function renameProfile(oldName, newName) {
  const ok = await invoke("rename_preset", { old: oldName, new: newName }).catch(() => false);
  if (ok && state.activePreset === oldName) {
    state.activePreset = newName;
    localStorage.setItem(PROFILE_KEY, newName);
  }
  await loadPresets();
  return ok;
}

export async function deleteProfile(name) {
  await deletePreset(name);
  if (state.activePreset === name) setActivePreset(null);
}

/** True if a profile with this name already exists. */
export function profileExists(name) {
  return state.presets.some((p) => p.name === name);
}
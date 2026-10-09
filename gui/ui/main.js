// Entry point: wire the panels, load state and keep the status fresh.

import { invoke, listen } from "./js/api.js";
import { $, $$, bindSlider } from "./js/util.js";
import { state, loadConfig, loadDevices, loadPresets, setExpert, setMode } from "./js/state.js";
import { initScreen, renderBars, renderCaptureState } from "./js/screen.js";
import { initCinema } from "./js/cinema.js";
import { initColor } from "./js/color.js";
import { initProfiles, renderPresets } from "./js/profiles.js";
import { initSettings, renderDevices } from "./js/settings.js";

function bindOutputs() {
  bindSlider("fps", "fps-out", (v) => v);
  bindSlider("smooth", "smooth-out", (v) => (v / 100).toFixed(2));
  bindSlider("brightness", "brightness-out", (v) => v);
  bindSlider("filter-strength", "filter-strength-out", (v) => v);
  bindSlider("cin-sens", "cin-sens-out", (v) => (v / 100).toFixed(2));
  bindSlider("cin-bri", "cin-bri-out", (v) => (v / 100).toFixed(2));
  bindSlider("cin-floor", "cin-floor-out", (v) => (v / 100).toFixed(2));
  bindSlider("cin-smooth", "cin-smooth-out", (v) => (v / 100).toFixed(2));
  bindSlider("cin-contrast", "cin-contrast-out", (v) => (v / 100).toFixed(2));
  bindSlider("cin-pulse", "cin-pulse-out", (v) => (v / 100).toFixed(2));
}

function renderStatus(st) {
  const running = st.bars.filter((bar) => bar.running).length;
  const dot = $("#status-dot");
  const text = $("#status-text");
  if (st.cinema) {
    dot.className = "dot on";
    text.textContent = "Kino-Modus";
  } else if (st.paused) {
    dot.className = "dot warn";
    text.textContent = "Statische Farbe";
  } else if (running) {
    dot.className = "dot on";
    text.textContent = `${running} Leiste(n) synchron`;
  } else {
    dot.className = "dot";
    text.textContent = "inaktiv";
  }
}

async function statusTick() {
  try {
    renderStatus(await invoke("status"));
    const captures = await invoke("capture_info").catch(() => null);
    if (captures && JSON.stringify(captures) !== JSON.stringify(state.streams)) {
      state.streams = captures;
      renderCaptureState();
    }
  } catch (_) { /* transient errors are fine */ }
}

async function boot() {
  bindOutputs();
  $$("#mode-tabs .seg").forEach((btn) =>
    btn.addEventListener("click", () => setMode(btn.dataset.mode)));
  setExpert(localStorage.getItem("bloqsync.expert") === "1");
  setMode(localStorage.getItem("bloqsync.mode") || "screen");

  initScreen();
  initCinema();
  initColor();
  initProfiles();
  initSettings();

  await loadConfig();
  await loadPresets();
  renderPresets();
  await loadDevices();
  renderDevices();
  renderCaptureState();
  renderBars();

  try {
    await listen("devices-changed", async () => {
      await loadDevices();
      renderBars();
      renderDevices();
    });
    await listen("autostart", async () => {
      state.streams = await invoke("open_capture").catch(() => state.streams);
      await loadDevices();
      renderCaptureState();
    });
  } catch (_) { /* event API unavailable */ }

  statusTick();
  setInterval(statusTick, 800);
  window.__bloqsyncReady = true;
}

boot();
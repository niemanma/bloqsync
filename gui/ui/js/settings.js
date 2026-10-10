// Automation toggles, expert mode, advanced controls, device list and legacy link.

import { invoke } from "./api.js";
import { $, escapeHtml } from "./util.js";
import { state, saveConfig, setExpert } from "./state.js";
import { t } from "./i18n.js";

// Controls whose `change` should persist the config.
const PERSISTED_CONTROLS = [
  "filter",
  "fps", "smooth", "brightness", "filter-strength", "max-frames",
  "cin-sens", "cin-bri", "cin-floor", "cin-smooth", "cin-contrast", "cin-pulse",
  "cin-color", "custom-color",
];

export function renderDevices() {
  const box = $("#device-list");
  if (!box) return;
  if (state.devices.length === 0) {
    box.innerHTML = `<p class="preset-empty">${t("devices.none")}</p>`;
    return;
  }
  box.innerHTML = state.devices.map((device) => `
    <div class="device-row">
      <span class="dev-id">${escapeHtml(device.uuid || device.id)}</span>
      <span class="dev-meta">${device.leds} LEDs · FW ${escapeHtml(device.firmware)}</span>
      <span class="dev-meta">${escapeHtml(device.path)}</span>
    </div>`).join("");
}

export function initSettings() {
  $("#expert-toggle")?.addEventListener("click", () => setExpert(!state.expert));

  $("#autostart")?.addEventListener("change", async (event) => {
    try {
      await invoke("set_autostart", { enabled: event.target.checked });
    } catch (_) { /* ignored */ }
    await saveConfig();
  });
  $("#autostart-sync")?.addEventListener("change", () => saveConfig());

  PERSISTED_CONTROLS.forEach((id) => {
    const el = $("#" + id);
    if (el) el.addEventListener("change", () => saveConfig());
  });

  // Brightness changes should be visible immediately, even mid-sync.
  $("#brightness")?.addEventListener("change", (event) => {
    invoke("set_brightness", { value: parseInt(event.target.value, 10) }).catch(() => {});
  });

  $("#open-legacy")?.addEventListener("click", () => {
    window.location.href = "legacy/index.html";
  });
}
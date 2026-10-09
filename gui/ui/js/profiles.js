// Named-profile management UI.

import { $, escapeHtml } from "./util.js";
import { state, applyPreset, saveConfig, savePreset, deletePreset } from "./state.js";

export function renderPresets() {
  const box = $("#preset-list");
  if (!box) return;
  if (state.presets.length === 0) {
    box.innerHTML = `<p class="preset-empty">Noch keine Profile gespeichert.</p>`;
    return;
  }
  box.innerHTML = "";
  state.presets.forEach((preset) => {
    const row = document.createElement("div");
    row.className = "preset-row";
    row.innerHTML = `
      <span class="preset-name">${escapeHtml(preset.name)}</span>
      <button class="btn primary preset-apply" type="button">Laden</button>
      <button class="btn ghost preset-del" type="button" data-tip="Profil löschen">✕</button>`;
    row.querySelector(".preset-apply").addEventListener("click", async () => {
      applyPreset(preset);
      await saveConfig();
    });
    row.querySelector(".preset-del").addEventListener("click", async () => {
      await deletePreset(preset.name);
      renderPresets();
    });
    box.appendChild(row);
  });
}

export function initProfiles() {
  $("#preset-save")?.addEventListener("click", async () => {
    const input = $("#preset-name");
    const name = input.value.trim();
    if (!name) {
      input.focus();
      return;
    }
    await savePreset(name);
    input.value = "";
    renderPresets();
  });
}
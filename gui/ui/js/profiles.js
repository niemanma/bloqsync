// Named-profile management: active profile, dirty state, create/rename/delete.

import { invoke } from "./api.js";
import { $, escapeHtml } from "./util.js";
import {
  state, applyPreset, saveConfig, saveActiveProfile, createProfile,
  renameProfile, deleteProfile, setActivePreset, isProfileDirty, profileExists,
} from "./state.js";
import { renderBars } from "./screen.js";

/** Update the top chip and the card header to show the active profile. */
export function renderProfileStatus() {
  const active = state.activePreset;
  const dirty = isProfileDirty();
  const label = dirty && active ? `custom (${active})` : (active || "custom");

  const chip = $("#profile-chip");
  if (chip) {
    chip.textContent = label;
    chip.classList.toggle("dirty", dirty);
  }
  const activeName = $("#profile-active");
  if (activeName) activeName.textContent = label;
  const dirtyTag = $("#profile-dirty");
  if (dirtyTag) dirtyTag.classList.toggle("hidden", !dirty);
}

/** Apply a profile (settings + bar→monitor assignment) so it takes effect. */
async function loadPreset(preset) {
  const status = await invoke("status").catch(() => null);
  const wasRunning = !!status && status.bars.some((bar) => bar.running);

  applyPreset(preset);
  setActivePreset(preset.name);
  renderBars();
  await saveConfig();

  if (wasRunning && state.streams.length > 0) {
    document.querySelectorAll(".bar").forEach((row) => row._start?.());
  }
  renderPresets();
  renderProfileStatus();
}

async function newFromInput() {
  const input = $("#preset-name");
  const name = input.value.trim();
  if (!name) {
    input.focus();
    return;
  }
  if (profileExists(name)) {
    alert(`Ein Profil "${name}" existiert bereits.`);
    return;
  }
  await createProfile(name);
  input.value = "";
  renderPresets();
  renderProfileStatus();
}

async function saveToActive() {
  if (!state.activePreset) {
    // Nothing loaded yet: behave like "create from the name field".
    await newFromInput();
    return;
  }
  await saveActiveProfile();
  renderPresets();
  renderProfileStatus();
}

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
    row.classList.toggle("is-active", preset.name === state.activePreset);
    row.innerHTML = `
      <span class="preset-name">${escapeHtml(preset.name)}</span>
      <button class="btn primary preset-apply" type="button">Laden</button>
      <button class="btn preset-rename" type="button">Umbenennen</button>
      <button class="btn ghost preset-del" type="button" data-tip="Profil löschen">✕</button>`;

    row.querySelector(".preset-apply").addEventListener("click", () => loadPreset(preset));
    row.querySelector(".preset-rename").addEventListener("click", async () => {
      const entered = prompt("Neuer Profilname:", preset.name);
      if (entered == null) return;
      const name = entered.trim();
      if (!name || name === preset.name) return;
      if (profileExists(name)) {
        alert(`Ein Profil "${name}" existiert bereits.`);
        return;
      }
      await renameProfile(preset.name, name);
      renderPresets();
      renderProfileStatus();
    });
    row.querySelector(".preset-del").addEventListener("click", async () => {
      if (!confirm(`Profil "${preset.name}" löschen?`)) return;
      await deleteProfile(preset.name);
      renderPresets();
      renderProfileStatus();
    });
    box.appendChild(row);
  });
}

export function initProfiles() {
  $("#preset-new")?.addEventListener("click", newFromInput);
  $("#preset-save")?.addEventListener("click", saveToActive);
  $("#preset-name")?.addEventListener("keydown", (e) => {
    if (e.key === "Enter") newFromInput();
  });
}
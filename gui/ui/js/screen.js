// Screen-sync panel: capture lifecycle, schematic map and bar assignment cards.

import { invoke } from "./api.js";
import { $, $$, escapeHtml } from "./util.js";
import { state, cfg, screenSettings, saveConfig } from "./state.js";
import { monitorOptionLabel, renderMonmap } from "./monitors.js";

function setRunning(row, running) {
  row.querySelector(".bar-start").disabled = running;
  row.querySelector(".bar-stop").disabled = !running;
  const status = row.querySelector(".bar-status");
  status.textContent = running ? "läuft" : "—";
  status.classList.toggle("on", running);
}

export function renderCaptureState() {
  const empty = $("#capture-empty");
  const live = $("#capture-live");
  if (!empty || !live) return;
  const open = state.streams.length > 0;
  empty.classList.toggle("hidden", open);
  live.classList.toggle("hidden", !open);
  if (open) {
    renderMonmap($("#monmap"), state.streams);
    renderBars();
  }
}

export function renderBars() {
  const box = $("#bars");
  if (!box) return;
  box.innerHTML = "";
  if (state.devices.length === 0) {
    box.innerHTML = `<p class="preset-empty">Keine Leiste gefunden – stecke deine SyncLight an.</p>`;
    return;
  }

  state.devices.forEach((device, i) => {
    const key = device.uuid || device.id;
    if (!state.rowState[key]) {
      const saved = cfg().bars?.find((b) => b.bar_path === key);
      state.rowState[key] = {
        stream: saved?.stream_index ?? i,
        reverse: saved?.reverse ?? true,
      };
    }
    const row = document.createElement("div");
    row.className = "bar";
    row._key = key;

    const sub = state.expert
      ? `<span class="bar-sub">${escapeHtml(key)} · ${device.leds} LEDs · FW ${escapeHtml(device.firmware)}</span>`
      : "";
    row.innerHTML = `
      <div>
        <div class="bar-title">Leiste ${i + 1}</div>
        ${sub}
        <label class="mini-check"><input type="checkbox" class="bar-rev" ${state.rowState[key].reverse ? "checked" : ""}/> gespiegelt</label>
      </div>
      <select class="bar-stream"></select>
      <span class="bar-status">—</span>
      <div class="bar-actions">
        <button class="icon-btn bar-identify" type="button" data-tip="Diese Leiste kurz blinken lassen">✨</button>
        <button class="btn primary bar-start" type="button">Start</button>
        <button class="btn bar-stop" type="button" disabled>Stop</button>
      </div>`;

    const select = row.querySelector(".bar-stream");
    if (state.streams.length === 0) {
      select.appendChild(new Option("Erst Monitore verbinden", ""));
      select.disabled = true;
      row.querySelector(".bar-start").disabled = true;
    } else {
      state.streams.forEach((stream) => {
        const option = new Option(monitorOptionLabel(stream, state.expert), String(stream.index));
        if (stream.index === state.rowState[key].stream) option.selected = true;
        select.appendChild(option);
      });
    }

    const reverse = row.querySelector(".bar-rev");
    const start = row.querySelector(".bar-start");
    const stop = row.querySelector(".bar-stop");
    const status = row.querySelector(".bar-status");

    select.addEventListener("change", () => {
      state.rowState[key].stream = parseInt(select.value, 10) || 0;
    });
    reverse.addEventListener("change", () => {
      state.rowState[key].reverse = reverse.checked;
    });
    start.addEventListener("click", async () => {
      start.disabled = true;
      status.textContent = "startet …";
      status.classList.remove("on");
      try {
        await invoke("start_bar", {
          barPath: key,
          streamIndex: state.rowState[key].stream,
          reverse: reverse.checked,
          ...screenSettings(),
        });
        setRunning(row, true);
        await saveConfig();
      } catch (error) {
        status.textContent = "Fehler";
        start.disabled = false;
      }
    });
    stop.addEventListener("click", async () => {
      await invoke("stop_bar", { barPath: key }).catch(() => {});
      setRunning(row, false);
    });
    row.querySelector(".bar-identify").addEventListener("click", () => {
      invoke("identify_bar", { barPath: key }).catch(() => {});
    });

    box.appendChild(row);
  });
}

async function openCapture() {
  const button = $("#connect-monitors");
  if (button) {
    button.disabled = true;
    button.textContent = "Bitte Monitore im Dialog wählen …";
  }
  try {
    state.streams = await invoke("open_capture");
  } catch (_) {
    state.streams = [];
  }
  if (button) {
    button.disabled = false;
    button.textContent = "Monitore verbinden";
  }
  renderCaptureState();
}

export function initScreen() {
  $("#connect-monitors")?.addEventListener("click", openCapture);
  $("#disconnect-monitors")?.addEventListener("click", async () => {
    await invoke("close_capture").catch(() => {});
    state.streams = [];
    renderCaptureState();
  });
  $("#start-all")?.addEventListener("click", () => {
    if (state.streams.length === 0) return;
    $$(".bar-start").forEach((button) => {
      if (!button.disabled) button.click();
    });
  });
  $("#stop-all")?.addEventListener("click", async () => {
    await invoke("stop_all").catch(() => {});
    $$(".bar").forEach((row) => setRunning(row, false));
  });
}
// Live LED mirror: polls the backend for the current colours of every bar and
// renders an animated strip so the user can watch the hardware in real time.

import { invoke } from "./api.js";
import { $ } from "./util.js";
import { state } from "./state.js";
import { t } from "./i18n.js";

const rows = new Map(); // key -> { el, cells, count }
let lastSig = null;

function ensureRows() {
  const box = $("#live-strips");
  if (!box) return;
  // Label and order rows by the bar's assigned monitor (not by USB order).
  const entries = state.devices
    .map((d, i) => {
      const key = d.uuid || d.id;
      return {
        key,
        stream: state.rowState[key]?.stream ?? i,
        reverse: state.rowState[key]?.reverse ?? true,
      };
    })
    .sort((a, b) => a.stream - b.stream);
  const sig = entries.map((e) => `${e.key}:${e.stream}:${e.reverse}`).join(",");
  if (sig === lastSig) return;
  lastSig = sig;

  box.innerHTML = "";
  rows.clear();
  if (entries.length === 0) {
    box.innerHTML = `<p class="preset-empty">${t("devices.none")}</p>`;
    return;
  }
  for (const e of entries) {
    const row = document.createElement("div");
    row.className = "live-row";
    row.innerHTML = `
      <div class="live-label">${t("bar.monitor", { n: e.stream + 1 })}</div>
      <div class="live-bar"></div>`;
    box.appendChild(row);
    rows.set(e.key, { el: row.querySelector(".live-bar"), cells: [], count: 0, reverse: e.reverse });
  }
}

function setCount(row, n) {
  if (row.count === n) return;
  row.count = n;
  row.el.innerHTML = "";
  row.cells = [];
  for (let i = 0; i < n; i++) {
    const c = document.createElement("span");
    c.className = "led";
    row.el.appendChild(c);
    row.cells.push(c);
  }
}

function paint(row, cols) {
  if (!Array.isArray(cols) || cols.length === 0) return;
  // The engine streams in LED order; a mirrored (reverse) bar is mounted the
  // other way round, so reverse the visual to match what the user sees.
  const led = row.reverse ? cols.slice().reverse() : cols;
  setCount(row, led.length);
  for (let i = 0; i < led.length && i < row.cells.length; i++) {
    const [r, g, b] = led[i];
    row.cells[i].style.backgroundColor = `rgb(${r},${g},${b})`;
    row.cells[i].style.boxShadow = `0 0 8px rgba(${r},${g},${b},.65)`;
  }
}

export function renderLiveMode(mode) {
  const el = $("#live-mode");
  if (el) el.textContent = mode;
}

export async function liveTick() {
  ensureRows();
  const live = await invoke("live_state").catch(() => null);
  if (!live) return;
  for (const [key, row] of rows) {
    if (live[key]) paint(row, live[key]);
  }
}

export function initLive() {
  ensureRows();
}

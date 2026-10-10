// Screen-sync panel: capture lifecycle, schematic map and bar assignment cards.

import { invoke } from "./api.js";
import { $, $$, escapeHtml } from "./util.js";
import { state, cfg, screenSettings, saveConfig, modelById, defaultModelId } from "./state.js";
import { monitorOptionLabel, renderMonmap } from "./monitors.js";
import { t } from "./i18n.js";

/** Human-readable model conflict (empty when everything lines up). */
function modelWarning(model, device) {
  if (!model) return "";
  const sum = model.left + model.top + model.bottom + model.right;
  const messages = [];
  if (model.leds !== device.leds) {
    messages.push(t("bar.warnModelLeds", { model: model.leds, bar: device.leds }));
  }
  if (sum !== model.leds) {
    messages.push(t("bar.warnDist", {
      l: model.left, t: model.top, b: model.bottom, r: model.right, sum, leds: model.leds,
    }));
  }
  return messages.join(" · ");
}

function setRunning(row, running) {
  row.querySelector(".bar-start").disabled = running;
  row.querySelector(".bar-stop").disabled = !running;
  const status = row.querySelector(".bar-status");
  status.textContent = running ? t("bar.running") : "—";
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
    box.innerHTML = `<p class="preset-empty">${t("devices.none")}</p>`;
    return;
  }

  state.devices.forEach((device, i) => {
    const key = device.uuid || device.id;
    if (!state.rowState[key]) {
      const saved = cfg().bars?.find((b) => b.bar_path === key);
      state.rowState[key] = {
        stream: saved?.stream_index ?? i,
        reverse: saved?.reverse ?? true,
        modelId: saved?.model_id ?? defaultModelId(),
      };
    }
    const st = state.rowState[key];
    // Keep the assignment valid when the set of monitors changed.
    if (state.streams.length && !state.streams.some((s) => s.index === st.stream)) {
      st.stream = state.streams[Math.min(i, state.streams.length - 1)].index;
    }
    // Name each bar after the monitor it is assigned to; before a capture is
    // open we can only fall back to a sequential label.
    const title = state.streams.length
      ? t("bar.monitor", { n: st.stream + 1 })
      : t("bar.fallbackName", { n: i + 1 });

    const row = document.createElement("div");
    row.className = "bar";
    row._key = key;

    const sub = state.expert
      ? `<span class="bar-sub">${escapeHtml(key)} · ${device.leds} LEDs · FW ${escapeHtml(device.firmware)}</span>`
      : "";
    row.innerHTML = `
      <div>
        <div class="bar-title">${title}</div>
        ${sub}
        <label class="mini-check"><input type="checkbox" class="bar-rev" ${st.reverse ? "checked" : ""}/> ${t("bar.mirror")}</label>
        <span class="bar-warn"></span>
      </div>
      <select class="bar-stream"></select>
      <select class="bar-model" data-tip="${t("bar.modelTip")}"></select>
      <span class="bar-status">—</span>
      <div class="bar-actions">
        <button class="icon-btn bar-identify" type="button" data-tip="${t("bar.identifyTip")}" aria-label="${t("bar.identifyTip")}">
          <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M9 18h6"/><path d="M10 22h4"/>
            <path d="M12 2a7 7 0 0 0-4 12.7c.6.5 1 1.2 1 2V17h6v-.3c0-.8.4-1.5 1-2A7 7 0 0 0 12 2z"/>
          </svg>
        </button>
        <button class="btn primary bar-start" type="button">Start</button>
        <button class="btn bar-stop" type="button" disabled>Stop</button>
      </div>`;

    const select = row.querySelector(".bar-stream");
    if (state.streams.length === 0) {
      select.appendChild(new Option(t("bar.connectFirst"), ""));
      select.disabled = true;
      row.querySelector(".bar-start").disabled = true;
    } else {
      state.streams.forEach((stream) => {
        const option = new Option(monitorOptionLabel(stream, state.expert), String(stream.index));
        if (stream.index === st.stream) option.selected = true;
        select.appendChild(option);
      });
    }

    // Bar model dropdown.
    const modelSelect = row.querySelector(".bar-model");
    state.barModels.forEach((model) => {
      const option = new Option(model.name, model.id);
      if (model.id === st.modelId) option.selected = true;
      modelSelect.appendChild(option);
    });

    const titleEl = row.querySelector(".bar-title");
    const warnEl = row.querySelector(".bar-warn");
    const reverse = row.querySelector(".bar-rev");
    const start = row.querySelector(".bar-start");
    const stop = row.querySelector(".bar-stop");
    const status = row.querySelector(".bar-status");
    const refreshWarning = () => {
      const warning = modelWarning(modelById(st.modelId), device);
      warnEl.textContent = warning ? "⚠ " + warning : "";
      warnEl.title = warning;
      warnEl.classList.toggle("hidden", !warning);
    };
    refreshWarning();

    select.addEventListener("change", () => {
      st.stream = parseInt(select.value, 10) || 0;
      titleEl.textContent = t("bar.monitor", { n: st.stream + 1 });
      saveConfig();
    });
    modelSelect.addEventListener("change", () => {
      st.modelId = modelSelect.value;
      refreshWarning();
      saveConfig();
    });
    reverse.addEventListener("change", () => {
      st.reverse = reverse.checked;
      saveConfig();
    });
    row._start = async () => {
      if (state.streams.length === 0) return;
      start.disabled = true;
      status.textContent = t("bar.starting");
      status.classList.remove("on");
      const model = modelById(st.modelId);
      try {
        await invoke("start_bar", {
          barPath: key,
          streamIndex: st.stream,
          reverse: reverse.checked,
          ...screenSettings(),
          left: model.left,
          top: model.top,
          right: model.right,
          bottom: model.bottom,
          modelId: st.modelId,
        });
        setRunning(row, true);
        await saveConfig();
      } catch (error) {
        status.textContent = t("bar.error");
        start.disabled = false;
      }
    };
    start.addEventListener("click", () => row._start());
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
    button.textContent = t("screen.connectBusy");
  }
  try {
    state.streams = await invoke("open_capture");
  } catch (_) {
    state.streams = [];
  }
  if (button) {
    button.disabled = false;
    button.textContent = t("screen.connect");
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
    // (Re)apply the current mapping to every bar, even if it is already
    // running, so a changed monitor assignment takes effect immediately.
    $$(".bar").forEach((row) => row._start?.());
  });
  $("#stop-all")?.addEventListener("click", async () => {
    await invoke("stop_all").catch(() => {});
    $$(".bar").forEach((row) => setRunning(row, false));
  });
}
const { invoke } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);

let devices = [];
let streams = [];
let savedConfig = { bars: [], fps: 45, smooth: 0.55, left: 18, top: 18, right: 18, bottom: 0, brightness: 200 };
const rowState = {}; // bar_path -> {stream, reverse}

function bindSlider(id, out, fmt) {
  const el = $(id);
  const o = $(out);
  const upd = () => (o.textContent = fmt(el.value));
  el.addEventListener("input", upd);
  upd();
  return el;
}
const fps = bindSlider("fps", "fps-out", (v) => v);
const smooth = bindSlider("smooth", "smooth-out", (v) => (v / 100).toFixed(2));
const brightness = bindSlider("brightness", "brightness-out", (v) => v);
const filterStrength = bindSlider("filter-strength", "filter-strength-out", (v) => v);
const cinSens = bindSlider("cin-sens", "cin-sens-out", (v) => (v / 100).toFixed(2));
const cinBri = bindSlider("cin-bri", "cin-bri-out", (v) => (v / 100).toFixed(2));
const cinPulse = bindSlider("cin-pulse", "cin-pulse-out", (v) => (v / 100).toFixed(2));
const cinFloor = bindSlider("cin-floor", "cin-floor-out", (v) => (v / 100).toFixed(2));
const cinSmooth = bindSlider("cin-smooth", "cin-smooth-out", (v) => (v / 100).toFixed(2));
const cinContrast = bindSlider("cin-contrast", "cin-contrast-out", (v) => (v / 100).toFixed(2));
const filterSel = $("filter");
const maxFrames = $("max-frames");
const autostart = $("autostart");
const autostartSync = $("autostart-sync");

function streamLabel(s) {
  const size = s.size ? `${s.size[0]}×${s.size[1]}` : "?";
  const xs = streams.map((x) => (x.position ? x.position[0] : 0));
  const ys = streams.map((x) => (x.position ? x.position[1] : 0));
  const minx = Math.min(...xs), maxx = Math.max(...xs);
  const miny = Math.min(...ys), maxy = Math.max(...ys);
  const px = s.position ? s.position[0] : 0;
  const py = s.position ? s.position[1] : 0;
  let where = [];
  if (minx !== maxx) { if (px === minx) where.push("links"); if (px === maxx) where.push("rechts"); }
  if (miny !== maxy) { if (py === miny) where.push("oben"); if (py === maxy) where.push("unten"); }
  const pos = s.position ? `(${px}, ${py})` : "";
  const desc = where.length ? " · " + where.join("/") : "";
  return `Monitor ${s.index + 1}: ${size} ${pos}${desc}`;
}

function renderStreams() {
  const box = $("streams");
  if (streams.length === 0) {
    box.innerHTML = '<p class="muted">Noch keine Aufnahme.</p>';
    return;
  }
  // Schematic layout map from the real monitor positions/sizes.
  const hasGeo = streams.every((s) => s.position && s.size);
  let html = "";
  if (hasGeo) {
    const xs = streams.map((s) => s.position[0]);
    const ys = streams.map((s) => s.position[1]);
    const minx = Math.min(...xs), miny = Math.min(...ys);
    const maxx = Math.max(...streams.map((s) => s.position[0] + s.size[0]));
    const maxy = Math.max(...streams.map((s) => s.position[1] + s.size[1]));
    const bw = maxx - minx || 1, bh = maxy - miny || 1;
    const scale = Math.min(560 / bw, 220 / bh);
    html += `<div class="monmap" style="width:${Math.round(bw * scale)}px;height:${Math.round(bh * scale)}px">`;
    streams.forEach((s, i) => {
      const x = (s.position[0] - minx) * scale;
      const y = (s.position[1] - miny) * scale;
      const w = s.size[0] * scale, h = s.size[1] * scale;
      html += `<div class="monbox" style="left:${Math.round(x)}px;top:${Math.round(y)}px;width:${Math.round(w)}px;height:${Math.round(h)}px"><span class="monnum">${i + 1}</span><span class="monres">${s.size[0]}×${s.size[1]}</span></div>`;
    });
    html += "</div>";
  }
  html += '<ul class="list">';
  for (const s of streams) html += `<li>${streamLabel(s)}</li>`;
  html += "</ul>";
  box.innerHTML = html;
  renderBars();
}

function renderBars() {
  const box = $("bars");
  box.innerHTML = "";
  if (devices.length === 0) {
    box.innerHTML = '<p class="muted">Keine Leiste gefunden.</p>';
    return;
  }
  for (const d of devices) {
    const st = rowState[(d.uuid || d.id)] || (rowState[(d.uuid || d.id)] = {
      stream: savedConfig.bars.find((b) => b.bar_path === (d.uuid || d.id))?.stream_index ?? 0,
      reverse: savedConfig.bars.find((b) => b.bar_path === (d.uuid || d.id))?.reverse ?? true,
    });

    const row = document.createElement("div");
    row.className = "bar-row";
    row.innerHTML = `
      <div class="bar-name">${(d.uuid || d.id)}<span class="muted"> · ${d.leds} LEDs · FW ${d.firmware}</span></div>
      <select class="bar-stream"></select>
      <label class="check"><input type="checkbox" class="bar-rev" ${st.reverse ? "checked" : ""}/> gespiegelt</label>
      <span class="bar-status muted">—</span>
      <button class="bar-identify">Ident.</button>
      <button class="bar-start primary">Start</button>
      <button class="bar-stop" disabled>Stop</button>
    `;
    const sel = row.querySelector(".bar-stream");
    if (streams.length === 0) {
      const o = document.createElement("option");
      o.textContent = "erst Aufnahme starten";
      sel.appendChild(o);
      sel.disabled = true;
      row.querySelector(".bar-start").disabled = true;
    } else {
      for (const s of streams) {
        const o = document.createElement("option");
        o.value = s.index;
        o.textContent = streamLabel(s);
        if (s.index === st.stream) o.selected = true;
        sel.appendChild(o);
      }
    }
    const rev = row.querySelector(".bar-rev");
    const startBtn = row.querySelector(".bar-start");
    const stopBtn = row.querySelector(".bar-stop");
    const statusEl = row.querySelector(".bar-status");
    row._setRunning = (running) => {
      startBtn.disabled = running || streams.length === 0;
      stopBtn.disabled = !running;
      statusEl.textContent = running ? "läuft" : "—";
      statusEl.style.color = running ? "#34c759" : "";
    };
    sel.addEventListener("change", () => (st.stream = parseInt(sel.value, 10)));
    rev.addEventListener("change", () => (st.reverse = rev.checked));
    startBtn.addEventListener("click", async () => {
      startBtn.disabled = true;
      statusEl.textContent = "warte auf Monitor-Dialog …";
      try {
        await invoke("start_bar", {
          barPath: (d.uuid || d.id),
          streamIndex: parseInt(sel.value, 10),
          fps: parseInt(fps.value, 10),
          smooth: parseFloat(smooth.value) / 100,
          reverse: rev.checked,
          left: +$("z-left").value,
          top: +$("z-top").value,
          right: +$("z-right").value,
          bottom: +$("z-bottom").value,
          brightness: parseInt(brightness.value, 10),
          filter: filterSel.value,
          filterStrength: parseInt(filterStrength.value, 10),
          maxFrames: parseInt(maxFrames.value, 10),
        });
        row._setRunning(true);
        saveConfig();
      } catch (e) {
        statusEl.textContent = "Fehler: " + e;
        startBtn.disabled = false;
      }
    });
    stopBtn.addEventListener("click", async () => {
      await invoke("stop_bar", { barPath: (d.uuid || d.id) });
      row._setRunning(false);
    });
    row.querySelector(".bar-identify").addEventListener("click", () => {
      invoke("identify_bar", { barPath: (d.uuid || d.id) }).catch(() => {});
    });
    box.appendChild(row);
  }
  // Also show configured bars that are currently not connected.
  const present = new Set(devices.map((d) => d.uuid || d.id));
  for (const b of savedConfig.bars || []) {
    if (present.has(b.bar_path)) continue;
    const row = document.createElement("div");
    row.className = "bar-row";
    row.innerHTML = `
      <div class="bar-name">${b.bar_path}<span class="muted"> · nicht verbunden</span></div>
      <span class="muted">—</span>
      <label class="check"><input type="checkbox" disabled ${b.reverse ? "checked" : ""}/> gespiegelt</label>
      <span class="bar-status muted">nicht verbunden</span>
      <button class="bar-start primary" disabled>Start</button>
      <button class="bar-stop" disabled>Stop</button>
    `;
    box.appendChild(row);
  }
}

function currentConfig() {
  return {
    bars: devices.map((d) => ({
      bar_path: (d.uuid || d.id),
      stream_index: rowState[(d.uuid || d.id)]?.stream ?? 0,
      reverse: rowState[(d.uuid || d.id)]?.reverse ?? true,
    })),
    fps: parseInt(fps.value, 10),
    smooth: parseFloat(smooth.value) / 100,
    left: +$("z-left").value,
    top: +$("z-top").value,
    right: +$("z-right").value,
    bottom: +$("z-bottom").value,
    brightness: parseInt(brightness.value, 10),
    filter: filterSel.value,
    filter_strength: parseInt(filterStrength.value, 10),
    max_frames: parseInt(maxFrames.value, 10),
    autostart: autostart.checked,
    autostart_sync: autostartSync.checked,
    custom_color: $("custom-color").value,
    cinema_color: $("cin-color").value,
    cinema_sensitivity: parseFloat(cinSens.value) / 100,
    cinema_brightness: parseFloat(cinBri.value) / 100,
    cinema_floor: parseFloat(cinFloor.value) / 100,
    cinema_smooth: parseFloat(cinSmooth.value) / 100,
    cinema_contrast: parseFloat(cinContrast.value) / 100,
    cinema_pulse: parseFloat(cinPulse.value) / 100,
  };
}
function saveConfig() {
  invoke("save_config", { cfg: currentConfig() }).catch(() => {});
}

$("open-capture").addEventListener("click", async () => {
  const btn = $("open-capture");
  btn.disabled = true;
  btn.textContent = "bitte Monitore im Dialog wählen …";
  try {
    streams = await invoke("open_capture");
  } catch (e) {
    $("streams").innerHTML = `<li class="muted">Fehler: ${e}</li>`;
  }
  btn.disabled = false;
  btn.textContent = "Monitore erfassen";
  renderStreams();
});

$("close-capture").addEventListener("click", async () => {
  await invoke("close_capture");
  streams = [];
  renderStreams();
});

$("start-all").addEventListener("click", async () => {
  if (streams.length === 0) {
    alert("Zuerst 'Monitore erfassen' klicken.");
    return;
  }
  for (const row of document.querySelectorAll(".bar-row")) {
    const b = row.querySelector(".bar-start");
    if (!b.disabled) b.click();
  }
});
$("stop-all").addEventListener("click", async () => {
  await invoke("stop_all");
  for (const row of document.querySelectorAll(".bar-row")) row._setRunning(false);
});

brightness.addEventListener("change", () => invoke("set_brightness", { value: parseInt(brightness.value, 10) }).catch(() => {}));
document.querySelectorAll(".sw").forEach((b) => {
  b.addEventListener("click", () => {
    const [r, g, bl] = b.dataset.rgb.split(",").map(Number);
    invoke("set_all_color", { r, g, b: bl }).catch(() => {});
  });
});
const customColor = $("custom-color");
if (customColor) {
  let t = null;
  customColor.addEventListener("input", () => {
    if (t) clearTimeout(t);
    t = setTimeout(() => {
      const v = customColor.value;
      const r = parseInt(v.slice(1, 3), 16);
      const g = parseInt(v.slice(3, 5), 16);
      const b = parseInt(v.slice(5, 7), 16);
      invoke("set_all_color", { r, g, b }).catch(() => {});
    }, 150);
  });
}
const resumeBtn = $("resume-sync");
if (resumeBtn) {
  resumeBtn.addEventListener("click", () => {
    invoke("resume_sync").catch(() => {});
  });
}
const cinemaStart = $("cinema-start");
if (cinemaStart) {
  cinemaStart.addEventListener("click", () => {
    const v = $("cin-color").value;
    invoke("cinema_start", {
      sensitivity: parseFloat(cinSens.value) / 100,
      brightness: parseFloat(cinBri.value) / 100,
      onset: parseFloat(cinPulse.value) / 100,
      floor: parseFloat(cinFloor.value) / 100,
      smooth: parseFloat(cinSmooth.value) / 100,
      contrast: parseFloat(cinContrast.value) / 100,
      r: parseInt(v.slice(1, 3), 16),
      g: parseInt(v.slice(3, 5), 16),
      b: parseInt(v.slice(5, 7), 16),
    }).catch(() => {});
  });
}
const cinemaStop = $("cinema-stop");
if (cinemaStop) {
  cinemaStop.addEventListener("click", () => invoke("cinema_stop").catch(() => {}));
}

setInterval(async () => {
  try {
    const st = await invoke("status");
    const running = st.bars.filter((s) => s.running).length;
    if (st.cinema) {
      $("status-dot").className = "dot on";
      $("status-text").textContent = "Kino-Modus (Audio)";
    } else if (st.paused) {
      $("status-dot").className = "dot off";
      $("status-text").textContent = "pausiert (statische Farbe)";
    } else {
      $("status-dot").className = "dot " + (running ? "on" : "off");
      $("status-text").textContent = running ? `${running} Leiste(n) synchron` : "inaktiv";
    }
    // Keep the capture list in sync with the backend (autostart opens it in Rust).
    const caps = await invoke("capture_info").catch(() => null);
    if (caps && JSON.stringify(caps) !== JSON.stringify(streams)) {
      streams = caps;
      renderStreams();
    }
  } catch (e) {}
}, 700);

autostart.addEventListener("change", async () => {
  try { await invoke("set_autostart", { enabled: autostart.checked }); } catch (e) {}
  saveConfig();
});
autostartSync.addEventListener("change", saveConfig);
[cinSens, cinBri, cinFloor, cinSmooth, cinContrast, cinPulse].forEach((el) =>
  el && el.addEventListener("change", saveConfig)
);
$("cin-color").addEventListener("change", saveConfig);
$("custom-color").addEventListener("change", saveConfig);

try {
  const { listen } = window.__TAURI__.event;
  listen("autostart", async () => {
    devices = await invoke("list_devices").catch(() => devices);
    streams = await invoke("open_capture").catch(() => []);
    renderStreams();
  });
  listen("devices-changed", async () => {
    devices = await invoke("list_devices").catch(() => devices);
    renderBars();
  });
} catch (e) {}

(async () => {
  try { savedConfig = await invoke("get_config"); } catch (e) {}
  fps.value = savedConfig.fps; fps.dispatchEvent(new Event("input"));
  smooth.value = Math.round(savedConfig.smooth * 100); smooth.dispatchEvent(new Event("input"));
  brightness.value = savedConfig.brightness; brightness.dispatchEvent(new Event("input"));
  $("z-left").value = savedConfig.left;
  $("z-top").value = savedConfig.top;
  $("z-right").value = savedConfig.right;
  $("z-bottom").value = savedConfig.bottom;
  filterSel.value = savedConfig.filter || "none";
  filterStrength.value = savedConfig.filter_strength || 8;
  filterStrength.dispatchEvent(new Event("input"));
  maxFrames.value = savedConfig.max_frames ?? 0;
  autostart.checked = !!savedConfig.autostart;
  autostartSync.checked = !!savedConfig.autostart_sync;
  if (savedConfig.custom_color) $("custom-color").value = savedConfig.custom_color;
  if (savedConfig.cinema_color) $("cin-color").value = savedConfig.cinema_color;
  cinSens.value = Math.round((savedConfig.cinema_sensitivity ?? 1.0) * 100); cinSens.dispatchEvent(new Event("input"));
  cinBri.value = Math.round((savedConfig.cinema_brightness ?? 0.7) * 100); cinBri.dispatchEvent(new Event("input"));
  cinFloor.value = Math.round((savedConfig.cinema_floor ?? 0.35) * 100); cinFloor.dispatchEvent(new Event("input"));
  cinSmooth.value = Math.round((savedConfig.cinema_smooth ?? 0.6) * 100); cinSmooth.dispatchEvent(new Event("input"));
  cinContrast.value = Math.round((savedConfig.cinema_contrast ?? 1.0) * 100); cinContrast.dispatchEvent(new Event("input"));
  cinPulse.value = Math.round((savedConfig.cinema_pulse ?? 0.0) * 100); cinPulse.dispatchEvent(new Event("input"));

  devices = await invoke("list_devices");
  renderBars();
})();

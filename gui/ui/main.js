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
const filterSel = $("filter");
const maxFrames = $("max-frames");
const autostart = $("autostart");
const autostartSync = $("autostart-sync");

function streamLabel(s) {
  const pos = s.position ? `@ ${s.position[0]},${s.position[1]}` : "";
  const size = s.size ? `${s.size[0]}×${s.size[1]}` : "?";
  return `Monitor ${s.index}: ${size} ${pos}`;
}

function renderStreams() {
  const ul = $("streams");
  if (streams.length === 0) {
    ul.innerHTML = '<li class="muted">Noch keine Aufnahme.</li>';
    return;
  }
  ul.innerHTML = "";
  for (const s of streams) {
    const li = document.createElement("li");
    li.textContent = streamLabel(s);
    ul.appendChild(li);
  }
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
    for (const d of devices) invoke("set_color", { barPath: (d.uuid || d.id), r, g, b: bl }).catch(() => {});
  });
});

setInterval(async () => {
  try {
    const st = await invoke("status");
    const running = st.filter((s) => s.running).length;
    $("status-dot").className = "dot " + (running ? "on" : "off");
    $("status-text").textContent = running ? `${running} Leiste(n) synchron` : "inaktiv";
  } catch (e) {}
}, 700);

autostart.addEventListener("change", async () => {
  try { await invoke("set_autostart", { enabled: autostart.checked }); } catch (e) {}
  saveConfig();
});
autostartSync.addEventListener("change", saveConfig);

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

  devices = await invoke("list_devices");
  renderBars();
})();

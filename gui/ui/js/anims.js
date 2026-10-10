// Animation panel: gallery, 2D colour board editor and live preview.
//
// A pattern is a chain of points on a Hue × Saturation board; each point carries
// a brightness and the vector (duration % + swift) to the next point. The
// durations are percentages that must add up to 100 — they are only checked when
// the user asks (Check durations / Save / Play), never silently rewound.
//
// The preview is a JavaScript mirror of the Rust renderer while the bars use the
// real one.

import { invoke } from "./api.js";
import { $, $$, escapeHtml, bindSlider, hexToRgb, rgbToHs } from "./util.js";
import {
  state, saveAnimation, deleteAnimation, animationById, reloadConfigRaw,
} from "./state.js";
import { t } from "./i18n.js";
import {
  Preview, MOVEMENT_IDS, normalizeAnim, normalizePoint, DEFAULT_POINT, hsv,
} from "./anim-preview.js";

let preview = null;
let boardCanvas = null;   // offscreen Hue × Saturation gradient
let editPoints = [];      // working copy of the pattern
let selected = 0;
let dragging = -1;

const round1 = (v) => Math.round(v * 10) / 10;

// ── helpers ─────────────────────────────────────────────────────────

function isUserAnim(id) {
  return (state.config?.animations || []).some((a) => a.id === id);
}

function slug(name) {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "anim";
}

function movementLabel(id) {
  return t("anim.movement." + id);
}

function hexOf(p) {
  const [r, g, b] = hsv(p.hue, Math.max(0.05, p.sat), 1);
  const h = (n) => n.toString(16).padStart(2, "0");
  return `#${h(r)}${h(g)}${h(b)}`;
}

// ── colour board canvas ─────────────────────────────────────────────

function buildBoard(w, h) {
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  const ctx = c.getContext("2d");
  const img = ctx.createImageData(w, h);
  for (let y = 0; y < h; y++) {
    const sat = 1 - y / (h - 1);
    for (let x = 0; x < w; x++) {
      const [r, g, b] = hsv(x / (w - 1), sat, 1);
      const o = (y * w + x) * 4;
      img.data[o] = r; img.data[o + 1] = g; img.data[o + 2] = b; img.data[o + 3] = 255;
    }
  }
  ctx.putImageData(img, 0, 0);
  return c;
}

function canvasXY(p) {
  const canvas = $("#anim-board");
  return [p.hue * canvas.width, (1 - p.sat) * canvas.height];
}

function eventToHueSat(ev) {
  const canvas = $("#anim-board");
  const rect = canvas.getBoundingClientRect();
  const x = Math.min(1, Math.max(0, (ev.clientX - rect.left) / rect.width));
  const y = Math.min(1, Math.max(0, (ev.clientY - rect.top) / rect.height));
  return [x, 1 - y];
}

function redrawBoard() {
  const canvas = $("#anim-board");
  if (!canvas) return;
  const ctx = canvas.getContext("2d");
  if (!boardCanvas) boardCanvas = buildBoard(canvas.width, canvas.height);
  ctx.drawImage(boardCanvas, 0, 0);
  const pts = editPoints;

  ctx.lineWidth = 3;
  for (let i = 0; i < pts.length; i++) {
    const a = canvasXY(pts[i]);
    const b = canvasXY(pts[(i + 1) % pts.length]);
    ctx.setLineDash(pts[i].swift ? [8, 6] : []);
    ctx.strokeStyle = pts[i].swift ? "#ffffff" : hexOf(pts[i]);
    ctx.beginPath();
    ctx.moveTo(a[0], a[1]);
    ctx.lineTo(b[0], b[1]);
    ctx.stroke();
  }
  ctx.setLineDash([]);

  pts.forEach((p, i) => {
    const [x, y] = canvasXY(p);
    ctx.beginPath();
    ctx.arc(x, y, i === selected ? 11 : 8, 0, Math.PI * 2);
    ctx.fillStyle = hexOf(p);
    ctx.fill();
    ctx.lineWidth = i === selected ? 3 : 2;
    ctx.strokeStyle = i === selected ? "#ffffff" : "rgba(0,0,0,.6)";
    ctx.stroke();
    ctx.fillStyle = "#ffffff";
    ctx.font = "bold 11px sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(String(i + 1), x, y);
  });
}

// ── points table ────────────────────────────────────────────────────

function durationsSum() {
  return editPoints.reduce((s, p) => s + Math.max(0, p.duration), 0);
}

function updateDurationSum() {
  const el = $("#anim-duration-sum");
  if (el) el.textContent = t("anim.sumLabel", { sum: round1(durationsSum()) });
}

function renderPointTable() {
  const body = $("#anim-points-body");
  if (!body) return;
  body.innerHTML = editPoints.map((p, i) => `
    <tr class="anim-point-row${i === selected ? " sel" : ""}" data-i="${i}">
      <td class="pt-num">${i + 1}</td>
      <td><input type="color" class="pt-color" value="${hexOf(p)}" /></td>
      <td><input type="number" class="pt-bri" min="0" max="100" value="${Math.round(p.brightness * 100)}" /></td>
      <td><input type="number" class="pt-dur" min="0" max="100" step="0.5" value="${round1(p.duration)}" /></td>
      <td><input type="checkbox" class="pt-swift" ${p.swift ? "checked" : ""} /></td>
      <td><button class="btn ghost pt-del" type="button" aria-label="${t("anim.pointDelete")}">✕</button></td>
    </tr>`).join("");
  updateDurationSum();
}

function selectPoint(i) {
  selected = i;
  $$("#anim-points-body .anim-point-row").forEach((r) =>
    r.classList.toggle("sel", +r.dataset.i === i));
  redrawBoard();
}

/** Append a new point (default colour, duration 0) and select it. */
function addPoint(p) {
  editPoints.push(p || { ...normalizePoint(DEFAULT_POINT), duration: 0 });
  selected = editPoints.length - 1;
  renderPointTable();
  redrawBoard();
  updatePreview();
}

function clearMsg() {
  const msg = $("#anim-apply-msg");
  if (msg) { msg.textContent = ""; msg.className = "anim-msg"; }
}

/** Validate that the durations add up to 100 %; returns true when they do. */
function checkDurations() {
  const sum = round1(durationsSum());
  const ok = Math.abs(sum - 100) < 0.5;
  const msg = $("#anim-apply-msg");
  if (msg) {
    msg.textContent = ok ? t("anim.applyOk") : t("anim.applyErr", { sum });
    msg.className = "anim-msg " + (ok ? "ok" : "err");
  }
  return ok;
}

// ── editor ⇄ animation ──────────────────────────────────────────────

function fillForm(anim) {
  const a = normalizeAnim(anim);
  $("#anim-name").value = a.name || "";
  $("#anim-movement").value = MOVEMENT_IDS.includes(a.movement) ? a.movement : "rotate_right";
  $("#anim-speed").value = Math.round(a.speed * 100);
  $("#anim-cycles").value = a.cycles;
  $("#anim-brightness").value = Math.round(a.brightness * 100);
  editPoints = a.points.length ? a.points.map(normalizePoint) : [normalizePoint(DEFAULT_POINT)];
  selected = 0;
  state.animEditId = a.id && isUserAnim(a.id) ? a.id : null;
  updateMovementUi();
  ["anim-speed", "anim-brightness"].forEach((id) =>
    $("#" + id)?.dispatchEvent(new Event("input")));
  clearMsg();
  renderPointTable();
  redrawBoard();
  updatePreview();
}

function readForm() {
  return {
    id: state.animEditId || "",
    name: $("#anim-name").value.trim(),
    movement: $("#anim-movement").value,
    speed: (+$("#anim-speed").value || 0) / 100,
    cycles: Math.max(0.1, +$("#anim-cycles").value || 1),
    brightness: (+$("#anim-brightness").value || 0) / 100,
    fps: 30,
    points: editPoints.map((p) => ({ ...p })),
  };
}

function updatePreview() {
  preview?.setAnimation(readForm());
}

function updateMovementUi() {
  const moving = $("#anim-movement").value !== "stationary";
  $("#anim-speed-field")?.classList.toggle("hidden", !moving);
  const label = $("#anim-cycles-label");
  if (label) label.textContent = moving ? t("anim.cyclesRot") : t("anim.cyclesSec");
}

// ── rendering (gallery) ─────────────────────────────────────────────

function renderMovementOptions() {
  const select = $("#anim-movement");
  if (!select) return;
  const current = select.value;
  select.innerHTML = "";
  MOVEMENT_IDS.forEach((id) => select.appendChild(new Option(movementLabel(id), id)));
  select.value = MOVEMENT_IDS.includes(current) ? current : "rotate_right";
}

export function renderAnimations() {
  const box = $("#anim-gallery");
  if (!box) return;
  if (!state.animations.length) {
    box.innerHTML = `<p class="preset-empty">${t("anim.empty")}</p>`;
    return;
  }
  box.innerHTML = state.animations.map((a) => {
    const user = isUserAnim(a.id);
    return `<div class="anim-card">
      <div class="anim-card-text">
        <span class="anim-name">${escapeHtml(a.name)}</span>
        <span class="anim-kind-label">${escapeHtml(movementLabel(a.movement))}</span>
      </div>
      <div class="anim-card-actions">
        <button class="btn primary anim-run" type="button" data-id="${escapeHtml(a.id)}" data-tip="${t("anim.playShort")}">▶</button>
        <button class="btn anim-edit" type="button" data-id="${escapeHtml(a.id)}" data-tip="${t("anim.editTip")}">✎</button>
        ${user ? `<button class="btn ghost anim-del" type="button" data-id="${escapeHtml(a.id)}" data-tip="${t("anim.deleteTip")}">✕</button>` : ""}
      </div>
    </div>`;
  }).join("");

  box.querySelectorAll(".anim-run").forEach((b) =>
    b.addEventListener("click", () => playAnimation(animationById(b.dataset.id))));
  box.querySelectorAll(".anim-edit").forEach((b) =>
    b.addEventListener("click", () => fillForm(animationById(b.dataset.id))));
  box.querySelectorAll(".anim-del").forEach((b) =>
    b.addEventListener("click", async () => {
      await deleteAnimation(b.dataset.id);
      await reloadConfigRaw();
      renderAnimations();
    }));
}

// ── actions ─────────────────────────────────────────────────────────

async function playAnimation(anim) {
  if (!anim) return;
  const norm = normalizeAnim(anim);
  // Show what is playing on the preview strip, even when it is not the one
  // currently open in the editor.
  preview?.setAnimation(norm);
  const brightness = parseInt($("#brightness")?.value, 10);
  await invoke("animation_start", {
    animation: norm,
    fps: anim.fps || 30,
    brightness: Number.isFinite(brightness) ? brightness : null,
  }).catch(() => {});
}

async function saveFromForm() {
  const anim = readForm();
  if (!anim.name) {
    $("#anim-name").focus();
    return;
  }
  if (!checkDurations()) return;
  if (!anim.id) anim.id = `user-${slug(anim.name)}-${Date.now().toString(36)}`;
  await saveAnimation(anim);
  state.animEditId = anim.id;
  await reloadConfigRaw();
  renderAnimations();
}

// ── init ────────────────────────────────────────────────────────────

function initBoard() {
  const canvas = $("#anim-board");
  if (!canvas) return;
  canvas.addEventListener("mousedown", (ev) => {
    ev.preventDefault();
    const [hue, sat] = eventToHueSat(ev);
    const px = hue * canvas.width;
    const py = (1 - sat) * canvas.height;
    let hit = -1;
    let best = 16 * 16;
    editPoints.forEach((p, i) => {
      const [x, y] = canvasXY(p);
      const d = (x - px) * (x - px) + (y - py) * (y - py);
      if (d < best) { best = d; hit = i; }
    });
    if (hit >= 0) {
      selectPoint(hit);
      dragging = hit;
    } else {
      addPoint({ ...normalizePoint(DEFAULT_POINT), hue, sat, duration: 0 });
      dragging = selected;
    }
  });
  window.addEventListener("mousemove", (ev) => {
    if (dragging < 0) return;
    const [hue, sat] = eventToHueSat(ev);
    editPoints[dragging] = { ...editPoints[dragging], hue, sat };
    redrawBoard();
    updatePreview();
  });
  window.addEventListener("mouseup", () => { dragging = -1; });
}

function initPointsTable() {
  const body = $("#anim-points-body");
  if (!body) return;
  body.addEventListener("click", (ev) => {
    const row = ev.target.closest(".anim-point-row");
    if (!row) return;
    const i = +row.dataset.i;
    if (ev.target.closest(".pt-del")) {
      if (editPoints.length <= 1) return;
      editPoints.splice(i, 1);
      selected = Math.min(selected, editPoints.length - 1);
      renderPointTable();
      redrawBoard();
      updatePreview();
      return;
    }
    selectPoint(i);
  });
  body.addEventListener("input", (ev) => {
    const row = ev.target.closest(".anim-point-row");
    if (!row) return;
    const i = +row.dataset.i;
    if (ev.target.classList.contains("pt-bri")) {
      editPoints[i].brightness = Math.min(1, Math.max(0, (+ev.target.value || 0) / 100));
      redrawBoard();
      updatePreview();
    } else if (ev.target.classList.contains("pt-dur")) {
      editPoints[i].duration = Math.max(0, +ev.target.value || 0);
      updateDurationSum();
      updatePreview();
    } else if (ev.target.classList.contains("pt-color")) {
      const { h, s } = rgbToHs(hexToRgb(ev.target.value));
      editPoints[i].hue = h;
      editPoints[i].sat = s;
      redrawBoard();
      updatePreview();
    }
  });
  body.addEventListener("change", (ev) => {
    const row = ev.target.closest(".anim-point-row");
    if (!row) return;
    const i = +row.dataset.i;
    if (ev.target.classList.contains("pt-swift")) {
      editPoints[i].swift = ev.target.checked;
      redrawBoard();
      updatePreview();
    }
  });
}

export function initAnimations() {
  preview = new Preview($("#anim-preview"), 54);
  renderMovementOptions();
  bindSlider("anim-speed", "anim-speed-out", (v) => (v / 100).toFixed(2));
  bindSlider("anim-brightness", "anim-brightness-out", (v) => (v / 100).toFixed(2));
  initBoard();
  initPointsTable();

  $$("#panel-anim input, #panel-anim select").forEach((el) => {
    el.addEventListener("input", updatePreview);
    el.addEventListener("change", updatePreview);
  });

  $("#anim-movement")?.addEventListener("change", () => {
    updateMovementUi();
    updatePreview();
  });

  $("#anim-apply")?.addEventListener("click", () => checkDurations());
  $("#anim-add-point")?.addEventListener("click", () => addPoint());
  $("#anim-play")?.addEventListener("click", () => {
    if (!checkDurations()) return;
    playAnimation(readForm());
  });
  $("#anim-stop")?.addEventListener("click", () => invoke("animation_stop").catch(() => {}));
  $("#anim-save")?.addEventListener("click", () => saveFromForm());
  $("#anim-new")?.addEventListener("click", () => {
    fillForm({ ...normalizeAnim({}), name: "", points: [normalizePoint(DEFAULT_POINT)] });
  });

  editPoints = [normalizePoint(DEFAULT_POINT)];
  selected = 0;
  renderPointTable();
  redrawBoard();
  updatePreview();
  preview.start();
}

/** Load an animation into the editor (used after the list has loaded). */
export function editAnimation(anim) {
  if (anim) fillForm(anim);
}

/** (Re)build the panel when the language changes. */
export function refreshAnimations() {
  renderMovementOptions();
  renderAnimations();
  updateMovementUi();
  renderPointTable();
  redrawBoard();
  updatePreview();
}
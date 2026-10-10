// Small dependency-free DOM and formatting helpers.

export const $ = (sel, root = document) => root.querySelector(sel);
export const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));

export function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (c) => (
    { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]
  ));
}

export function hexToRgb(hex) {
  const v = String(hex).replace("#", "");
  return [
    parseInt(v.slice(0, 2), 16) || 0,
    parseInt(v.slice(2, 4), 16) || 0,
    parseInt(v.slice(4, 6), 16) || 0,
  ];
}

export function rgbToHex(rgb) {
  const p = (n) => Math.max(0, Math.min(255, Math.round(n || 0))).toString(16).padStart(2, "0");
  return `#${p(rgb[0])}${p(rgb[1])}${p(rgb[2])}`;
}

/** RGB (0..255) -> { h, s } with h, s in 0..1 (value/brightness is ignored). */
export function rgbToHs(rgb) {
  const r = rgb[0] / 255, g = rgb[1] / 255, b = rgb[2] / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const d = max - min;
  let h = 0;
  if (d !== 0) {
    if (max === r) h = ((g - b) / d) % 6;
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h /= 6;
    if (h < 0) h += 1;
  }
  return { h, s: max === 0 ? 0 : d / max };
}

export function clamp(value, lo, hi) {
  return Math.min(hi, Math.max(lo, value));
}

export function debounce(fn, ms = 120) {
  let timer = null;
  return (...args) => {
    clearTimeout(timer);
    timer = setTimeout(() => fn(...args), ms);
  };
}

/** Keep a range's `<output>` in sync with its value and return the input. */
export function bindSlider(id, outId, format) {
  const input = $("#" + id);
  const out = $("#" + outId);
  if (!input || !out) return input;
  const update = () => { out.textContent = format(input.value); };
  input.addEventListener("input", update);
  update();
  return input;
}
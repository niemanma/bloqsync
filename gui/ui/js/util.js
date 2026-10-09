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
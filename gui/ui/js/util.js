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
/** One floating tooltip for every `[data-tip]` element, clamped to the
 *  viewport so it is never clipped by the window or a panel. */
export function initTooltips() {
  let el = null;
  const ensure = () => {
    if (!el) {
      el = document.createElement("div");
      el.className = "bq-tooltip";
      document.body.appendChild(el);
    }
    return el;
  };
  const show = (target) => {
    const text = target.getAttribute("data-tip");
    if (!text) return;
    const tip = ensure();
    tip.textContent = text;
    tip.classList.add("show");
    const r = target.getBoundingClientRect();
    const t = tip.getBoundingClientRect();
    const pad = 8;
    let left = r.left + r.width / 2 - t.width / 2;
    left = Math.max(pad, Math.min(left, window.innerWidth - t.width - pad));
    let top = r.top - t.height - pad;
    if (top < pad) top = r.bottom + pad;                     // flip below
    top = Math.min(top, window.innerHeight - t.height - pad);
    tip.style.left = `${Math.round(left)}px`;
    tip.style.top = `${Math.round(Math.max(pad, top))}px`;
  };
  const hide = () => el && el.classList.remove("show");
  document.addEventListener("mouseover", (e) => {
    const t = e.target.closest && e.target.closest("[data-tip]");
    if (t) show(t);
  });
  document.addEventListener("mouseout", (e) => {
    const t = e.target.closest && e.target.closest("[data-tip]");
    if (t && !t.contains(e.relatedTarget)) hide();
  });
  document.addEventListener("focusin", (e) => {
    const t = e.target.closest && e.target.closest("[data-tip]");
    if (t) show(t);
  });
  document.addEventListener("focusout", hide);
  window.addEventListener("scroll", hide, true);
  window.addEventListener("blur", hide);
}

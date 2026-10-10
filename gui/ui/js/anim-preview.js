// Live LED-strip preview for animations. This is a faithful JavaScript port of
// the Rust renderer in `src/anim.rs`, used only to preview an animation while
// editing it; the bars themselves are driven by the Rust implementation.
//
// Model: a pattern is a closed chain of points on a Hue × Saturation board, each
// carrying a brightness and the vector (duration + swift) to the next point.
// A movement maps the pattern onto the strip over time.

const DEFAULTS = {
  id: "", name: "", movement: "rotate_right", speed: 0.25, cycles: 1,
  brightness: 0.9, fps: 30, points: [],
};

export const MOVEMENT_IDS = ["stationary", "rotate_right", "rotate_left", "march_right", "march_left"];

export function normalizePoint(p) {
  return {
    hue: Number(p?.hue ?? 0.08),
    sat: Number(p?.sat ?? 0.85),
    brightness: Number(p?.brightness ?? 1),
    duration: Math.max(0, Number(p?.duration ?? 100)) || 0,
    swift: !!p?.swift,
  };
}

export function normalizeAnim(a) {
  const out = { ...DEFAULTS, ...(a || {}) };
  out.speed = Number(out.speed ?? DEFAULTS.speed);
  out.cycles = Math.max(0.1, Number(out.cycles ?? DEFAULTS.cycles));
  out.brightness = Number(out.brightness ?? DEFAULTS.brightness);
  out.points = (out.points || []).map(normalizePoint);
  return out;
}

export const DEFAULT_POINT = normalizePoint({});

// ── colour helpers ──────────────────────────────────────────────────
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

export function hsv(h, s, v) {
  h = (((h % 1) + 1) % 1) * 6;
  const i = Math.floor(h);
  const f = h - i;
  const p = v * (1 - s);
  const q = v * (1 - s * f);
  const t = v * (1 - s * (1 - f));
  const m = [[v, t, p], [q, v, p], [p, v, t], [p, q, v], [t, p, v], [v, p, q]][((i % 6) + 6) % 6];
  return [Math.round(m[0] * 255), Math.round(m[1] * 255), Math.round(m[2] * 255)];
}
function scale(c, v) {
  v = clamp(v, 0, 1);
  return [Math.round(c[0] * v), Math.round(c[1] * v), Math.round(c[2] * v)];
}
function mix(a, b, t) {
  t = clamp(t, 0, 1);
  return [
    Math.round(a[0] + (b[0] - a[0]) * t),
    Math.round(a[1] + (b[1] - a[1]) * t),
    Math.round(a[2] + (b[2] - a[2]) * t),
  ];
}
const lerp = (a, b, t) => a + (b - a) * t;

function pointRgb(p) {
  return hsv(p.hue, clamp(p.sat, 0, 1), clamp(p.brightness, 0, 1));
}
function mixPoints(a, b, t, swift) {
  if (swift) return mix(pointRgb(a), pointRgb(b), t);
  return hsv(lerp(a.hue, b.hue, t), clamp(lerp(a.sat, b.sat, t), 0, 1), clamp(lerp(a.brightness, b.brightness, t), 0, 1));
}

/** Evaluate the pattern at cycle position `phase` (0..1). */
export function field(anim, phase) {
  const pts = anim.points;
  if (!pts.length) return [0, 0, 0];
  if (pts.length === 1) return scale(pointRgb(pts[0]), anim.brightness);
  const total = pts.reduce((s, p) => s + Math.max(0, p.duration), 0);
  if (total <= 0) return scale(pointRgb(pts[0]), anim.brightness);
  let ph = (((phase % 1) + 1) % 1) * total;
  for (let i = 0; i < pts.length; i++) {
    const d = Math.max(0, pts[i].duration);
    if (ph < d || i === pts.length - 1) {
      const t = d > 0 ? clamp(ph / d, 0, 1) : 0;
      const next = pts[(i + 1) % pts.length];
      return scale(mixPoints(pts[i], next, t, pts[i].swift), anim.brightness);
    }
    ph -= d;
  }
  return scale(pointRgb(pts[pts.length - 1]), anim.brightness);
}

export class AnimRenderer {
  constructor() { this.time = 0; }

  render(dt, n, anim) {
    if (n <= 0) return [];
    dt = clamp(dt, 0, 0.1);
    this.time += dt;
    const moving = anim.movement !== "stationary";
    const phase = moving
      ? this.time * clamp(anim.speed, 0, 4)
      : this.time / Math.max(0.1, anim.cycles);
    const ph = ((phase % 1) + 1) % 1;
    const out = [];
    for (let i = 0; i < n; i++) {
      const x = i / n;
      switch (anim.movement) {
        case "stationary": out.push(field(anim, ph)); break;
        case "rotate_left": out.push(field(anim, (((x + ph) % 1) + 1) % 1)); break;
        case "march_right": { const p = x - ph; out.push(p >= 0 && p < 1 ? field(anim, p) : [0, 0, 0]); break; }
        case "march_left": { const p = x + ph; out.push(p >= 0 && p < 1 ? field(anim, p) : [0, 0, 0]); break; }
        default: /* rotate_right */ out.push(field(anim, (((x - ph) % 1) + 1) % 1));
      }
    }
    return out;
  }
}

/** A DOM strip of LEDs that renders an animation live. */
export class Preview {
  constructor(el, n = 54) {
    this.el = el;
    this.n = n;
    this.renderer = new AnimRenderer();
    this.anim = normalizeAnim({ points: [DEFAULT_POINT] });
    this.running = false;
    this.raf = 0;
    this.last = 0;
    this.cells = [];
    this.el.innerHTML = "";
    for (let i = 0; i < n; i++) {
      const cell = document.createElement("span");
      cell.className = "led";
      this.el.appendChild(cell);
      this.cells.push(cell);
    }
    this.paint(this.renderer.render(0, n, this.anim));
  }

  setAnimation(anim) {
    this.anim = normalizeAnim(anim);
    if (!this.running) this.paint(this.renderer.render(0.016, this.n, this.anim));
  }

  start() {
    if (this.running) return;
    this.running = true;
    this.last = performance.now();
    const loop = (now) => {
      if (!this.running) return;
      const dt = (now - this.last) / 1000;
      this.last = now;
      this.paint(this.renderer.render(dt, this.n, this.anim));
      this.raf = requestAnimationFrame(loop);
    };
    this.raf = requestAnimationFrame(loop);
  }

  stop() {
    this.running = false;
    if (this.raf) cancelAnimationFrame(this.raf);
    this.raf = 0;
  }

  paint(cols) {
    for (let i = 0; i < this.n; i++) {
      const c = cols[i] || [0, 0, 0];
      this.cells[i].style.background = `rgb(${c[0]},${c[1]},${c[2]})`;
    }
  }
}
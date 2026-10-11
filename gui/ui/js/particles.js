// Subtle falling "rain drops" in the background (decorative).

import { $ } from "./util.js";

export function initParticles() {
  const canvas = $("#bg-canvas");
  if (!canvas) return;
  const ctx = canvas.getContext("2d");
  const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const COLORS = ["139,92,246", "34,211,238", "168,85,247"];
  let w = 0, h = 0;
  const drops = [];

  function resize() {
    w = canvas.width = window.innerWidth;
    h = canvas.height = window.innerHeight;
    const target = Math.min(220, Math.round((w * h) / 9000));
    drops.length = 0;
    for (let i = 0; i < target; i++) {
      drops.push({
        x: Math.random() * w,
        y: Math.random() * h,
        len: 12 + Math.random() * 26,
        vy: 1.6 + Math.random() * 3.4,
        vx: (Math.random() - 0.5) * 0.6,
        a: 0.25 + Math.random() * 0.5,
        c: COLORS[(Math.random() * COLORS.length) | 0],
      });
    }
  }
  resize();
  window.addEventListener("resize", resize);

  function frame() {
    ctx.clearRect(0, 0, w, h);
    ctx.lineCap = "round";
    for (const d of drops) {
      ctx.strokeStyle = `rgba(${d.c},${d.a})`;
      ctx.lineWidth = 1.4;
      ctx.beginPath();
      ctx.moveTo(d.x, d.y);
      ctx.lineTo(d.x - d.vx * 3, d.y - d.len);
      ctx.stroke();
      d.y += d.vy;
      d.x += d.vx;
      if (d.y - d.len > h) { d.y = -d.len; d.x = Math.random() * w; }
    }
    requestAnimationFrame(frame);
  }
  frame();
}

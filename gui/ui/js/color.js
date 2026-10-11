// Static-colour panel actions.

import { invoke } from "./api.js";
import { $, debounce, hexToRgb, rgbToHex } from "./util.js";
import { saveConfig, setMode } from "./state.js";
import { recolorSelected } from "./anims.js";

export function initColor() {
  document.querySelectorAll(".sw[data-rgb]").forEach((swatch) => {
    swatch.addEventListener("click", async () => {
      const [r, g, b] = swatch.dataset.rgb.split(",").map(Number);
      if (await recolorSelected(r, g, b)) return;
      invoke("set_all_color", { r, g, b }).catch(() => {});
    });
  });

  // Cinema base colour – same swatch control as Effects, but it sets the
  // cinema colour (used when the Cinema mode starts).
  document.querySelectorAll(".sw[data-cin]").forEach((swatch) => {
    swatch.addEventListener("click", () => {
      const [r, g, b] = swatch.dataset.cin.split(",").map(Number);
      const input = $("#cin-color");
      if (!input) return;
      input.value = rgbToHex([r, g, b]);
      input.dispatchEvent(new Event("input"));
      input.dispatchEvent(new Event("change"));
    });
  });

  const custom = $("#custom-color");
  if (custom) {
    const apply = debounce(async () => {
      const [r, g, b] = hexToRgb(custom.value);
      if (await recolorSelected(r, g, b)) return;
      invoke("set_all_color", { r, g, b }).catch(() => {});
    }, 120);
    custom.addEventListener("input", apply);
    custom.addEventListener("change", () => saveConfig());
  }

  $("#resume-sync")?.addEventListener("click", () => {
    invoke("resume_sync").catch(() => {});
    setMode("screen");
  });
}
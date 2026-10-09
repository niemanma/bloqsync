// Static-colour panel actions.

import { invoke } from "./api.js";
import { $, debounce, hexToRgb } from "./util.js";
import { saveConfig, setMode } from "./state.js";

export function initColor() {
  document.querySelectorAll(".sw[data-rgb]").forEach((swatch) => {
    swatch.addEventListener("click", () => {
      const [r, g, b] = swatch.dataset.rgb.split(",").map(Number);
      invoke("set_all_color", { r, g, b }).catch(() => {});
    });
  });

  const custom = $("#custom-color");
  if (custom) {
    const apply = debounce(() => {
      const [r, g, b] = hexToRgb(custom.value);
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
// Expert-mode management of bar models (built-in + user-defined).

import { $, escapeHtml } from "./util.js";
import { state, saveBarModel, deleteBarModel, reloadConfigRaw } from "./state.js";
import { renderBars } from "./screen.js";

export function renderModels() {
  const box = $("#model-list");
  if (!box) return;
  const userIds = new Set((state.config?.bar_models || []).map((m) => m.id));
  box.innerHTML = state.barModels.map((m) => {
    const sum = m.left + m.top + m.bottom + m.right;
    const meta = `${m.leds} LEDs · ${m.left}/${m.top}/${m.bottom}/${m.right}` +
      (sum !== m.leds ? ` = ${sum} ⚠` : "");
    const del = userIds.has(m.id)
      ? `<button class="btn ghost model-del" type="button" data-id="${escapeHtml(m.id)}">✕</button>`
      : "";
    return `<div class="device-row">
      <span class="dev-id">${escapeHtml(m.name)}</span>
      <span class="dev-meta">${meta}</span>
      ${del}
    </div>`;
  }).join("");

  box.querySelectorAll(".model-del").forEach((button) => {
    button.addEventListener("click", async () => {
      await deleteBarModel(button.dataset.id);
      await reloadConfigRaw();
      renderModels();
      renderBars();
    });
  });
}

export function initModels() {
  const form = $("#model-form");

  const openForm = () => {
    if (!form) return;
    form.classList.remove("hidden");
    $("#model-name")?.focus();
  };
  const closeForm = () => {
    form?.classList.add("hidden");
  };

  $("#model-add")?.addEventListener("click", openForm);
  $("#model-cancel")?.addEventListener("click", closeForm);

  $("#model-save")?.addEventListener("click", async () => {
    const name = $("#model-name").value.trim();
    if (!name) {
      $("#model-name").focus();
      return;
    }
    const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
    const model = {
      id: `user-${slug || "model"}-${Date.now().toString(36)}`,
      name,
      match: {
        vendor_id: ($("#model-vendor")?.value || "1a86").trim(),
        product_id: ($("#model-product")?.value || "fe07").trim(),
        usage_page: ($("#model-usage")?.value || "ff00").trim(),
      },
      leds: +$("#model-leds").value || 0,
      left: +$("#model-left").value || 0,
      top: +$("#model-top").value || 0,
      bottom: +$("#model-bottom").value || 0,
      right: +$("#model-right").value || 0,
      protocol: {
        report_size: +($("#model-report")?.value || 64),
        max_sections_per_frame: +($("#model-sections")?.value || 11),
        max_leds: +($("#model-maxleds")?.value || 254),
        inter_frame_gap_ms: +($("#model-gap")?.value || 3),
      },
    };
    await saveBarModel(model);
    await reloadConfigRaw();
    $("#model-name").value = "";
    closeForm();
    renderModels();
    renderBars();
  });
}
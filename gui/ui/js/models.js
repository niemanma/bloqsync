// Expert-mode management of bar models (built-in + user-defined).

import { $, escapeHtml } from "./util.js";
import { state, saveBarModel, deleteBarModel, reloadConfigRaw } from "./state.js";
import { renderBars } from "./screen.js";
import { t } from "./i18n.js";

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
    const msg = $("#model-msg");
    const leds = +$("#model-leds").value || 0;
    const left = +$("#model-left").value || 0;
    const top = +$("#model-top").value || 0;
    const bottom = +$("#model-bottom").value || 0;
    const right = +$("#model-right").value || 0;
    if (leds <= 0 || left + top + bottom + right !== leds) {
      if (msg) {
        msg.textContent = t("models.sumError", { sum: left + top + bottom + right, leds });
        msg.className = "anim-msg err";
      }
      return;
    }
    if (msg) { msg.textContent = ""; msg.className = "anim-msg"; }
    const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
    const model = {
      id: `user-${slug || "model"}-${Date.now().toString(36)}`,
      name,
      match: {
        vendor_id: ($("#model-vendor")?.value || "1a86").trim(),
        product_id: ($("#model-product")?.value || "fe07").trim(),
        usage_page: ($("#model-usage")?.value || "ff00").trim(),
      },
      leds,
      left,
      top,
      bottom,
      right,
      protocol: {
        report_size: +($("#model-report")?.value || 64),
        max_sections_per_frame: +($("#model-sections")?.value || 11),
        max_leds: +($("#model-maxleds")?.value || 254),
        inter_frame_gap_ms: +($("#model-gap")?.value || 3),
      },
    };
    try {
      await saveBarModel(model);
    } catch (error) {
      if (msg) { msg.textContent = String(error); msg.className = "anim-msg err"; }
      return;
    }
    await reloadConfigRaw();
    $("#model-name").value = "";
    closeForm();
    renderModels();
    renderBars();
  });
}
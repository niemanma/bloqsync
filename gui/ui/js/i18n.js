// Minimal i18n: language dictionaries + data-i18n attributes.
//
// Add a language by dropping a dictionary into ./lang/<code>.js and listing it
// in LANGS. Missing keys fall back to English.

import en from "./lang/en.js";
import de from "./lang/de.js";

const LANGS = { en, de };
const LANG_KEY = "bloqsync.lang";
let current = "en";

export function availableLanguages() {
  return Object.keys(LANGS);
}

export function getLanguage() {
  return current;
}

/** Translate a key, optionally interpolating `{placeholders}`. */
export function t(key, vars) {
  const dict = LANGS[current] || en;
  let text = dict[key] ?? en[key] ?? key;
  if (vars) {
    for (const [name, value] of Object.entries(vars)) {
      text = text.replaceAll(`{${name}}`, String(value));
    }
  }
  return text;
}

function applyAttrs(el) {
  const text = el.getAttribute("data-i18n");
  if (text) el.textContent = t(text);
  const title = el.getAttribute("data-i18n-title");
  if (title) el.setAttribute("title", t(title));
  const tip = el.getAttribute("data-i18n-tip");
  if (tip) el.setAttribute("data-tip", t(tip));
  const placeholder = el.getAttribute("data-i18n-placeholder");
  if (placeholder) el.setAttribute("placeholder", t(placeholder));
  const aria = el.getAttribute("data-i18n-aria");
  if (aria) el.setAttribute("aria-label", t(aria));
}

export function applyTranslations(root = document) {
  root
    .querySelectorAll(
      "[data-i18n], [data-i18n-title], [data-i18n-tip], [data-i18n-placeholder], [data-i18n-aria]",
    )
    .forEach(applyAttrs);
  document.documentElement.lang = current;
}

export function setLanguage(code) {
  current = LANGS[code] ? code : "en";
  localStorage.setItem(LANG_KEY, current);
  applyTranslations();
  window.dispatchEvent(new CustomEvent("language-changed", { detail: current }));
  return current;
}

/** Initialise from the stored preference (default English). */
export function initLanguage() {
  const saved = localStorage.getItem(LANG_KEY) || "en";
  current = LANGS[saved] ? saved : "en";
  applyTranslations();
  return current;
}
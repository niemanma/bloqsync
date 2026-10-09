// Cinema mode (audio-reactive) actions.

import { invoke } from "./api.js";
import { $ } from "./util.js";
import { cinemaSettings, setMode } from "./state.js";

export function initCinema() {
  $("#cinema-start")?.addEventListener("click", async () => {
    try {
      await invoke("cinema_start", cinemaSettings());
      setMode("cinema");
    } catch (_) { /* ignored */ }
  });
  $("#cinema-stop")?.addEventListener("click", () => {
    invoke("cinema_stop").catch(() => {});
  });
}
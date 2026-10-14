// Thin wrapper around the Tauri globals injected by `withGlobalTauri`.
const tauri = window.__TAURI__ || {};

// Commands polled on a timer: a failure here is transient and must not spam the
// user with error toasts.
const QUIET = new Set(["live_state", "status", "capture_info"]);

let lastToast = { msg: "", at: 0 };

/** Show a transient error banner (so IPC failures are never silent). */
function toast(msg) {
  const now = Date.now();
  if (msg === lastToast.msg && now - lastToast.at < 4000) return;
  lastToast = { msg, at: now };
  let el = document.getElementById("bq-toast");
  if (!el) {
    el = document.createElement("div");
    el.id = "bq-toast";
    el.className = "bq-toast";
    document.body.appendChild(el);
  }
  el.textContent = msg;
  el.classList.add("show");
  clearTimeout(el._timer);
  el._timer = setTimeout(() => el.classList.remove("show"), 4000);
}

export const invoke = async (cmd, args) => {
  try {
    return await tauri.core.invoke(cmd, args);
  } catch (error) {
    const msg = String((error && error.message) || error || "Unbekannter Fehler");
    if (!QUIET.has(cmd)) toast(msg);
    throw error;
  }
};

export const listen = (name, cb) => tauri.event.listen(name, cb);

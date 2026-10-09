// Thin wrapper around the Tauri globals injected by `withGlobalTauri`.
const tauri = window.__TAURI__ || {};

export const invoke = (cmd, args) => tauri.core.invoke(cmd, args);
export const listen = (name, cb) => tauri.event.listen(name, cb);
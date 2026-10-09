// Human-friendly monitor naming and the schematic layout map.

export function monitorLabel(index) {
  return `Monitor ${index + 1}`;
}

export function monitorOptionLabel(stream, expert = false) {
  const base = monitorLabel(stream.index);
  if (expert && stream.size) return `${base} (${stream.size[0]}×${stream.size[1]})`;
  return base;
}

/** Render a schematic overview of the monitor arrangement into `container`. */
export function renderMonmap(container, streams) {
  if (!container) return;
  const hasGeo = streams.length > 0 && streams.every((s) => s.position && s.size);
  if (!hasGeo) {
    container.innerHTML = "";
    container.style.width = "";
    container.style.height = "";
    return;
  }

  const minX = Math.min(...streams.map((s) => s.position[0]));
  const minY = Math.min(...streams.map((s) => s.position[1]));
  const maxX = Math.max(...streams.map((s) => s.position[0] + s.size[0]));
  const maxY = Math.max(...streams.map((s) => s.position[1] + s.size[1]));
  const width = maxX - minX || 1;
  const height = maxY - minY || 1;
  const scale = Math.min(560 / width, 220 / height);

  container.style.width = `${Math.round(width * scale)}px`;
  container.style.height = `${Math.round(height * scale)}px`;
  container.innerHTML = streams.map((s) => {
    const x = (s.position[0] - minX) * scale;
    const y = (s.position[1] - minY) * scale;
    const w = s.size[0] * scale;
    const h = s.size[1] * scale;
    return `<div class="monbox" style="left:${Math.round(x)}px;top:${Math.round(y)}px;width:${Math.round(w)}px;height:${Math.round(h)}px">
      <span class="monnum">${s.index + 1}</span>
      <span class="monres">${s.size[0]}×${s.size[1]}</span>
    </div>`;
  }).join("");
}
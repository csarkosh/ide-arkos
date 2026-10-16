// Durations as people write them: "30s", "5m", "2h", "1h30m".
const UNITS = { s: 1000, m: 60_000, h: 3_600_000, d: 86_400_000 };

export function parseDuration(text) {
  if (typeof text !== 'string' || !text.trim()) return null;
  let total = 0;
  const re = /(\d+)\s*([smhd])/gy;
  let m;
  let end = 0;
  while ((m = re.exec(text.trim()))) {
    total += Number(m[1]) * UNITS[m[2]];
    end = re.lastIndex;
  }
  return end === text.trim().length && total > 0 ? total : null;
}

export function formatDuration(ms) {
  if (ms >= UNITS.d) return `${Math.round(ms / UNITS.d)}d`;
  if (ms >= UNITS.h) return `${Math.round(ms / UNITS.h)}h`;
  if (ms >= UNITS.m) return `${Math.round(ms / UNITS.m)}m`;
  return `${Math.round(ms / UNITS.s)}s`;
}

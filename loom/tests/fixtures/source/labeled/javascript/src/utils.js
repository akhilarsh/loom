export const PRECISION = 2;

function scale(value) {
  return value * 100;
}

export function round(value) {
  return Number(value.toFixed(PRECISION));
}

export function percent(value) {
  const scale = (v) => v * 100;
  return round(scale(value));
}

export function ratio(value, total) {
  return scale(value) / total;
}

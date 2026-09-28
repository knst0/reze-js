export const kib = (bytes) => (bytes / 1024).toFixed(2);
export const signed = (value, format) => (value > 0 ? "+" : "") + format(value);
export const ratio = (value, base) => (base > 0 ? `${(value / base).toFixed(2)}×` : "");
export const ms = (value) => value.toFixed(2);
export const us = (value) => value.toFixed(1);

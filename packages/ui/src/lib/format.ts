const locale = 'en-GB';

export const formatMoney = (value: number, currency = 'EUR', decimals = 0) =>
  new Intl.NumberFormat(locale, { style: 'currency', currency, minimumFractionDigits: decimals, maximumFractionDigits: decimals }).format(value);

export const formatInt = (value: number) => new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(value);

export const formatPercent = (value: number, decimals = 1) =>
  new Intl.NumberFormat(locale, { style: 'percent', minimumFractionDigits: decimals, maximumFractionDigits: decimals }).format(value);

/** A sign on every change, with a true minus. */
export const formatSigned = (value: number, format: (v: number) => string = formatInt) =>
  (value > 0 ? '+' : value < 0 ? '−' : '') + format(Math.abs(value));

/** Minutes since midnight as a 24-hour clock. */
export const formatClock = (minutes: number) =>
  `${String(Math.floor(minutes / 60) % 24).padStart(2, '0')}:${String(Math.round(minutes % 60)).padStart(2, '0')}`;

/** "1 h 42 m" or "42 m" from minutes. */
export const formatDuration = (minutes: number) => {
  const h = Math.floor(minutes / 60);
  const m = Math.round(minutes % 60);
  return h ? `${h} h ${String(m).padStart(2, '0')} m` : `${m} m`;
};

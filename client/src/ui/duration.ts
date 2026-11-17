// Durations in game time and in real time. At fast speeds a game day can be
// a few seconds, so "closes in 2 days" alone misleads: every countdown (offer
// expiry, auctions, listings, rallies) also says roughly how long that is on
// the player's clock, from the measured tick rate (`TickClock.msPerTick`).

/** Ticks in a game day when the server doesn't say (sim `WorldConfig` default). */
export const DEFAULT_TICKS_PER_DAY = 48;

/** "≈ 40 s", "≈ 3 min", "≈ 2 h 10 min", "≈ 3 days" (real time). */
export function realTime(ms: number): string {
  const s = Math.max(0, ms) / 1000;
  if (s < 90) return `≈ ${Math.max(1, Math.round(s))} s`;
  const min = s / 60;
  if (min < 90) return `≈ ${Math.round(min)} min`;
  const h = min / 60;
  if (h < 36) {
    const whole = Math.floor(h);
    const rest = Math.round((h - whole) * 60);
    return `≈ ${whole} h${rest ? ` ${rest} min` : ''}`;
  }
  return `≈ ${Math.round(h / 24)} days`;
}

/** Game time: "5 h" under a day, else "2.5 days" (one decimal below 10 days). */
export function gameTime(ticks: number, ticksPerDay = DEFAULT_TICKS_PER_DAY): string {
  const days = Math.max(0, ticks) / ticksPerDay;
  if (days < 1) return `${Math.max(1, Math.round(days * 24))} h`;
  return `${days < 10 ? days.toFixed(1) : Math.round(days)} days`;
}

/** "2.5 days (≈ 12 s)": game time, then real time at the measured speed. */
export function gameAndReal(ticks: number, ticksPerDay: number, msPerTick: number): string {
  return `${gameTime(ticks, ticksPerDay)} (${realTime(ticks * msPerTick)})`;
}

/** Ticks until the next season starts (seasons are quarters of the year; `Season::from_day`). */
export function ticksToNextSeason(tick: number, ticksPerDay: number, daysPerYear: number): number {
  const year = Math.max(4, daysPerYear) * ticksPerDay;
  const inYear = tick % year;
  const quarter = Math.floor((Math.floor(inYear / ticksPerDay) * 4) / Math.max(4, daysPerYear));
  // The first day of the next quarter, as `Season::from_day` counts (floor(day * 4 / year)).
  let day = Math.floor(inYear / ticksPerDay) + 1;
  while (Math.floor((day * 4) / Math.max(4, daysPerYear)) === quarter && day < daysPerYear) day++;
  return Math.max(0, day * ticksPerDay - inYear);
}

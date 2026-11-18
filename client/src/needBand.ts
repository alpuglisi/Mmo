/**
 * The range of a need over the last game day (playtest 5: an animal's hunger
 * read 14, 41, 87 and 55 within a day; each meal drops it at once and a
 * carnivore burns 40–57 a day). The inspector shows the live value with this
 * band beside it, from the states the client has seen.
 */
export class NeedBand {
  private samples = new Map<number, { tick: number; v: number }[]>();

  /** Record organism `id`'s reading `v` at game tick `tick`, keeping one day of them. */
  add(id: number, tick: number, v: number, ticksPerDay: number): void {
    let s = this.samples.get(id);
    if (!s) {
      // Only a few organisms are watched at a time.
      if (this.samples.size >= 32) this.samples.delete(this.samples.keys().next().value as number);
      s = [];
      this.samples.set(id, s);
    }
    const last = s[s.length - 1];
    if (last && last.tick === tick) last.v = v;
    else s.push({ tick, v });
    while (s.length > 0 && s[0].tick < tick - ticksPerDay) s.shift();
  }

  /** The last day's lowest, highest and mean reading, once it spans a tenth of a day or more. */
  band(id: number, ticksPerDay: number): { lo: number; hi: number; mean: number } | null {
    const s = this.samples.get(id);
    if (!s || s.length < 3 || s[s.length - 1].tick - s[0].tick < ticksPerDay / 10) return null;
    let lo = Infinity, hi = -Infinity, sum = 0;
    for (const x of s) {
      lo = Math.min(lo, x.v);
      hi = Math.max(hi, x.v);
      sum += x.v;
    }
    return { lo, hi, mean: sum / s.length };
  }
}

/** The band in words: "32–87 over the last day, about 52 on average". */
export function bandText(b: { lo: number; hi: number; mean: number } | null): string | null {
  if (!b || b.hi - b.lo < 10) return null;
  return `${Math.round(b.lo)}–${Math.round(b.hi)} over the last day, about ${Math.round(b.mean)} on average`;
}

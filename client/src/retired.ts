// Which lineage the player retired (playtest 2: after "Retire lineage" the
// game said "Your lineage has died out"). Kept for the browser session, so a
// reload still words it right.

const KEY = 'evo.retiredLineage';

export function markRetired(lineage: number): void {
  try {
    sessionStorage.setItem(KEY, String(lineage));
  } catch {
    /* private mode: this page only */
  }
  current = lineage;
}

let current: number | null = null;

/** Whether `lineage` is the one the player retired. */
export function wasRetired(lineage: number | null | undefined): boolean {
  if (lineage === null || lineage === undefined) return false;
  if (current === null) {
    try {
      const v = sessionStorage.getItem(KEY);
      current = v === null ? null : Number(v);
    } catch {
      /* private mode */
    }
  }
  return current === lineage;
}

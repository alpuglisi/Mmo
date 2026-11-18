// The design a player founded last, kept in this browser so the design
// screen starts from it after a retirement or an extinction (playtest 4:
// "the designer forgets the previous design"). The name starts empty: a new
// lineage gets a new name. Only known genes and valid values come back.

import { newEditorDesign, type EditorDesign } from './designMorph';
import { clampMorphology } from './morphology';
import type { GeneCatalog, PlayerId } from './protocol';

const key = (player: PlayerId | undefined) => `evo.lastDesign.${player ?? 'anon'}`;

export function saveLastDesign(player: PlayerId | undefined, d: EditorDesign): void {
  try {
    localStorage.setItem(key(player), JSON.stringify({ ...d, name: '' }));
  } catch {
    /* private mode or full: the next design starts from the starter */
  }
}

/** The last founded design checked against today's catalogue, or null. */
export function restoreDesign(raw: unknown, cat: GeneCatalog): EditorDesign | null {
  if (!raw || typeof raw !== 'object') return null;
  const r = raw as Partial<EditorDesign>;
  const base = newEditorDesign(cat);
  const int = (v: unknown, lo: number, hi: number, dflt: number) =>
    typeof v === 'number' && Number.isInteger(v) && v >= lo && v <= hi ? v : dflt;
  const traits = { ...base.traits };
  if (r.traits && typeof r.traits === 'object') {
    for (const g of cat.genes) {
      const v = (r.traits as Record<string, unknown>)[g.name];
      if (typeof v === 'number' && Number.isFinite(v) && v >= 0 && v <= 1) traits[g.name] = v;
    }
  }
  return {
    name: '',
    thermo: r.thermo === 'cold' || r.thermo === 'warm' ? r.thermo : base.thermo,
    birth: r.birth === 'live' || r.birth === 'incubated_eggs' || r.birth === 'abandoned_eggs' ? r.birth : base.birth,
    litter_size: int(r.litter_size, 1, cat.max_litter, base.litter_size),
    founders: int(r.founders, 2, 400, base.founders),
    traits,
    morphology: clampMorphology(r.morphology ?? {}, base.morphology),
  };
}

export function loadLastDesign(player: PlayerId | undefined, cat: GeneCatalog): EditorDesign | null {
  try {
    const s = localStorage.getItem(key(player));
    return s ? restoreDesign(JSON.parse(s), cat) : null;
  } catch {
    return null;
  }
}

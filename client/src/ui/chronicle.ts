// The World tab: the active catastrophe, the era clock and finished eras'
// rankings, the species tree, catastrophes and achievements (from
// `/api/species`, `/api/eras`, `/api/catastrophes`, `/api/achievements`).
// The data preparation is pure and unit-tested; `renderWorld` only builds DOM.

import type {
  AchievementList, CatastropheList, CatastropheView, EraList, PlayerId, SpeciesList, SpeciesView,
} from '../protocol';
import { cssColor, ownerColor, OWN_COLOR } from '../world/colors';
import { h, pct } from './dom';

export interface Chronicle {
  species: SpeciesList;
  eras: EraList;
  catastrophes: CatastropheList;
  achievements: AchievementList;
}

const CATASTROPHE_LABELS: Record<string, string> = {
  severe_winter: 'Severe winter',
  megadrought: 'Megadrought',
  epidemic: 'Epidemic',
  flood_year: 'Flood year',
};

const CATASTROPHE_EFFECTS: Record<string, string> = {
  severe_winter: 'every tile is about 10 °C colder than a normal winter',
  megadrought: 'the rain has failed: soil dries out, lakes, marsh and streams shrink, plants stop growing',
  epidemic: 'a virulent strain spreads: many organisms fall sick at once and more every day',
  flood_year: 'relentless rain: soaked soil, swollen lakes and marsh, deep water that weak swimmers cannot cross',
};

export function catastropheLabel(kind: string): string {
  return CATASTROPHE_LABELS[kind] ?? kind.replace(/_/g, ' ');
}

/** The banner text for the catastrophe under way, or null. */
export function catastropheBanner(c: CatastropheView | undefined | null, today: number): string | null {
  if (!c || !c.active) return null;
  const left = Math.max(0, c.end_day - today);
  const effect = CATASTROPHE_EFFECTS[c.kind];
  return `${catastropheLabel(c.kind)}${effect ? `: ${effect}` : ''} · ${left} day${left === 1 ? '' : 's'} left`;
}

/** One row of the species tree: a species and its depth below its root. */
export interface SpeciesRow {
  species: SpeciesView;
  depth: number;
  alive: boolean;
  /** Short status: "~123 adults", "extinct day 40" or "merged into Voles". */
  status: string;
}

/**
 * The species as a tree: roots (no parent, or a parent not in the list) in
 * the server's order (living first, largest first), each followed by its
 * daughter species, depth first.
 */
export function speciesTree(list: SpeciesView[]): SpeciesRow[] {
  const byId = new Map(list.map((s) => [s.id, s]));
  const children = new Map<number, SpeciesView[]>();
  for (const s of list) {
    if (s.parent !== undefined && byId.has(s.parent) && s.parent !== s.id) {
      const c = children.get(s.parent) ?? [];
      c.push(s);
      children.set(s.parent, c);
    }
  }
  const rows: SpeciesRow[] = [];
  const seen = new Set<number>();
  const visit = (s: SpeciesView, depth: number) => {
    if (seen.has(s.id)) return;
    seen.add(s.id);
    rows.push({ species: s, depth, alive: s.extinct_day === undefined, status: speciesStatus(s, byId) });
    for (const c of children.get(s.id) ?? []) visit(c, depth + 1);
  };
  for (const s of list) {
    if (s.parent === undefined || !byId.has(s.parent)) visit(s, 0);
  }
  // Anything left (a cycle should not happen) goes at the top level.
  for (const s of list) visit(s, 0);
  return rows;
}

function speciesStatus(s: SpeciesView, byId: Map<number, SpeciesView>): string {
  if (s.merged_into !== undefined) return `merged into ${byId.get(s.merged_into)?.name ?? `#${s.merged_into}`}`;
  if (s.extinct_day !== undefined) return `extinct day ${s.extinct_day + 1}`;
  return `~${Math.round(s.adults)} adults`;
}

/** Diet word for a mean carnivory. */
export function dietLabel(carnivory: number): string {
  return carnivory < 0.3 ? 'herbivore' : carnivory < 0.6 ? 'omnivore' : 'carnivore';
}

/** "diet:carnivore" → "carnivores", "biome:forest" → "forest". */
export function nicheLabel(niche: string): string {
  const [axis, what] = niche.includes(':') ? niche.split(':', 2) : ['', niche];
  return axis === 'diet' ? `${what}s` : what;
}

/** The player's place in each finished era (1-based among players, wild left out). */
export function myEraRanks(eras: EraList, me: PlayerId): { era: number; rank: number | null; share: number }[] {
  return eras.eras.map((e) => {
    const players = e.rows.filter((r) => r.player !== 0);
    const k = players.findIndex((r) => r.player === me);
    return { era: e.era, rank: k < 0 ? null : k + 1, share: k < 0 ? 0 : players[k].share };
  });
}

const swatch = (p: PlayerId, me: PlayerId) => h('span', { class: 'swatch', style: `background:${cssColor(p === me ? OWN_COLOR : ownerColor(p))}` });

export function renderWorld(el: HTMLElement, c: Chronicle | null, me: PlayerId, today: number): void {
  if (!c) {
    el.replaceChildren(h('p', { class: 'muted' }, 'Loading the world chronicle…'));
    return;
  }
  const { species, eras, catastrophes, achievements } = c;
  const banner = catastropheBanner(catastrophes.active, today);
  const tree = speciesTree(species.species);
  const myRanks = new Map(myEraRanks(eras, me).map((r) => [r.era, r]));
  const open = new Set([...el.querySelectorAll<HTMLDetailsElement>('details[data-key]')].filter((d) => d.open).map((d) => d.dataset.key));
  const parts: (Node | null)[] = [
    banner ? h('div', { class: 'catastrophe-banner', id: 'world-catastrophe' }, banner) : null,
    h('h3', {}, `Era ${eras.era + 1}`),
    h('p', { class: 'muted small', id: 'era-clock' },
      `Days ${eras.first_day + 1}–${eras.last_day + 1} · ${eras.days_left} day${eras.days_left === 1 ? '' : 's'} left. `
      + 'At the end of each era the legacy ranking is final and niche leaders earn prestige.'),
    h('h3', {}, 'Era leaderboards'),
    eras.eras.length === 0
      ? h('p', { class: 'muted small' }, 'No era has ended yet.')
      : h('div', { id: 'era-boards' }, ...eras.eras.map((e) => {
        const mine = myRanks.get(e.era);
        return h('details', { class: 'season', 'data-key': `era-${e.era}`, open: open.has(`era-${e.era}`) || e === eras.eras[0] || undefined },
          h('summary', {}, `Era ${e.era + 1} · days ${e.first_day + 1}–${e.last_day + 1}${mine?.rank ? ` · you #${mine.rank}` : ''}`
            + `${e.catastrophes.length ? ` · ${e.catastrophes.length} catastrophe${e.catastrophes.length === 1 ? '' : 's'}` : ''}`),
          h('table', { class: 'board' },
            h('thead', {}, h('tr', {}, h('th', {}, '#'), h('th', {}, 'Player'), h('th', {}, 'Final share'), h('th', { title: 'Weighted by offspring raised to adulthood' }, 'Weighted'))),
            h('tbody', {}, ...e.rows.slice(0, 8).map((r, k) => h('tr', { class: r.player === me ? 'me' : '' },
              h('td', {}, String(k + 1)), h('td', {}, swatch(r.player, me), r.name), h('td', {}, pct(r.share)), h('td', {}, pct(r.weighted_share)))))),
          e.niches.length > 0 && h('p', { class: 'muted small' }, 'Niche leaders: ',
            e.niches.map((n) => `${nicheLabel(n.niche)} ${n.name} ${pct(n.share, 0)}`).join(' · ')));
      })),
    h('h3', {}, `Species (${species.alive} living)`),
    h('p', { class: 'muted small' },
      species.detected_day === undefined
        ? 'Species are detected every few days.'
        : `Detected on day ${species.detected_day + 1} from ${species.samples} sampled adults: organisms that recognize each other as kin and are fertile together form one species.`),
    tree.length === 0 ? null : h('ul', { class: 'species-tree', id: 'species-list' }, ...tree.map((r) => {
      const s = r.species;
      return h('li', { class: `species${r.alive ? '' : ' extinct'}`, style: `padding-left:${r.depth * 14}px`, 'data-species': s.id },
        r.depth > 0 ? h('span', { class: 'muted' }, '└ ') : null,
        swatch(s.dominant, me),
        h('strong', {}, s.name),
        h('span', { class: 'muted small' }, ` · ${dietLabel(s.carnivory)} · ${r.status} · ${s.dominant_name} ${pct(s.dominant_share, 0)} of alleles`
          + `${s.founder_name ? ` · founded by ${s.founder_name}` : ''} · since day ${s.founded_day + 1}`));
    })),
    species.founded.length > 0 ? h('p', { class: 'small' }, 'Species founded: ', species.founded.map((f) => `${f.name} ${f.count}`).join(' · ')) : null,
    h('h3', {}, 'Catastrophes'),
    catastrophes.history.length === 0
      ? h('p', { class: 'muted small' }, catastrophes.enabled ? 'None yet. They strike now and then from the second year on.' : 'Catastrophes are off in this world.')
      : h('ul', { class: 'feed-list', id: 'catastrophe-list' }, ...catastrophes.history.map((k) => h('li', { class: 'feed-item' },
        h('strong', {}, catastropheLabel(k.kind)),
        h('span', { class: 'muted small' }, ` · days ${k.start_day + 1}–${k.end_day}${k.active ? ' (now)' : ''} · population ${k.population_before}`
          + `${k.population_after !== undefined ? ` → ${k.population_after}` : ''}`
          + `${k.active ? '' : ` · ${k.survivors.length} of ${k.players_before.length} player lineages survived`}`)))),
    h('h3', {}, 'Achievements'),
    achievements.players.length === 0 ? h('p', { class: 'muted small' }, 'No player lineages yet.')
      : h('table', { class: 'board', id: 'achievement-table' },
        h('thead', {}, h('tr', {}, h('th', {}, 'Player'), h('th', { title: 'Speciation events credited' }, 'Species'),
          h('th', { title: 'Longest-lived lineage' }, 'Longevity'), h('th', { title: 'Niches led at an era end' }, 'Niches'),
          h('th', { title: 'Catastrophes survived' }, 'Survived'))),
        h('tbody', {}, ...achievements.players.map((p) => h('tr', { class: p.player === me ? 'me' : '' },
          h('td', {}, swatch(p.player, me), p.name), h('td', {}, String(p.species_founded)), h('td', {}, `${p.longevity_days} d`),
          h('td', {}, String(p.niche_dominance)), h('td', {}, String(p.catastrophes_survived)))))),
    achievements.recent.length > 0 ? h('ul', { class: 'feed-list' }, ...achievements.recent.slice(0, 12).map((a) => h('li', { class: 'feed-item' },
      h('span', { class: 'feed-kind kind-achievement' }, a.name), h('span', {}, a.text), h('span', { class: 'muted small' }, ` · day ${a.day + 1}`)))) : null,
  ];
  el.replaceChildren(...parts.filter((p): p is Node => p !== null));
}

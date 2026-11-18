// Founder design editor ("Organism design" in docs/game-design.md):
// thermoregulation, birth type, offspring per birth, founders and trait
// sliders, with the point cost against the budget updated live.
//
// Two modes (docs/morphology.md, section 6): Simple (sliders and a passive
// avatar; the default) and Builder (the drag-to-shape body builder above the
// sliders). Both edit one design: its traits plus its body plan
// (`EditorDesign.morphology`); `designMorph.ts` keeps the five shared genes
// (eye, ear, leg, tail and fang size) in step between the two.

import { api, ApiError } from '../api';
import { designCost, validateDesign } from '../cost';
import { seasonNotice } from '../seasonNotice';
import { designWarnings, FOUNDING_RAMP_TEXT } from '../designWarnings';
import { applyMorphology, applyTrait, designPayload, morphPrices, newEditorDesign, starterValue } from '../designMorph';
import { defaultMorphology, sameMorphology, type Morphology } from '../morphology';
import type { BirthSpec, DesignCompatView, FoundResponse, GeneCatalog, GeneInfo, PlayerId, SpeciesList, ThermoSpec, WorldSummary } from '../protocol';
import { hybridAdvice } from '../hybrid';
import { foundingHint } from '../foodHint';
import { adultMass, flight, geneValue } from '../bodyMath';
import { avatarView } from './avatarView';
import { createBodyBuilder, type BodyBuilder, type BuilderChange } from './builder/builder';
import type { BuilderPreset } from './builder/presets';
import { h } from './dom';
import { readMode, writeMode, type EditorMode } from './editorMode';
import { caption, kg, TRAIT_TEXT, type Level } from './traitText';
import { traitEnds, traitName } from './traitNames';
import { visionReadoutView } from './visionReadout';

export interface DesignCallbacks {
  token: string;
  /** The player (to tell their own species from others' in the interbreeding check). */
  player?: PlayerId;
  onFounded: (r: FoundResponse | null) => void;
  onUnauthorized: () => void;
  onLogout: () => void;
  /**
   * Founding again after the last lineage died out: the screen says so and
   * offers a way back to the map (`onCancel`).
   */
  refound?: boolean;
  /** The last lineage was retired by the player, not killed off (the screen words it so). */
  retired?: boolean;
  onCancel?: () => void;
}

const BIRTHS: [BirthSpec, string, string][] = [
  ['live', 'Live birth', 'Few, well-developed young; mothers carry them.'],
  ['incubated_eggs', 'Incubated eggs', 'Eggs kept warm and guarded.'],
  ['abandoned_eggs', 'Abandoned eggs', 'Many eggs, no parental care.'],
];

export async function designScreen(cb: DesignCallbacks): Promise<HTMLElement> {
  let cat: GeneCatalog;
  // The season (for the winter notice) is a nicety: the editor opens without it.
  const world = api.world().catch(() => null);
  // The living species, for the interbreeding check (optional too).
  const species = api.species().catch(() => null);
  try {
    cat = await api.genes();
  } catch (e) {
    return h('div', { class: 'screen center' }, h('div', { class: 'card' }, h('p', { class: 'error' }, `Could not load the gene list: ${(e as Error).message}`)));
  }
  return buildEditor(cat, cb, await world, await species);
}

/** How often the editor re-reads the world (season notice, founding hint, species): a game day is seconds. */
export const DESIGN_WORLD_REFRESH_MS = 10000;

export function buildEditor(cat: GeneCatalog, cb: DesignCallbacks, world: WorldSummary | null = null, species: SpeciesList | null = null): HTMLElement {
  // The one design both modes edit: traits plus the whole body plan.
  const spec = newEditorDesign(cat);
  // The season notice: kept current while the player designs (it said "winter in 2 days" during winter).
  const seasonEl = h('p', { class: 'notice season-notice', id: 'design-season', role: 'note' });
  const showSeason = () => {
    const t = seasonNotice(world, spec.thermo);
    seasonEl.hidden = !t;
    if (t && seasonEl.textContent !== t) seasonEl.textContent = t;
  };
  showSeason();

  // --- basics
  const name = h('input', { id: 'design-name', maxlength: 40, placeholder: 'e.g. Marsh striders', value: '' });
  name.addEventListener('input', () => { spec.name = name.value; update(); });

  const thermoRadios = (['warm', 'cold'] as ThermoSpec[]).map((t) => {
    const r = h('input', { type: 'radio', name: 'thermo', value: t, checked: t === spec.thermo });
    r.addEventListener('change', () => { spec.thermo = t; showSeason(); update(); });
    return h('label', { class: 'choice' }, r, t === 'warm' ? 'Warm-blooded' : 'Cold-blooded',
      h('small', {}, t === 'warm' ? 'Active in the cold, eats more.' : 'Cheap to run, needs warmth to move.'));
  });

  const birthRadios = BIRTHS.map(([b, title, hint]) => {
    const r = h('input', { type: 'radio', name: 'birth', value: b, checked: b === spec.birth });
    r.addEventListener('change', () => { spec.birth = b; update(); });
    return h('label', { class: 'choice' }, r, title, h('small', {}, hint));
  });

  const litter = h('input', { id: 'design-litter', type: 'number', min: 1, max: cat.max_litter, step: 1, value: spec.litter_size });
  litter.addEventListener('input', () => { spec.litter_size = Number(litter.value); update(); });
  const founders = h('input', { id: 'design-founders', type: 'number', min: 2, max: 400, step: 1, value: spec.founders });
  founders.addEventListener('input', () => { spec.founders = Number(founders.value); update(); });

  // --- traits
  const costCells = new Map<string, HTMLElement>();
  const sliders = new Map<string, HTMLInputElement>();
  const values = new Map<string, HTMLElement>();
  /** Move a slider to the design's value (the builder changed it). */
  const showTrait = (gene: string) => {
    const v = spec.traits[gene];
    const s = sliders.get(gene);
    if (s) s.value = String(v);
    const val = values.get(gene);
    if (val) val.textContent = v.toFixed(2);
  };
  // Live captions: what the current value means in play, given the whole
  // design (`traitText.ts`); `update` refreshes them on every change.
  const captions = new Map<string, HTMLElement>();
  const levelRows = new Map<string, Map<Level, HTMLElement>>();
  const traitRow = (g: GeneInfo): HTMLElement => {
    const text = TRAIT_TEXT[g.name];
    const title = traitName(g.name);
    const ends = traitEnds(g.name);
    const id = `trait-${g.name}`;
    // The design's own value (the starting design raises some traits above their defaults).
    const start = spec.traits[g.name] ?? g.default;
    const s = h('input', {
      type: 'range', class: 'trait-slider', min: 0, max: 1, step: 0.01, value: start, 'data-gene': g.name,
      'aria-label': ends ? `${title} (0 ${ends[0]}, 1 ${ends[1]})` : title,
      'aria-describedby': text ? `${id}-caption` : undefined,
    });
    const val = h('span', { class: 'num trait-val' }, start.toFixed(2));
    const tip = g.cost > 0 ? `${g.cost} points per unit above ${g.default}` : 'free: a trade-off, not an upgrade';
    const cost = h('span', { class: 'num cost trait-cost', title: tip }, '');
    s.addEventListener('input', () => {
      applyTrait(spec, g.name, Number(s.value));
      val.textContent = Number(s.value).toFixed(2);
      update();
    });
    // A finished slider move: the builder (if open) follows, as one undo step.
    s.addEventListener('change', () => syncBuilder());
    sliders.set(g.name, s);
    values.set(g.name, val);
    costCells.set(g.name, cost);
    const name = h('span', { class: 'trait-name', title: text ? `${text.what} (${tip})` : tip }, title);
    // A two-ended slider shows what each end means under it ("small … huge").
    const endsRow = ends && h('div', { class: 'trait-ends', 'aria-hidden': 'true' }, h('span', {}, ends[0]), h('span', {}, ends[1]));
    if (!text) return h('div', { class: 'trait', 'data-trait': g.name }, h('span', { class: 'trait-label' }, name), s, val, cost, endsRow);
    // The full low / middle / high text behind an info toggle: a hover
    // tooltip can't be opened on a phone.
    const levels = new Map<Level, HTMLElement>();
    const levelRow = (l: Level, title: string) => {
      const row = h('li', { 'data-level': l }, h('strong', {}, title), ' ', text[l]);
      levels.set(l, row);
      return row;
    };
    levelRows.set(g.name, levels);
    const info = h('div', { class: 'trait-info', id: `${id}-info`, hidden: true },
      h('p', {}, text.what),
      h('ul', {}, levelRow('low', 'Low:'), levelRow('mid', 'Middle:'), levelRow('high', 'High:')));
    const more = h('button', {
      type: 'button', class: 'trait-more', 'aria-expanded': 'false', 'aria-controls': info.id,
      'aria-label': `About ${title}`, title: text.what,
    }, 'i');
    more.addEventListener('click', () => {
      const open = info.hidden;
      info.hidden = !open;
      more.setAttribute('aria-expanded', String(open));
    });
    const cap = h('div', { class: 'trait-caption', id: `${id}-caption` });
    captions.set(g.name, cap);
    return h('div', { class: 'trait', 'data-trait': g.name }, h('span', { class: 'trait-label' }, name, more), s, val, cost, endsRow, cap, info);
  };
  const paid = cat.genes.filter((g) => g.cost > 0);
  const free = cat.genes.filter((g) => g.cost === 0);

  // --- live preview of the founders, with the vision readout under it
  const readout = visionReadoutView();
  const avatar = avatarView(readout.el);

  // --- Simple / Builder
  let mode: EditorMode = readMode();
  let builder: BodyBuilder | null = null;
  const builderHost = h('div', { class: 'design-builder', id: 'design-builder', hidden: true });
  const presetNote = h('div', { class: 'design-preset-confirm', id: 'design-preset-confirm', role: 'status', hidden: true });
  const modeBtn = (m: EditorMode, text: string, title: string) =>
    h('button', { type: 'button', id: `design-mode-${m}`, class: 'secondary', 'data-mode': m, 'aria-pressed': 'false', title }, text);
  const modeBtns = [modeBtn('simple', 'Simple', 'Sliders only'), modeBtn('builder', 'Builder', 'Shape eyes, ears, legs, tail, snout and horns by dragging')];
  const modeSwitch = h('div', { class: 'design-mode', role: 'group', 'aria-label': 'Editor mode' }, ...modeBtns);
  const modeHint = h('p', { class: 'muted small design-mode-hint', id: 'design-mode-hint' }, '');

  /** The total cost if the body plan were `m` (a preset's cost change). */
  const costWith = (m: Morphology, traits: Record<string, number> = {}): number => {
    const copy = { ...spec, traits: { ...spec.traits, ...traits } };
    applyMorphology(copy, m);
    return designCost(designPayload(copy, cat), cat).total;
  };

  /** A preset that changes the cost asks first, inline (no blocking dialog). */
  let pendingPreset: ((ok: boolean) => void) | null = null;
  /** The preset being applied: its genes follow its body plan (`onBuilderChange`). */
  let presetTraits: Record<string, number> | null = null;
  const confirmPreset = (preset: BuilderPreset, next: Morphology): boolean | Promise<boolean> => {
    pendingPreset?.(false);
    presetTraits = preset.traits ?? null;
    const now = designCost(payload(), cat).total;
    const then = costWith(next, preset.traits);
    const delta = then - now;
    if (Math.abs(delta) < 0.05) return true;
    return new Promise<boolean>((resolve) => {
      const done = (ok: boolean) => {
        pendingPreset = null;
        presetNote.hidden = true;
        presetNote.replaceChildren();
        resolve(ok);
      };
      pendingPreset = done;
      const sign = delta > 0 ? '+' : '−';
      const apply = h('button', { type: 'button', id: 'design-preset-apply' }, 'Apply');
      const cancel = h('button', { type: 'button', class: 'secondary', id: 'design-preset-cancel' }, 'Cancel');
      apply.addEventListener('click', () => done(true));
      cancel.addEventListener('click', () => done(false));
      const over = then > cat.budget + 1e-3 ? ', over budget' : '';
      presetNote.replaceChildren(
        h('span', {}, `${preset.label}: ${now.toFixed(1)} → ${then.toFixed(1)} points (${sign}${Math.abs(delta).toFixed(1)})${over}.`),
        apply, cancel);
      presetNote.hidden = false;
    });
  };

  const onBuilderChange = (value: Morphology, change: BuilderChange) => {
    if (change.source === 'preset' && presetTraits) {
      // The preset's genes (diet), on their sliders as a player would set them.
      for (const [g, v] of Object.entries(presetTraits)) {
        const s = sliders.get(g);
        if (!s) continue;
        s.value = String(v);
        s.dispatchEvent(new Event('input'));
      }
      presetTraits = null;
    }
    for (const gene of applyMorphology(spec, value)) showTrait(gene);
    // Costs, captions and validation wait for the end of a drag.
    update(change.final);
  };

  /** Bring the builder to the design's body plan (after slider moves). */
  function syncBuilder(): void {
    if (builder && mode === 'builder' && !sameMorphology(builder.value, spec.morphology)) builder.set(spec.morphology);
  }

  function setMode(m: EditorMode, remember = true): void {
    mode = m;
    if (remember) writeMode(m);
    for (const b of modeBtns) b.setAttribute('aria-pressed', String(b.dataset.mode === m));
    if (m === 'builder') {
      if (!builder) {
        builder = createBodyBuilder({ value: spec.morphology, onChange: onBuilderChange, confirmPreset, idPrefix: 'design-bb', prices: morphPrices(cat), design: spec });
        builderHost.appendChild(builder.el);
      } else syncBuilder();
      builderHost.hidden = false;
      modeHint.textContent = 'Drag the eyes, ears, legs, tail, snout and horns; pick a coat pattern and wing shape. Eye, ear, leg, tail and fang sizes are also sliders below; both stay in step.';
    } else {
      builderHost.hidden = true;
      pendingPreset?.(false);
      modeHint.textContent = 'Builder mode shapes the body by dragging: eye count and placement, ears, legs, tail, snout, horns, coat pattern and wing shape.';
    }
    page.classList.toggle('builder-mode', m === 'builder');
  }
  for (const b of modeBtns) b.addEventListener('click', () => setMode(b.dataset.mode as EditorMode));

  // --- budget panel
  const total = h('div', { class: 'budget-total', id: 'budget-total' });
  const bar = h('div', { class: 'bar-fill' });
  const breakdown = h('div', { class: 'muted small' });
  const problem = h('p', { class: 'error', id: 'design-problem', role: 'alert' });
  // Before a name is typed the button is off: say why, and take the player to the field.
  const nameLink = h('button', { type: 'button', class: 'link-btn', id: 'design-need-name-go' }, 'Name your lineage');
  nameLink.addEventListener('click', () => {
    name.scrollIntoView({ block: 'center', behavior: 'smooth' });
    name.focus({ preventScroll: true });
  });
  const unspent = h('p', { class: 'muted small', id: 'design-unspent' });
  // Who this design could interbreed with (wild and rival species), and the market it implies.
  const hybridBox = h('section', { class: 'card hybrid-card', id: 'design-hybrid', hidden: !species, 'aria-live': 'polite' });
  // Steep banks: the server's check of how much of today's shore these founders can drink from.
  const waterBox = h('section', { class: 'card hybrid-card hybrid-warn', id: 'design-water', hidden: true, 'aria-live': 'polite' });
  // Founding size against the land's food (playtest 1, finding 4).
  const foodBox = h('p', { class: 'small food-hint', id: 'design-food' });
  // The world the founders land in (drought, catastrophe) and wings a heavy body can't use.
  const warnBox = h('div', { id: 'design-warnings', class: 'design-warnings', role: 'status' });
  const rampNote = h('p', { class: 'muted small', id: 'design-ramp' }, FOUNDING_RAMP_TEXT);
  const needName = h('p', { class: 'muted small', id: 'design-need-name' }, nameLink, ' (Basics, at the top) to found it.');
  const serverError = h('p', { class: 'error', id: 'design-server-error', role: 'alert' });
  const submit = h('button', { id: 'design-submit', type: 'button' }, 'Found lineage');
  // Phones: the budget card sits below ~45 sliders, so a bar along the bottom
  // keeps the total in sight and jumps to it (and the Found button).
  const dockTotal = h('span', { class: 'dock-total', id: 'design-dock-total' });
  const budgetCard = h('section', { class: 'card budget', id: 'design-budget' });
  const dock = h('div', { class: 'design-dock narrow-only', id: 'design-dock' }, dockTotal,
    h('button', { type: 'button', id: 'design-dock-go', onclick: () => budgetCard.scrollIntoView({ behavior: 'smooth', block: 'start' }) }, 'Review & found ↓'));
  const reset = h('button', { type: 'button', class: 'secondary' }, 'Reset traits');

  reset.addEventListener('click', () => {
    // Traits and body plan back to the starting design (the builder follows as one undo step).
    spec.morphology = defaultMorphology();
    for (const g of cat.genes) {
      const v = starterValue(cat, g.name);
      spec.traits[g.name] = v;
      const s = sliders.get(g.name)!;
      s.value = String(v);
      s.dispatchEvent(new Event('input'));
    }
    syncBuilder();
  });

  function payload() {
    return designPayload(spec, cat);
  }

  let hybridKey = '';
  /** The server's sampled check of the current design (individuals, not species means), when it answered. */
  let sampled: DesignCompatView | null = null;
  /** The server's sampled fertility in words: the lineages any founder could breed with, most fertile first. */
  function sampledText(v: DesignCompatView | null): string {
    if (!v) return '';
    const pct = (x: number) => `${Math.round(x * 100)}%`;
    const fertile = v.rows.filter((r) => r.fertile_share !== undefined ? r.fertile_share > 0 : r.fertility > 0).slice(0, 4);
    if (!fertile.length) return `Sampled now (${v.sample} founders against each lineage's adults): sterile with every living lineage.`;
    return `Sampled now (${v.sample} founders against each lineage's adults; the market uses the same check): ` + fertile
      .map((r) => `${r.lineage_name}${r.wild ? ' (wild)' : ` (${r.owner_name})`} ${pct(r.fertility)} fertile on average`
        + (r.fertility_low !== undefined && r.fertility_high !== undefined ? ` (${pct(r.fertility_low)}–${pct(r.fertility_high)})` : ''))
      .join(', ') + '.';
  }

  function renderHybrid(): void {
    if (!species) return;
    const p = payload();
    const a = hybridAdvice(p, species.species, cb.player ?? -1);
    const server = sampledText(sampled);
    const key = JSON.stringify(a) + server;
    if (key === hybridKey) return;
    hybridKey = key;
    hybridBox.className = `card hybrid-card hybrid-${a.level}`;
    hybridBox.replaceChildren(
      h('h2', {}, 'Interbreeding'),
      h('p', { class: 'hybrid-headline' }, a.level === 'warn' ? '⚠ ' : '', a.headline),
      h('ul', { class: 'small hybrid-points' }, ...a.points.map((t) => h('li', {}, t))),
      h('p', { class: 'small muted hybrid-market', id: 'design-hybrid-market' }, a.market),
      h('p', { class: 'small muted hybrid-confidence', id: 'design-hybrid-confidence' }, a.confidence),
      ...(server ? [h('p', { class: 'small hybrid-sampled', id: 'design-hybrid-sampled' }, server)] : []));
  }

  let waterTimer: ReturnType<typeof setTimeout> | undefined;
  let waterSeq = 0;
  /** Ask the server (a moment after the last change) whether the founders can reach most water. */
  function checkWater(): void {
    clearTimeout(waterTimer);
    waterTimer = setTimeout(async () => {
      const seq = ++waterSeq;
      // A name is required to validate: check the design before the player has typed one.
      const body = payload();
      if (!body.name.trim()) body.name = 'Design';
      const r = await api.designCompatibility(cb.token, body).catch(() => null);
      if (seq !== waterSeq) return;
      sampled = r;
      renderHybrid();
      const warning = r?.water_warning;
      waterBox.hidden = !warning;
      if (warning) waterBox.replaceChildren(h('h2', {}, 'Drinking water'), h('p', { class: 'hybrid-headline' }, '⚠ ', warning));
    }, 600);
  }

  /** The body's points in words: cold blood and body size (smaller than the default or past the free size). */
  function bodyParts(body: number): string[] {
    if (!(body > 0)) return [];
    const cold = spec.thermo === 'cold' ? cat.cost_cold_blooded ?? 0 : 0;
    const size = body - cold;
    return [cold > 0 ? `cold blood ${cold.toFixed(1)}` : '', size > 0.05 ? `body size ${size.toFixed(1)}` : ''].filter(Boolean);
  }

  /** Redraw from the design; `full` (default) also recomputes costs, captions and validation. */
  function update(full = true): void {
    avatar.update(spec, spec.morphology);
    readout.update(spec.morphology);
    if (!full) return;
    // The builder wears the design's covering and colours (sliders change them).
    builder?.setDesign(spec);
    const c = designCost(payload(), cat);
    total.textContent = `${c.total.toFixed(1)} / ${cat.budget} points`;
    dockTotal.textContent = `${c.total.toFixed(1)} / ${cat.budget} points · founding with points left is fine`;
    const frac = c.total / cat.budget;
    bar.style.width = `${Math.min(100, frac * 100)}%`;
    bar.classList.toggle('over', frac > 1 + 1e-5);
    const extras = [
      c.morph > 0 ? `body plan ${c.morph.toFixed(1)}` : '',
      ...bodyParts(c.body),
    ].filter(Boolean);
    breakdown.textContent = [`traits ${c.traits.toFixed(1)}`, `offspring ${c.litter.toFixed(1)}`, `extra founders ${c.founders.toFixed(1)}`, ...extras].join(' · ');
    // Most of the budget unspent (the starting design): say so before the player founds a bare-minimum animal.
    const left = cat.budget - c.total;
    unspent.textContent = left > cat.budget / 2
      ? `${left.toFixed(1)} points still unspent. The starting design is a sturdy herbivore ready for its first winter: raise the traits that suit the world, or try a preset in Builder mode.`
      : left > 0.05 ? `${left.toFixed(1)} points left to spend.` : left >= -1e-3 ? 'Every point spent.' : '';
    // Wings a heavy body can't use cost points and do nothing: say so.
    const wings = geneValue(spec, 'wings');
    if (wings > 0.25 && !flight(spec).glide) {
      unspent.textContent += ` Wings do nothing on a ${kg(adultMass(spec))} body: too heavy to fly or glide, so their points are wasted.`;
    } else if (wings > 0.5 && !flight(spec).fly) {
      unspent.textContent += ` At ${kg(adultMass(spec))} the body is too heavy to fly: its wings only glide.`;
    }
    renderHybrid();
    checkWater();
    const food = foundingHint(payload(), world);
    foodBox.textContent = food.text;
    foodBox.className = `small food-hint food-${food.level}`;
    const warns = designWarnings(payload(), world);
    const wkey = JSON.stringify(warns);
    if (warnBox.dataset.key !== wkey) {
      warnBox.dataset.key = wkey;
      warnBox.replaceChildren(...warns.map((w) => h('p', { class: `small notice design-warning warn-${w.level}`, 'data-warning': w.id }, w.text)));
    }
    for (const [g, cell] of costCells) {
      const p = c.perGene[g];
      cell.textContent = p ? p.toFixed(1) : '';
    }
    for (const [g, cap] of captions) {
      const c = caption(g, spec)!;
      cap.textContent = c.text;
      cap.dataset.level = c.level;
      for (const [l, row] of levelRows.get(g)!) row.classList.toggle('current', l === c.level);
    }
    const p = validateDesign(payload(), cat);
    problem.textContent = p && !(p.code === 'bad_name' && spec.name === '') ? p.message : '';
    needName.hidden = spec.name.trim() !== '';
    submit.disabled = p !== null;
    serverError.textContent = '';
  }

  submit.addEventListener('click', async () => {
    submit.disabled = true;
    serverError.textContent = '';
    try {
      const r = await api.found(cb.token, payload());
      cb.onFounded(r);
    } catch (e) {
      if (e instanceof ApiError) {
        if (e.status === 401) return cb.onUnauthorized();
        // A first founding that raced another tab: the lineage exists, go play. Founding again: say why not.
        if (e.code === 'already_founded' && !cb.refound) return cb.onFounded(null);
        const extra = e.code === 'over_budget' && e.body ? ` (server: ${Number(e.body.cost).toFixed(1)} of ${e.body.budget})` : '';
        serverError.textContent = `${e.code}: ${e.message}${extra}`;
      } else {
        serverError.textContent = String(e);
      }
      submit.disabled = false;
    }
  });

  const page = h('div', { class: 'screen design' },
    h('header', { class: 'topbar' }, h('strong', { class: 'topbar-title' }, cb.refound ? 'Design a new lineage' : 'Design your founders'),
      h('span', { class: 'muted' }, cb.refound
        ? cb.retired
          ? 'You retired your last lineage. Your renown, records and titles stay; the new founders start fresh.'
          : 'Your last lineage died out. Your renown, records and titles stay; the new founders start fresh.'
        : 'After launch the design changes only through breeding, mutation and selection.'),
      h('span', { class: 'spacer' }),
      cb.onCancel ? h('button', { class: 'secondary', id: 'design-cancel', onclick: cb.onCancel }, 'Back to the map') : null,
      h('button', { class: 'secondary', onclick: cb.onLogout }, 'Log out')),
    h('div', { class: 'design-body' },
      h('div', { class: 'design-main' },
        seasonEl,
        h('section', { class: 'card' }, h('h2', {}, 'Basics'),
          h('label', {}, 'Lineage name', name),
          h('fieldset', {}, h('legend', {}, 'Thermoregulation'), ...thermoRadios),
          h('fieldset', {}, h('legend', {}, 'Birth type'), ...birthRadios),
          h('div', { class: 'row' },
            h('label', {}, `Offspring per birth (1–${cat.max_litter}, ${cat.cost_per_extra_offspring} pts each above 1)`, litter),
            h('label', {}, cat.flat_founders !== undefined && cat.founder_cost_step
              ? `Founders (${cat.base_founders} included, ${cat.cost_per_extra_founder} pts each up to ${cat.flat_founders}, then each ${cat.founder_cost_step} pts dearer than the last)`
              : `Founders (${cat.base_founders} included, ${cat.cost_per_extra_founder} pts each extra)`, founders))),
        h('section', { class: 'card design-traits' },
          h('div', { class: 'design-traits-head' }, h('h2', {}, 'Traits'), modeSwitch),
          modeHint, presetNote, builderHost,
          h('p', { class: 'muted small' }, 'Points are spent on raising a trait above its default. The line under each slider says what its setting does; the i button explains low, middle and high.'),
          h('div', { class: 'traits' }, ...paid.map(traitRow))),
        h('section', { class: 'card' }, h('h2', {}, 'Behavior, diet and mate preferences'), h('p', { class: 'muted small' }, 'Free: these are trade-offs.'),
          h('div', { class: 'traits' }, ...free.map(traitRow)))),
      h('aside', { class: 'design-side' }, avatar.el,
        (budgetCard.append(h('h2', {}, 'Budget'), total, h('div', { class: 'bar' }, bar), breakdown, unspent, foodBox, warnBox, rampNote, problem, needName, serverError,
          h('div', { class: 'row' }, submit, reset)), budgetCard),
        hybridBox,
        waterBox),
      dock));
  setMode(mode, false);
  update();
  // Re-read the world and the species while the editor is open; stop once it is gone.
  let attached = false;
  const refresh = setInterval(async () => {
    if (!page.isConnected) {
      if (attached) clearInterval(refresh);
      return;
    }
    attached = true;
    const [w, sp] = await Promise.all([api.world().catch(() => null), species ? api.species().catch(() => null) : Promise.resolve(null)]);
    if (w) world = w;
    if (sp) species = sp;
    showSeason();
    update();
  }, DESIGN_WORLD_REFRESH_MS);
  return page;
}

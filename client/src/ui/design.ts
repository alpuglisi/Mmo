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
import { applyMorphology, applyTrait, designPayload, morphPrices, newEditorDesign } from '../designMorph';
import { defaultMorphology, sameMorphology, type Morphology } from '../morphology';
import type { BirthSpec, FoundResponse, GeneCatalog, GeneInfo, ThermoSpec } from '../protocol';
import { avatarView } from './avatarView';
import { createBodyBuilder, type BodyBuilder, type BuilderChange } from './builder/builder';
import type { BuilderPreset } from './builder/presets';
import { h } from './dom';
import { readMode, writeMode, type EditorMode } from './editorMode';
import { caption, TRAIT_TEXT, type Level } from './traitText';
import { traitEnds, traitName } from './traitNames';
import { visionReadoutView } from './visionReadout';

export interface DesignCallbacks {
  token: string;
  onFounded: (r: FoundResponse | null) => void;
  onUnauthorized: () => void;
  onLogout: () => void;
}

const BIRTHS: [BirthSpec, string, string][] = [
  ['live', 'Live birth', 'Few, well-developed young; mothers carry them.'],
  ['incubated_eggs', 'Incubated eggs', 'Eggs kept warm and guarded.'],
  ['abandoned_eggs', 'Abandoned eggs', 'Many eggs, no parental care.'],
];

export async function designScreen(cb: DesignCallbacks): Promise<HTMLElement> {
  let cat: GeneCatalog;
  try {
    cat = await api.genes();
  } catch (e) {
    return h('div', { class: 'screen center' }, h('div', { class: 'card' }, h('p', { class: 'error' }, `Could not load the gene list: ${(e as Error).message}`)));
  }
  return buildEditor(cat, cb);
}

export function buildEditor(cat: GeneCatalog, cb: DesignCallbacks): HTMLElement {
  // The one design both modes edit: traits plus the whole body plan.
  const spec = newEditorDesign(cat);

  // --- basics
  const name = h('input', { id: 'design-name', maxlength: 40, placeholder: 'e.g. Marsh striders', value: '' });
  name.addEventListener('input', () => { spec.name = name.value; update(); });

  const thermoRadios = (['warm', 'cold'] as ThermoSpec[]).map((t) => {
    const r = h('input', { type: 'radio', name: 'thermo', value: t, checked: t === spec.thermo });
    r.addEventListener('change', () => { spec.thermo = t; update(); });
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
    const s = h('input', {
      type: 'range', class: 'trait-slider', min: 0, max: 1, step: 0.01, value: g.default, 'data-gene': g.name,
      'aria-label': ends ? `${title} (0 ${ends[0]}, 1 ${ends[1]})` : title,
      'aria-describedby': text ? `${id}-caption` : undefined,
    });
    const val = h('span', { class: 'num trait-val' }, g.default.toFixed(2));
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
  const modeBtns = [modeBtn('simple', 'Simple', 'Sliders only'), modeBtn('builder', 'Builder', 'Shape eyes, ears, legs, tail and snout by dragging')];
  const modeSwitch = h('div', { class: 'design-mode', role: 'group', 'aria-label': 'Editor mode' }, ...modeBtns);
  const modeHint = h('p', { class: 'muted small design-mode-hint', id: 'design-mode-hint' }, '');

  /** The total cost if the body plan were `m` (a preset's cost change). */
  const costWith = (m: Morphology): number => {
    const copy = { ...spec, traits: { ...spec.traits } };
    applyMorphology(copy, m);
    return designCost(designPayload(copy, cat), cat).total;
  };

  /** A preset that changes the cost asks first, inline (no blocking dialog). */
  let pendingPreset: ((ok: boolean) => void) | null = null;
  const confirmPreset = (preset: BuilderPreset, next: Morphology): boolean | Promise<boolean> => {
    pendingPreset?.(false);
    const now = designCost(payload(), cat).total;
    const then = costWith(next);
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
        builder = createBodyBuilder({ value: spec.morphology, onChange: onBuilderChange, confirmPreset, idPrefix: 'design-bb', prices: morphPrices(cat) });
        builderHost.appendChild(builder.el);
      } else syncBuilder();
      builderHost.hidden = false;
      modeHint.textContent = 'Drag the eyes, ears, legs, tail and snout. Eye, ear, leg, tail and fang sizes are also sliders below; both stay in step.';
    } else {
      builderHost.hidden = true;
      pendingPreset?.(false);
      modeHint.textContent = 'Builder mode shapes the body by dragging: eye count and placement, ears, legs, tail and snout.';
    }
    page.classList.toggle('builder-mode', m === 'builder');
  }
  for (const b of modeBtns) b.addEventListener('click', () => setMode(b.dataset.mode as EditorMode));

  // --- budget panel
  const total = h('div', { class: 'budget-total', id: 'budget-total' });
  const bar = h('div', { class: 'bar-fill' });
  const breakdown = h('div', { class: 'muted small' });
  const problem = h('p', { class: 'error', id: 'design-problem', role: 'alert' });
  const serverError = h('p', { class: 'error', id: 'design-server-error', role: 'alert' });
  const submit = h('button', { id: 'design-submit', type: 'button' }, 'Found lineage');
  const reset = h('button', { type: 'button', class: 'secondary' }, 'Reset traits');

  reset.addEventListener('click', () => {
    // Traits and body plan back to the defaults (the builder follows as one undo step).
    spec.morphology = defaultMorphology();
    for (const g of cat.genes) {
      spec.traits[g.name] = g.default;
      const s = sliders.get(g.name)!;
      s.value = String(g.default);
      s.dispatchEvent(new Event('input'));
    }
    syncBuilder();
  });

  function payload() {
    return designPayload(spec, cat);
  }

  /** Redraw from the design; `full` (default) also recomputes costs, captions and validation. */
  function update(full = true): void {
    avatar.update(spec, spec.morphology);
    readout.update(spec.morphology);
    if (!full) return;
    const c = designCost(payload(), cat);
    total.textContent = `${c.total.toFixed(1)} / ${cat.budget} points`;
    const frac = c.total / cat.budget;
    bar.style.width = `${Math.min(100, frac * 100)}%`;
    bar.classList.toggle('over', frac > 1 + 1e-5);
    const extras = [
      c.morph > 0 ? `body plan ${c.morph.toFixed(1)}` : '',
      c.body > 0 ? `cold blood / small body ${c.body.toFixed(1)}` : '',
    ].filter(Boolean);
    breakdown.textContent = [`traits ${c.traits.toFixed(1)}`, `offspring ${c.litter.toFixed(1)}`, `extra founders ${c.founders.toFixed(1)}`, ...extras].join(' · ');
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
        if (e.code === 'already_founded') return cb.onFounded(null);
        const extra = e.code === 'over_budget' && e.body ? ` (server: ${Number(e.body.cost).toFixed(1)} of ${e.body.budget})` : '';
        serverError.textContent = `${e.code}: ${e.message}${extra}`;
      } else {
        serverError.textContent = String(e);
      }
      submit.disabled = false;
    }
  });

  const page = h('div', { class: 'screen design' },
    h('header', { class: 'topbar' }, h('strong', {}, 'Design your founders'),
      h('span', { class: 'muted' }, 'After launch the design changes only through breeding, mutation and selection.'),
      h('span', { class: 'spacer' }), h('button', { class: 'secondary', onclick: cb.onLogout }, 'Log out')),
    h('div', { class: 'design-body' },
      h('div', { class: 'design-main' },
        h('section', { class: 'card' }, h('h2', {}, 'Basics'),
          h('label', {}, 'Lineage name', name),
          h('fieldset', {}, h('legend', {}, 'Thermoregulation'), ...thermoRadios),
          h('fieldset', {}, h('legend', {}, 'Birth type'), ...birthRadios),
          h('div', { class: 'row' },
            h('label', {}, `Offspring per birth (1–${cat.max_litter}, ${cat.cost_per_extra_offspring} pts each above 1)`, litter),
            h('label', {}, `Founders (${cat.base_founders} included, ${cat.cost_per_extra_founder} pts each extra)`, founders))),
        h('section', { class: 'card design-traits' },
          h('div', { class: 'design-traits-head' }, h('h2', {}, 'Traits'), modeSwitch),
          modeHint, presetNote, builderHost,
          h('p', { class: 'muted small' }, 'Points are spent on raising a trait above its default. The line under each slider says what its setting does; the i button explains low, middle and high.'),
          h('div', { class: 'traits' }, ...paid.map(traitRow))),
        h('section', { class: 'card' }, h('h2', {}, 'Behavior, diet and mate preferences'), h('p', { class: 'muted small' }, 'Free: these are trade-offs.'),
          h('div', { class: 'traits' }, ...free.map(traitRow)))),
      h('aside', { class: 'design-side' }, avatar.el,
        h('section', { class: 'card budget' }, h('h2', {}, 'Budget'), total, h('div', { class: 'bar' }, bar), breakdown, problem, serverError,
          h('div', { class: 'row' }, submit, reset)))));
  setMode(mode, false);
  update();
  return page;
}

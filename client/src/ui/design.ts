// Founder design editor ("Organism design" in docs/game-design.md):
// thermoregulation, birth type, offspring per birth, founders and trait
// sliders, with the point cost against the budget updated live.

import { api, ApiError } from '../api';
import { changedTraits, designCost, validateDesign } from '../cost';
import type { BirthSpec, DesignSpec, FoundResponse, GeneCatalog, GeneInfo, ThermoSpec } from '../protocol';
import { h, label } from './dom';

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
  const spec: DesignSpec = {
    name: '',
    thermo: 'warm',
    birth: 'live',
    litter_size: 2,
    traits: Object.fromEntries(cat.genes.map((g) => [g.name, g.default])),
    founders: cat.base_founders,
  };

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
  const traitRow = (g: GeneInfo): HTMLElement => {
    const s = h('input', { type: 'range', min: 0, max: 1, step: 0.01, value: g.default, 'data-gene': g.name, 'aria-label': label(g.name) });
    const val = h('span', { class: 'num' }, g.default.toFixed(2));
    const cost = h('span', { class: 'num cost' }, '');
    s.addEventListener('input', () => {
      spec.traits[g.name] = Number(s.value);
      val.textContent = Number(s.value).toFixed(2);
      update();
    });
    sliders.set(g.name, s);
    costCells.set(g.name, cost);
    const tip = g.cost > 0 ? `${g.cost} points per unit above ${g.default}` : 'free: a trade-off, not an upgrade';
    return h('div', { class: 'trait', title: tip }, h('span', { class: 'trait-name' }, label(g.name)), s, val, cost);
  };
  const paid = cat.genes.filter((g) => g.cost > 0);
  const free = cat.genes.filter((g) => g.cost === 0);

  // --- budget panel
  const total = h('div', { class: 'budget-total', id: 'budget-total' });
  const bar = h('div', { class: 'bar-fill' });
  const breakdown = h('div', { class: 'muted small' });
  const problem = h('p', { class: 'error', id: 'design-problem', role: 'alert' });
  const serverError = h('p', { class: 'error', id: 'design-server-error', role: 'alert' });
  const submit = h('button', { id: 'design-submit', type: 'button' }, 'Found lineage');
  const reset = h('button', { type: 'button', class: 'secondary' }, 'Reset traits');

  reset.addEventListener('click', () => {
    for (const g of cat.genes) {
      spec.traits[g.name] = g.default;
      const s = sliders.get(g.name)!;
      s.value = String(g.default);
      s.dispatchEvent(new Event('input'));
    }
  });

  function payload(): DesignSpec {
    return { ...spec, name: spec.name.trim(), traits: changedTraits(spec.traits, cat) };
  }

  function update(): void {
    const c = designCost(payload(), cat);
    total.textContent = `${c.total.toFixed(1)} / ${cat.budget} points`;
    const frac = c.total / cat.budget;
    bar.style.width = `${Math.min(100, frac * 100)}%`;
    bar.classList.toggle('over', frac > 1 + 1e-5);
    breakdown.textContent = `traits ${c.traits.toFixed(1)} · offspring ${c.litter.toFixed(1)} · extra founders ${c.founders.toFixed(1)}`;
    for (const [g, cell] of costCells) {
      const p = c.perGene[g];
      cell.textContent = p ? p.toFixed(1) : '';
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
        h('section', { class: 'card' }, h('h2', {}, 'Traits'), h('p', { class: 'muted small' }, 'Points are spent on raising a trait above its default.'),
          h('div', { class: 'traits' }, ...paid.map(traitRow))),
        h('section', { class: 'card' }, h('h2', {}, 'Behavior, diet and mate preferences'), h('p', { class: 'muted small' }, 'Free: these are trade-offs.'),
          h('div', { class: 'traits' }, ...free.map(traitRow)))),
      h('aside', { class: 'card budget' }, h('h2', {}, 'Budget'), total, h('div', { class: 'bar' }, bar), breakdown, problem, serverError,
        h('div', { class: 'row' }, submit, reset))));
  update();
  return page;
}

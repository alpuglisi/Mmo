// The design editor's compact vision readout (docs/morphology.md, section 6,
// "Live readout"): view %, depth ° and blind spots, each against the default
// body with an arrow. Shown under the founder preview in both editor modes.

import type { Morphology } from '../morphology';
import { visionReadout } from '../designMorph';
import { visionOf, visionSummary } from '../vision';
import { h } from './dom';

export interface VisionReadoutView {
  el: HTMLElement;
  update: (m: Readonly<Morphology>) => void;
}

export function visionReadoutView(): VisionReadoutView {
  const cells = new Map<string, { v: HTMLElement; d: HTMLElement; box: HTMLElement }>();
  const list = h('dl', { class: 'vision-stats' });
  for (const key of ['coverage', 'depth', 'blind']) {
    const v = h('dd', { class: 'num', id: `vision-${key}` }, '');
    const d = h('span', { class: 'vision-delta', id: `vision-${key}-delta` }, '');
    const box = h('div', { class: 'vision-stat', 'data-stat': key }, h('dt', {}, ''), v, d);
    cells.set(key, { v, d, box });
    list.appendChild(box);
  }
  const summary = h('p', { class: 'vision-summary muted small', id: 'vision-summary', 'aria-live': 'polite' }, '');
  const el = h('div', { class: 'vision-readout', id: 'vision-readout', role: 'group', 'aria-label': 'Vision against the default body' }, list, summary);
  let last = '';
  return {
    el,
    update(m) {
      const rows = visionReadout(m);
      const key = JSON.stringify(rows);
      if (key === last) return;
      last = key;
      for (const r of rows) {
        const c = cells.get(r.key)!;
        (c.box.firstChild as HTMLElement).textContent = r.label;
        c.v.textContent = r.value;
        c.d.textContent = r.arrow;
        c.box.dataset.trend = r.trend;
        c.box.title = `${r.label}: ${r.value} (${r.arrow === '=' ? 'as the default body' : `${r.arrow} vs the default body`}), ${r.hint}`;
      }
      summary.textContent = visionSummary(visionOf(m));
    },
  };
}

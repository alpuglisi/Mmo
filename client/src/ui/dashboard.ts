// The genetics dashboard (from `GET /api/lineage/stats`), the alerts feed
// and its preferences.

import { setPref, shows, toasts } from '../alertPrefs';
import type { FeedEntry } from '../model';
import { ALERT_KINDS, type AlertKind, type AlertPrefs, type LineageStats, type OrgId } from '../protocol';
import {
  CAUSE_COLORS, CAUSE_LABELS, SERIES, driftRows, geneValueText, geneHistogram, geneSparkline, legacyChart, originBar, originRows, pyramidChart, spreadBar, stackVitals, vitalsChart,
} from './charts';
import { h, pct } from './dom';
import { traitName } from './traitNames';
import { originExplain } from '../hybrid';

/** Genes shown as drift sparklines; the full table lists every gene. */
const TOP_DRIFT = 6;

function legend(items: [string, string][]): HTMLElement {
  return h('div', { class: 'chart-legend' }, ...items.map(([color, text]) => h('span', {}, h('i', { class: 'dot square-fill', style: `background:${color}` }), text)));
}

function card(title: string, sub: string | null, ...body: (Node | null)[]): HTMLElement {
  return h('section', { class: 'chart-card' }, h('h4', {}, title), sub && h('p', { class: 'muted small' }, sub), ...body);
}

/** What the older "cold or heat" bars mean (one count before the sim told them apart, snapshot v52). */
export const EXPOSURE_NOTE = 'Cold or heat (older days): days recorded before cold and heat were counted apart show them together. '
  + 'Newer days show cold, heat and winter sleep each on their own.';

/** `onWhyDying`: opens Help on "Why are my animals dying?" (linked from the deaths chart). */
export function renderDashboard(el: HTMLElement, s: LineageStats | null, onWhyDying?: () => void): void {
  if (!s) {
    el.replaceChildren(h('p', { class: 'muted' }, 'Loading lineage statistics…'));
    return;
  }
  if (s.lineage === null) {
    el.replaceChildren(h('p', { class: 'muted' }, 'You have no lineage in this world yet.'));
    return;
  }
  // Ages, pregnancies, generation and genes cover agents only.
  const kv = h('div', { class: 'stat-tiles' },
    ...([['Alive', s.alive], ['Adults', s.adults], ['Young', s.juveniles], ...(s.eggs
      // One tile, as the Lineage tab says it (playtest 7: "Clutches 0" beside "Eggs" read as one egg in no clutch).
      ? [[s.eggs.clutches === 0 ? 'Clutches now (no eggs)' : `Clutches now (${s.eggs.eggs} egg${s.eggs.eggs === 1 ? '' : 's'})`, s.eggs.clutches]] as [string, number][]
      : [['Pregnant', s.pregnant]] as [string, number][]), ['Generation', s.max_generation]] as [string, number][])
      .map(([k, v]) => h('div', { class: 'tile' }, h('strong', {}, String(v)), h('span', { class: 'muted small' }, k))),
    s.agents < s.alive ? h('div', { class: 'tile', title: 'Level of detail: the rest run as statistical cohorts where no player is watching' },
      h('strong', {}, String(s.agents)), h('span', { class: 'muted small' }, 'On the map')) : null);
  const st = stackVitals(s.vitals);
  const births = s.vitals.reduce((n, v) => n + v.births, 0);
  const deaths = st.days.reduce((n, d) => n + d.deaths, 0);
  const latest = s.legacy[s.legacy.length - 1];
  const drift = driftRows(s.genes, TOP_DRIFT);
  const recorded = s.history_days.length;
  const all = [...s.genes].sort((a, b) => traitName(a.gene).localeCompare(traitName(b.gene)));
  const origins = originRows(s.origins ?? [], s.player);
  const own = origins.find((o) => o.mine)?.share ?? 0;
  const why = origins.length ? originExplain(s.origins ?? [], s.player) : null;
  // After extinction there are no members to measure: say so instead of charting zeros.
  const noMembers = s.agents === 0;
  // Keep the gene table open across refreshes.
  const tableOpen = el.querySelector<HTMLDetailsElement>('details#all-genes')?.open ?? false;
  el.replaceChildren(...([
    h('h3', {}, `${s.name ?? 'Lineage'} · day ${s.day + 1}`),
    kv,
    // Whose genes: right under the counts, since it decides the score (playtest 1, finding 2).
    ...(origins.length === 0 ? [] : [h('section', { class: `chart-card whose-genes${own < 0.9 ? ' diluted' : ''}`, id: 'allele-origins' },
      h('h4', {}, 'Whose genes'),
      h('p', { class: 'whose-headline', id: 'whose-headline' }, why!.headline),
      originBar(origins),
      h('ul', { class: 'origin-list' }, ...origins.map((o) => h('li', {},
        h('i', { class: 'dot square-fill', style: `background:${o.color}` }),
        h('span', {}, o.label),
        h('span', { class: 'muted small' }, ` ${pct(o.share)}`)))),
      h('ul', { class: 'whose-why small' }, ...why!.points.map((p) => h('li', {}, p))))]),
    card('Population by age', 'Females left, males right; oldest at the top.',
      legend([[SERIES[0], 'female'], [SERIES[1], 'male']]), pyramidChart(s.age_bands)),
    card('Births and deaths', `Last ${s.vitals.length} days (the last bar is today so far): ${births} born, ${deaths} died.`,
      legend([[SERIES[2], 'born'], ...st.causes.map((c) => [CAUSE_COLORS[c], CAUSE_LABELS[c] ?? c.replace('_', ' ')] as [string, string])]), vitalsChart(s.vitals),
      // The sim counts these together; the death alerts tell them apart.
      st.causes.includes('exposure') ? h('p', { class: 'muted small', id: 'exposure-note' }, EXPOSURE_NOTE) : null,
      onWhyDying ? h('p', { class: 'small' }, h('button', { type: 'button', class: 'link', id: 'why-dying', onclick: onWhyDying }, 'Why are my animals dying? What each cause means and what helps')) : null),
    card('Legacy share', latest ? `Now ${pct(latest.share, 2)} of breeding alleles (weighted ${pct(latest.weighted_share, 2)}).` : 'Recorded at the end of each day.',
      legend([[SERIES[0], 'share'], [SERIES[2], 'weighted by offspring raised']]), legacyChart(s.legacy)),
    card('Gene drift', noMembers ? 'No data: none of your organisms are alive to measure.' : recorded > 0 ? `Largest changes of the lineage mean since founding (${recorded} recorded days); dashed: founding value.` : 'Drift shows after the first recorded day.',
      noMembers ? null : h('div', { class: 'drift-list', id: 'drift-list' }, ...drift.map((d) => h('div', { class: 'drift-row' },
        h('span', { class: 'gene-name' }, d.label),
        geneSparkline(d),
        h('span', { class: 'small' }, `${geneValueText(d.gene, d.mean)} (${geneValueText(d.gene, d.drift, true)})`))))),
    noMembers ? null : h('details', { class: 'chart-card', id: 'all-genes', open: tableOpen },
      h('summary', {}, `All genes: distributions (${all.length})`),
      h('p', { class: 'muted small' }, 'Members per tenth of the 0 … 1 range; dashed: founding mean.'),
      h('table', { class: 'gene-table', id: 'gene-table' },
        h('thead', {}, h('tr', {}, h('th', {}, 'Gene'), h('th', {}, 'Mean'), h('th', { title: 'Standard deviation over living members' }, 'SD'), h('th', {}, '0 … 1'))),
        h('tbody', {}, ...all.map((g) => h('tr', {},
          h('td', {}, traitName(g.gene)), h('td', {}, geneValueText(g.gene, g.mean)), h('td', {}, g.sd.toFixed(3)),
          h('td', {}, g.bins && g.bins.length > 0 ? geneHistogram(traitName(g.gene), g.bins, g.founding_mean) : spreadBar(g.mean, g.sd, g.founding_mean))))))),
  ] as (Node | null)[]).filter((x): x is Node => x !== null));
}

export const KIND_LABEL: Record<FeedEntry['kind'], string> = {
  daily_summary: 'day', birth: 'birth', death: 'death', milestone: 'milestone', need: 'need', offer: 'offer',
  world: 'world', achievement: 'prestige', market: 'market', group: 'group', attack: 'attack',
};

/** The alerts feed, newest first. Entries naming an organism link to it. */
export function renderFeed(feed: FeedEntry[], onOrganism: (id: OrgId) => void): HTMLElement {
  if (feed.length === 0) return h('p', { class: 'muted small' }, 'Nothing yet: births, deaths, offers and milestones show here.');
  return h('ul', { class: 'feed-list' }, ...[...feed].reverse().map((e) => h('li', { class: `feed-item kind-${e.kind}` },
    h('span', { class: `feed-kind kind-${e.kind}` }, KIND_LABEL[e.kind]),
    h('span', {}, e.text),
    e.day !== undefined && h('span', { class: 'muted small' }, ` · day ${e.day + 1}`),
    e.organism !== undefined && h('button', { class: 'link', onclick: () => onOrganism(e.organism!) }, `#${e.organism}`))));
}

const KIND_HINT: Record<AlertKind, string> = {
  daily_summary: 'births and deaths of each day', birth: 'first offspring, new generations', death: 'possessed organisms and founders',
  milestone: 'population and legacy milestones', need: 'a possessed organism parched or starving', world: 'catastrophes and eras',
  achievement: 'achievements', market: 'sales, purchases, refunds, renown',
  group: 'a migration arrived, a group left with no members',
  attack: 'how your attack orders end, and players hunting yours',
};

/** Per-kind switches: show in the feed, and toast on arrival. `onChange` gets the new preferences. */
export function renderAlertPrefs(prefs: AlertPrefs, onChange: (p: AlertPrefs) => void): HTMLElement {
  const box = (kind: AlertKind, field: 'show' | 'toast', on: boolean) => h('input', {
    type: 'checkbox', checked: on, 'data-kind': kind, 'data-field': field, 'aria-label': `${field} ${kind.replace('_', ' ')}`,
    onchange: (e: Event) => onChange(setPref(prefs, kind, field, (e.target as HTMLInputElement).checked)),
  });
  return h('table', { class: 'prefs-table' },
    h('thead', {}, h('tr', {}, h('th', {}, 'Alert'), h('th', {}, 'Show'), h('th', {}, 'Toast'))),
    h('tbody', {}, ...ALERT_KINDS.map((k) => h('tr', {},
      h('td', { title: KIND_HINT[k] }, h('span', { class: `feed-kind kind-${k}` }, KIND_LABEL[k]), h('span', { class: 'muted small' }, KIND_HINT[k])),
      h('td', {}, box(k, 'show', shows(prefs, k))),
      h('td', {}, box(k, 'toast', toasts(prefs, k)))))));
}

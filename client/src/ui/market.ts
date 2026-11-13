// The Market tab: renown balance, the stud marketplace (browse with filters
// and compatibility, buy, list and withdraw) and the ledger. Talks to the
// server with `browse_market`, `create_listing`, `withdraw_listing`,
// `buy_listing` and `ledger`; the agreements it creates show in the Matings
// tab like any other.

import type { GameModel } from '../model';
import type { ClientMsg, LedgerView, ListingView, MarketView, OrgId, ResultMsg } from '../protocol';
import { parseLedgerView, parseMarketView } from '../protocol';
import { h, label, pct } from './dom';
import './market.css';
import {
  balanceText, buyProblem, DEFAULT_FILTER, expiresIn, filterListings, ledgerRows, ledgerTotals, listingCompat, ownAdults,
  traitSummary, type MarketFilter, type RoleFilter,
} from './marketData';

export interface MarketDeps {
  model: GameModel;
  send: (m: ClientMsg) => Promise<ResultMsg>;
  /** Show an organism (center the camera, open the inspector). */
  onOrganism: (id: OrgId, x?: number, y?: number) => void;
}

export interface MarketPanel {
  el: HTMLElement;
  /** Fetch listings and the ledger again. */
  refresh: () => Promise<void>;
  /** Re-render from the latest state (cheap; called on state pushes). */
  render: () => void;
}

const TICKS_PER_DAY = 48;

export function buildMarket(deps: MarketDeps): MarketPanel {
  const { model } = deps;
  let market: MarketView | null = null;
  let ledger: LedgerView | null = null;
  let filter: MarketFilter = { ...DEFAULT_FILTER };
  let fetching = false;

  // --- balance
  const balanceEl = h('p', { id: 'market-balance', class: 'market-balance' });

  // --- list one of my adults
  const listOrg = h('select', { id: 'market-list-organism' });
  const fee = h('input', { id: 'market-fee', type: 'number', min: 0, step: 1, value: 10 });
  const expiry = h('input', { id: 'market-expiry', type: 'number', min: 1, max: 365, placeholder: 'never' });
  const listBtn = h('button', { id: 'market-list-submit' }, 'List');
  listBtn.addEventListener('click', async () => {
    const organism = Number(listOrg.value);
    if (!organism) return model.toast('error', 'Pick one of your adults to list.');
    const msg: ClientMsg = { type: 'create_listing', organism, fee: Math.max(0, Math.floor(Number(fee.value) || 0)) };
    if (expiry.value !== '') msg.expires_days = Math.max(1, Math.floor(Number(expiry.value)));
    const r = await deps.send(msg);
    if (r.ok) {
      model.toast('info', `#${organism} listed for ${msg.fee} renown.`);
      await refresh();
    }
  });
  const myListingsEl = h('div', { id: 'my-listings' });

  // --- browse
  const mineSel = h('select', { id: 'market-mine' });
  const roleSel = h('select', { id: 'market-role' },
    h('option', { value: 'any' }, 'studs and dams'), h('option', { value: 'stud' }, 'studs (male)'), h('option', { value: 'dam' }, 'dams (female)'));
  const maxFee = h('input', { id: 'market-max-fee', type: 'number', min: 0, placeholder: 'any' });
  const compatOnly = h('input', { id: 'market-compatible', type: 'checkbox' });
  const refreshBtn = h('button', { id: 'market-refresh', class: 'secondary' }, 'Refresh');
  const listingsEl = h('div', { id: 'market-listings' });
  const readFilter = (): void => {
    filter = {
      role: roleSel.value as RoleFilter,
      maxFee: maxFee.value === '' ? null : Math.max(0, Number(maxFee.value)),
      compatibleOnly: compatOnly.checked,
      hideOwn: true,
    };
    renderListings();
  };
  roleSel.addEventListener('change', readFilter);
  maxFee.addEventListener('input', readFilter);
  compatOnly.addEventListener('change', readFilter);
  // A new buyer organism changes every listing's compatibility.
  mineSel.addEventListener('change', () => void refresh());
  refreshBtn.addEventListener('click', () => void refresh());

  // --- ledger
  const ledgerEl = h('div', { id: 'market-ledger' });

  const el = h('div', { class: 'market' },
    balanceEl,
    h('h3', {}, 'Stud marketplace'),
    h('div', { class: 'form market-filters' },
      h('label', {}, 'For my adult', mineSel),
      h('div', { class: 'row' }, h('label', {}, 'Show', roleSel), h('label', {}, 'Max fee', maxFee)),
      h('div', { class: 'row' }, h('label', { class: 'check' }, compatOnly, 'compatible only'), refreshBtn)),
    listingsEl,
    h('h3', {}, 'List one of my adults'),
    h('div', { class: 'form' },
      h('label', {}, 'Adult', listOrg),
      h('div', { class: 'row' }, h('label', {}, 'Fee (renown)', fee), h('label', {}, 'Expires after (days)', expiry)),
      h('div', { class: 'row' }, listBtn),
      h('p', { class: 'muted small' }, 'Anyone may buy one mating with a listed organism: the listing is your consent. The fee is held in escrow and paid to you when the two meet and mate; it is refunded if they never do. A dam takes one buyer at a time.')),
    h('h4', {}, 'My listings'), myListingsEl,
    h('h3', {}, 'Ledger'), ledgerEl);

  // Options of the two organism pickers, rebuilt only when the adults change.
  let optionsKey = '';
  function renderPickers(): void {
    const adults = ownAdults(model.state);
    const listed = new Set((market?.listings ?? []).filter((l) => l.owner === model.player).map((l) => l.organism));
    const key = adults.map((o) => `${o.id}${o.pregnant ? 'p' : ''}${listed.has(o.id) ? 'l' : ''}`).join(',');
    if (key === optionsKey) return;
    optionsKey = key;
    const keepMine = mineSel.value;
    const keepList = listOrg.value;
    const opt = (o: (typeof adults)[number]) => h('option', { value: o.id }, `#${o.id} ${o.sex}${o.pregnant ? ' (pregnant)' : ''} · gen ${o.generation}`);
    mineSel.replaceChildren(h('option', { value: '' }, '(none: no compatibility)'), ...adults.map(opt));
    if ([...mineSel.options].some((o) => o.value === keepMine)) mineSel.value = keepMine;
    listOrg.replaceChildren(...adults.filter((o) => !listed.has(o.id)).map(opt));
    if ([...listOrg.options].some((o) => o.value === keepList)) listOrg.value = keepList;
  }

  function renderBalance(): void {
    const s = model.state;
    const slots = s?.slots_left ?? market?.slots_left ?? 0;
    balanceEl.replaceChildren(
      h('strong', { id: 'market-balance-value' }, balanceText(s?.balance ?? market?.balance, s?.escrow ?? market?.escrow)),
      h('span', { class: 'muted small' }, ` · ${slots} mating slot${slots === 1 ? '' : 's'} left this season`));
  }

  function listingCard(l: ListingView, own: boolean): HTMLElement {
    const s = model.state;
    const tick = s?.clock.tick ?? 0;
    const days = expiresIn(l, tick, TICKS_PER_DAY);
    const mine = s?.organisms.find((o) => o.id === Number(mineSel.value));
    const compat = listingCompat(l);
    const problem = own ? null : buyProblem(l, model.player, s?.balance ?? market?.balance ?? 0, s?.slots_left ?? 0, mine);
    const parents = l.mother !== undefined || l.father !== undefined
      ? `of #${l.mother ?? '?'} × #${l.father ?? '?'}` : 'a founder';
    const buy = h('button', { class: 'buy', disabled: problem !== null, title: problem ?? `Buy one mating for ${l.fee} renown` }, `Buy for ${l.fee}`);
    buy.addEventListener('click', async () => {
      if (!mine) return;
      const r = await deps.send({ type: 'buy_listing', listing: l.id, mine: mine.id });
      if (r.ok) {
        model.toast('info', `Bought a mating of #${mine.id} with ${l.owner_name}'s #${l.organism}: escort it from the Matings tab.`);
        await refresh();
      }
    });
    const withdraw = h('button', { class: 'secondary withdraw' }, 'Withdraw');
    withdraw.addEventListener('click', async () => {
      const r = await deps.send({ type: 'withdraw_listing', listing: l.id });
      if (r.ok) await refresh();
    });
    return h('div', { class: `listing${l.busy ? ' busy' : ''}`, 'data-listing': l.id, 'data-organism': l.organism },
      h('div', { class: 'listing-head' },
        h('strong', {}, `${l.role === 'stud' ? 'Stud' : 'Dam'} #${l.organism}`),
        h('span', { class: 'muted' }, ` · ${l.owner_name}'s ${l.lineage_name}`),
        h('span', { class: 'fee' }, `${l.fee} renown`)),
      h('div', { class: 'small' }, traitSummary(l.traits)),
      h('div', { class: 'small muted' },
        `gen ${l.generation}, ${parents} · ${l.age_days.toFixed(0)} days · health ${l.health.toFixed(0)} · raised ${l.offspring_raised}`,
        ` · at ${l.x.toFixed(0)}, ${l.y.toFixed(0)}`,
        l.sales ? ` · ${l.sales} sold` : '',
        days !== null ? ` · ${days < 1 ? 'expires today' : `${days.toFixed(0)} days left`}` : '',
        l.busy ? ' · promised' : '', l.pregnant ? ' · pregnant' : ''),
      h('div', { class: 'trait-bars' }, ...l.traits.filter((t) => t.value >= 0.5 || t.gene === 'body_size').slice(0, 6).map((t) =>
        h('span', { class: 'trait', title: `${label(t.gene)} ${t.value.toFixed(2)}` }, label(t.gene),
          h('span', { class: 'trait-bar' }, h('span', { style: `width:${pct(Math.max(0, Math.min(1, t.value)), 0)}` }))))),
      compat && h('div', { class: `small compat`, 'data-level': compat.level }, compat.text),
      h('div', { class: 'row' },
        own ? withdraw : buy,
        h('button', { class: 'secondary', onclick: () => deps.onOrganism(l.organism, l.x, l.y) }, 'Show'),
        !own && problem && h('span', { class: 'muted small' }, problem)));
  }

  // Rebuild the listing cards only when what they show changes.
  let listingsKey = '';
  function renderListings(): void {
    const s = model.state;
    const all = market?.listings ?? [];
    const shown = filterListings(all, filter, model.player);
    const mine = s?.organisms.find((o) => o.id === Number(mineSel.value));
    const key = JSON.stringify([shown.map((l) => [l.id, l.sales, l.busy, l.pregnant, l.preview?.fertility, l.preview?.distance?.toFixed(0)]),
      s?.balance, s?.slots_left, mine?.id, mine?.pregnant, market === null]);
    if (key !== listingsKey) {
      listingsKey = key;
      listingsEl.replaceChildren(...(market === null ? [h('p', { class: 'muted small' }, 'Loading…')]
        : shown.length === 0
          ? [h('p', { class: 'muted small' }, all.length === 0 ? 'No listings yet.' : 'No listings match the filters.')]
          : shown.map((l) => listingCard(l, false))));
    }
    const own = all.filter((l) => l.owner === model.player);
    const ownKey = JSON.stringify(own.map((l) => [l.id, l.sales, l.busy]));
    if (myListingsEl.dataset.key !== ownKey) {
      myListingsEl.dataset.key = ownKey;
      myListingsEl.replaceChildren(...(own.length === 0 ? [h('p', { class: 'muted small' }, 'None.')] : own.map((l) => listingCard(l, true))));
    }
  }

  function renderLedger(): void {
    if (!ledger) {
      ledgerEl.replaceChildren(h('p', { class: 'muted small' }, 'Loading…'));
      return;
    }
    const rows = ledgerRows(ledger.entries);
    const t = ledgerTotals(ledger.entries);
    const key = `${rows[0]?.seq ?? 0}:${rows.length}`;
    if (ledgerEl.dataset.key === key) return;
    ledgerEl.dataset.key = key;
    ledgerEl.replaceChildren(
      h('p', { class: 'muted small' }, `Recent: earned ${t.earned}, paid into escrow ${t.spent}, sales ${t.sold}, refunds ${t.refunded}.`),
      rows.length === 0 ? h('p', { class: 'muted small' }, 'No entries yet.')
        : h('table', { class: 'board ledger' },
          h('thead', {}, h('tr', {}, h('th', {}, 'Day'), h('th', {}, ''), h('th', {}, 'Renown'), h('th', {}, 'What'), h('th', {}, 'Balance'))),
          h('tbody', {}, ...rows.map((r) => h('tr', { 'data-seq': r.seq, class: `ledger-${r.kind}` },
            h('td', {}, String(r.day + 1)), h('td', {}, h('span', { class: 'feed-kind' }, r.kind)),
            h('td', { class: r.positive ? 'good' : 'bad' }, r.amount), h('td', { class: 'small' }, r.text), h('td', {}, String(r.balance)))))));
  }

  function render(): void {
    renderBalance();
    renderPickers();
    renderListings();
    renderLedger();
  }

  async function refresh(): Promise<void> {
    if (fetching) return;
    fetching = true;
    try {
      const mine = Number(mineSel.value) || undefined;
      const [m, l] = await Promise.all([deps.send(mine ? { type: 'browse_market', mine } : { type: 'browse_market' }), deps.send({ type: 'ledger', limit: 50 })]);
      if (m.ok) market = parseMarketView(m.data) ?? market;
      if (l.ok) ledger = parseLedgerView(l.data) ?? ledger;
    } finally {
      fetching = false;
    }
    render();
  }

  render();
  return { el, refresh, render };
}

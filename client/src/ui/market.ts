// The Market tab: renown balance, the stud marketplace (browse page by page
// with server-side filters, order and compatibility; buy, bid, list,
// auction and withdraw) and the ledger. Talks to the server with
// `browse_market`, `create_listing`, `create_auction`, `withdraw_listing`,
// `buy_listing`, `bid` and `ledger`; the agreements it creates show in the
// Matings tab like any other.

import type { GameModel } from '../model';
import type { ClientMsg, LedgerView, ListingView, MarketSort, MarketView, OrgId, ResultMsg } from '../protocol';
import { parseLedgerView, parseMarketView } from '../protocol';
import { h, label, pct } from './dom';
import './market.css';
import {
  auctionText, balanceText, bidProblem, buyProblem, DEFAULT_FILTER, expiresIn, ledgerRows, ledgerTotals, listingCompat,
  marketQuery, mergePages, ownAdults, pageText, traitSummary, type KindFilter, type MarketFilter, type RoleFilter,
} from './marketData';

export interface MarketDeps {
  model: GameModel;
  send: (m: ClientMsg) => Promise<ResultMsg>;
  /** Show an organism (center the camera, open the inspector). */
  onOrganism: (id: OrgId, x?: number, y?: number) => void;
}

export interface MarketPanel {
  el: HTMLElement;
  /** Fetch the loaded listing pages and the ledger again. */
  refresh: () => Promise<void>;
  /** Re-render from the latest state (cheap; called on state pushes). */
  render: () => void;
}

const TICKS_PER_DAY = 48;
/** Pages kept on a refresh (more are fetched with "Load more"). */
const MAX_PAGES = 10;

export function buildMarket(deps: MarketDeps): MarketPanel {
  const { model } = deps;
  /** The last page fetched (balance, total, cursor) and every page loaded so far. */
  let market: MarketView | null = null;
  let pages: ListingView[][] = [];
  let own: ListingView[] = [];
  let ledger: LedgerView | null = null;
  let filter: MarketFilter = { ...DEFAULT_FILTER };
  let fetching = false;
  /** A refresh asked for while another was in flight: run it after (it may carry new filters or follow a change). */
  let again = false;

  // --- balance
  const balanceEl = h('p', { id: 'market-balance', class: 'market-balance' });

  // --- list one of my adults (fixed fee or auction)
  const listOrg = h('select', { id: 'market-list-organism' });
  const listKind = h('select', { id: 'market-list-kind' },
    h('option', { value: 'fixed' }, 'fixed fee'), h('option', { value: 'auction' }, 'auction'));
  const fee = h('input', { id: 'market-fee', type: 'number', min: 0, step: 1, value: 10 });
  const expiry = h('input', { id: 'market-expiry', type: 'number', min: 1, max: 365, placeholder: 'never' });
  const auctionDays = h('input', { id: 'market-auction-days', type: 'number', min: 1, max: 30, value: 1 });
  const feeLabel = h('label', {}, 'Fee (renown)', fee);
  const expiryLabel = h('label', {}, 'Expires after (days)', expiry);
  const daysLabel = h('label', { hidden: true }, 'Closes after (days)', auctionDays);
  listKind.addEventListener('change', () => {
    const auction = listKind.value === 'auction';
    feeLabel.firstChild!.textContent = auction ? 'Reserve (renown)' : 'Fee (renown)';
    expiryLabel.hidden = auction;
    daysLabel.hidden = !auction;
    listBtn.textContent = auction ? 'Auction' : 'List';
  });
  const listBtn = h('button', { id: 'market-list-submit' }, 'List');
  listBtn.addEventListener('click', async () => {
    const organism = Number(listOrg.value);
    if (!organism) return model.toast('error', 'Pick one of your adults to list.');
    const price = Math.max(0, Math.floor(Number(fee.value) || 0));
    let msg: ClientMsg;
    if (listKind.value === 'auction') {
      msg = { type: 'create_auction', organism, reserve: price, days: Math.max(1, Math.floor(Number(auctionDays.value) || 1)) };
    } else {
      msg = { type: 'create_listing', organism, fee: price };
      if (expiry.value !== '') msg.expires_days = Math.max(1, Math.floor(Number(expiry.value)));
    }
    const r = await deps.send(msg);
    if (r.ok) {
      model.toast('info', msg.type === 'create_auction' ? `#${organism} auctioned from ${price} renown.` : `#${organism} listed for ${price} renown.`);
      await refresh();
    }
  });
  const myListingsEl = h('div', { id: 'my-listings' });

  // --- browse
  const mineSel = h('select', { id: 'market-mine' });
  const roleSel = h('select', { id: 'market-role' },
    h('option', { value: 'any' }, 'studs and dams'), h('option', { value: 'stud' }, 'studs (male)'), h('option', { value: 'dam' }, 'dams (female)'));
  const kindSel = h('select', { id: 'market-kind' },
    h('option', { value: 'any' }, 'fixed and auctions'), h('option', { value: 'fixed' }, 'fixed fee'), h('option', { value: 'auction' }, 'auctions'));
  const sortSel = h('select', { id: 'market-sort' },
    h('option', { value: 'fertility' }, 'best match'), h('option', { value: 'newest' }, 'newest'),
    h('option', { value: 'price_asc' }, 'cheapest'), h('option', { value: 'price_desc' }, 'priciest'),
    h('option', { value: 'ending' }, 'closing soonest'));
  const maxFee = h('input', { id: 'market-max-fee', type: 'number', min: 0, placeholder: 'any' });
  const compatOnly = h('input', { id: 'market-compatible', type: 'checkbox' });
  const refreshBtn = h('button', { id: 'market-refresh', class: 'secondary' }, 'Refresh');
  const listingsEl = h('div', { id: 'market-listings' });
  const pageEl = h('p', { id: 'market-page', class: 'muted small' });
  const moreBtn = h('button', { id: 'market-more', class: 'secondary', hidden: true }, 'Load more');
  moreBtn.addEventListener('click', () => void loadMore());
  // A filter, the order or the buyer organism changes the pages: start over.
  const readFilter = (): void => {
    filter = {
      role: roleSel.value as RoleFilter,
      kind: kindSel.value as KindFilter,
      maxFee: maxFee.value === '' ? null : Math.max(0, Number(maxFee.value)),
      compatibleOnly: compatOnly.checked,
      hideOwn: true,
      sort: sortSel.value as MarketSort,
    };
    pages = pages.slice(0, 1);
    // Rebuild the cards for a new buyer organism at once (their buttons
    // act for it), then fetch its compatibility.
    renderListings();
    void refresh();
  };
  for (const el of [roleSel, kindSel, sortSel, compatOnly, mineSel]) el.addEventListener('change', readFilter);
  maxFee.addEventListener('change', readFilter);
  refreshBtn.addEventListener('click', () => void refresh());

  // --- ledger
  const ledgerEl = h('div', { id: 'market-ledger' });

  const el = h('div', { class: 'market' },
    balanceEl,
    h('h3', {}, 'Stud marketplace'),
    h('div', { class: 'form market-filters' },
      h('label', {}, 'For my adult', mineSel),
      h('div', { class: 'row' }, h('label', {}, 'Show', roleSel), h('label', {}, 'Kind', kindSel)),
      h('div', { class: 'row' }, h('label', {}, 'Order', sortSel), h('label', {}, 'Max price', maxFee)),
      h('div', { class: 'row' }, h('label', { class: 'check' }, compatOnly, 'compatible only'), refreshBtn)),
    listingsEl,
    h('div', { class: 'row market-paging' }, pageEl, moreBtn),
    h('h3', {}, 'List one of my adults'),
    h('div', { class: 'form' },
      h('div', { class: 'row' }, h('label', {}, 'Adult', listOrg), h('label', {}, 'As', listKind)),
      h('div', { class: 'row' }, feeLabel, expiryLabel, daysLabel),
      h('div', { class: 'row' }, listBtn),
      h('p', { class: 'muted small' }, 'Anyone may buy one mating with a listed organism: the listing is your consent. The fee is held in escrow and paid to you when the two meet and mate; it is refunded if they never do. A dam takes one buyer at a time. An auction sells one mating to the highest bid at or above the reserve when it closes; bids are held in escrow and refunded when outbid.')),
    h('h4', {}, 'My listings'), myListingsEl,
    h('h3', {}, 'Ledger'), ledgerEl);

  // Options of the two organism pickers, rebuilt only when the adults change.
  let optionsKey = '';
  function renderPickers(): void {
    const adults = ownAdults(model.state);
    const listed = new Set(own.map((l) => l.organism));
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

  function chosen() {
    return model.state?.organisms.find((o) => o.id === Number(mineSel.value));
  }

  function listingCard(l: ListingView, isOwn: boolean): HTMLElement {
    const s = model.state;
    const tick = s?.clock.tick ?? 0;
    const days = expiresIn(l, tick, TICKS_PER_DAY);
    const mine = chosen();
    const compat = listingCompat(l);
    const balance = s?.balance ?? market?.balance ?? 0;
    const slots = s?.slots_left ?? 0;
    const a = l.auction;
    const parents = l.mother !== undefined || l.father !== undefined
      ? `of #${l.mother ?? '?'} × #${l.father ?? '?'}` : 'a founder';
    let action: HTMLElement;
    let problem: string | null = null;
    if (isOwn) {
      const hasBids = a?.high_bid !== undefined;
      action = h('button', { class: 'secondary withdraw', disabled: hasBids, title: hasBids ? 'An auction with a bid runs to its close' : 'Take it off the market' }, 'Withdraw');
      action.addEventListener('click', async () => {
        const r = await deps.send({ type: 'withdraw_listing', listing: l.id });
        if (r.ok) await refresh();
      });
    } else if (a) {
      const amount = h('input', { class: 'bid-amount', type: 'number', min: a.min_bid, step: 1, value: a.min_bid });
      const bid = h('button', { class: 'bid' }, 'Bid');
      const check = (): void => {
        const why = bidProblem(l, model.player, balance, slots, mine, Math.floor(Number(amount.value)));
        bid.disabled = why !== null;
        bid.title = why ?? `Bid ${amount.value} renown (held in escrow, refunded if outbid)`;
      };
      amount.addEventListener('input', check);
      check();
      problem = bidProblem(l, model.player, balance, slots, mine, a.min_bid);
      bid.addEventListener('click', async () => {
        if (!mine) return;
        const value = Math.floor(Number(amount.value));
        const r = await deps.send({ type: 'bid', listing: l.id, amount: value, mine: mine.id });
        if (r.ok) {
          model.toast('info', `Bid ${value} renown on auction #${l.id} for #${mine.id} (held in escrow).`);
          await refresh();
        }
      });
      action = h('span', { class: 'bid-form' }, amount, bid);
    } else {
      problem = buyProblem(l, model.player, balance, slots, mine);
      const buy = h('button', { class: 'buy', disabled: problem !== null, title: problem ?? `Buy one mating for ${l.fee} renown` }, `Buy for ${l.fee}`);
      buy.addEventListener('click', async () => {
        if (!mine) return;
        const r = await deps.send({ type: 'buy_listing', listing: l.id, mine: mine.id });
        if (r.ok) {
          model.toast('info', `Bought a mating of #${mine.id} with ${l.owner_name}'s #${l.organism}: escort it from the Matings tab.`);
          await refresh();
        }
      });
      action = buy;
    }
    const price = a ? `${a.high_bid ?? a.reserve} renown` : `${l.fee} renown`;
    return h('div', { class: `listing${l.busy ? ' busy' : ''}${a ? ' auction' : ''}`, 'data-listing': l.id, 'data-organism': l.organism, 'data-kind': l.kind },
      h('div', { class: 'listing-head' },
        h('strong', {}, `${l.role === 'stud' ? 'Stud' : 'Dam'} #${l.organism}`),
        h('span', { class: 'muted' }, ` · ${l.owner_name}'s ${l.lineage_name}`),
        h('span', { class: 'fee' }, price)),
      a && h('div', { class: 'small auction-line' }, auctionText(l, model.player, tick, TICKS_PER_DAY)),
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
        action,
        h('button', { class: 'secondary', onclick: () => deps.onOrganism(l.organism, l.x, l.y) }, 'Show'),
        !isOwn && problem && h('span', { class: 'muted small' }, problem)));
  }

  // Rebuild the listing cards only when what they show changes.
  let listingsKey = '';
  function renderListings(): void {
    const s = model.state;
    const shown = mergePages(pages);
    const mine = chosen();
    // What the cards show: the listing data and whether (and why not) it
    // can be bought, not the balance itself (which changes every day). And
    // the buyer organism: the Buy and Bid buttons act for the one picked
    // when the card was built, so a new pick must rebuild them.
    const balance = s?.balance ?? market?.balance ?? 0;
    const slots = s?.slots_left ?? 0;
    const why = (l: ListingView) => (l.auction ? bidProblem(l, model.player, balance, slots, mine, l.auction.min_bid) : buyProblem(l, model.player, balance, slots, mine));
    const key = JSON.stringify([shown.map((l) => [l.id, l.sales, l.busy, l.pregnant, l.preview?.fertility, l.preview?.distance?.toFixed(0),
      l.auction?.high_bid, l.auction?.high_bidder, why(l)]), market === null, mine?.id ?? null]);
    if (key !== listingsKey) {
      listingsKey = key;
      listingsEl.replaceChildren(...(market === null ? [h('p', { class: 'muted small' }, 'Loading…')]
        : shown.length === 0
          ? [h('p', { class: 'muted small' }, 'No listings match the filters.')]
          : shown.map((l) => listingCard(l, false))));
    }
    // The time left on auctions changes every tick: update the line in
    // place rather than rebuilding the cards (and a bid being typed).
    refreshAuctionLines(listingsEl, shown);
    refreshAuctionLines(myListingsEl, own);
    pageEl.textContent = pageText(shown.length, market);
    moreBtn.hidden = !market?.next_cursor;
    const ownKey = JSON.stringify(own.map((l) => [l.id, l.sales, l.busy, l.auction?.high_bid, l.auction?.bids]));
    if (myListingsEl.dataset.key !== ownKey) {
      myListingsEl.dataset.key = ownKey;
      myListingsEl.replaceChildren(...(own.length === 0 ? [h('p', { class: 'muted small' }, 'None.')] : own.map((l) => listingCard(l, true))));
    }
  }

  function refreshAuctionLines(root: HTMLElement, ls: ListingView[]): void {
    const tick = model.state?.clock.tick ?? 0;
    const byId = new Map(ls.map((l) => [l.id, l]));
    for (const card of root.querySelectorAll<HTMLElement>('.listing[data-kind="auction"]')) {
      const l = byId.get(Number(card.dataset.listing));
      const line = card.querySelector('.auction-line');
      if (l && line) line.textContent = auctionText(l, model.player, tick, TICKS_PER_DAY);
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

  const mineId = (): OrgId | undefined => Number(mineSel.value) || undefined;

  /** Fetch one page; null if the reply was refused or malformed. */
  async function fetchPage(cursor?: string): Promise<MarketView | null> {
    const r = await deps.send(marketQuery(filter, mineId(), cursor));
    return r.ok ? parseMarketView(r.data) : null;
  }

  /** Fetch the first page again, and as many after it as were loaded. */
  async function refresh(): Promise<void> {
    if (fetching) {
      again = true;
      return;
    }
    fetching = true;
    again = false;
    try {
      const want = Math.max(1, Math.min(MAX_PAGES, pages.length));
      const [first, l] = await Promise.all([fetchPage(), deps.send({ type: 'ledger', limit: 50 })]);
      if (l.ok) ledger = parseLedgerView(l.data) ?? ledger;
      if (first) {
        const next: ListingView[][] = [first.listings];
        own = first.own ?? own;
        let last = first;
        while (next.length < want && last.next_cursor) {
          const p = await fetchPage(last.next_cursor);
          if (!p) break;
          next.push(p.listings);
          last = p;
        }
        market = last;
        pages = next;
      }
    } finally {
      fetching = false;
    }
    render();
    if (again) await refresh();
  }

  /** Append the next page. */
  async function loadMore(): Promise<void> {
    const cursor = market?.next_cursor;
    if (fetching || !cursor) return;
    fetching = true;
    try {
      const p = await fetchPage(cursor);
      if (p) {
        market = p;
        pages = [...pages, p.listings];
      }
    } finally {
      fetching = false;
    }
    render();
    if (again) await refresh();
  }

  render();
  return { el, refresh, render };
}

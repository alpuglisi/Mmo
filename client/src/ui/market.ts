// The Market tab: renown balance, the stud marketplace (browse page by page
// with server-side filters, order and compatibility; buy, bid, list,
// auction and withdraw) and the ledger. Talks to the server with
// `browse_market`, `create_listing`, `create_auction`, `withdraw_listing`,
// `buy_listing`, `bid` and `ledger`; the agreements it creates show in the
// Matings tab like any other. Also the renown shop (`renown_shop`,
// `buy_mating_slot`, `reinforce`) and which lineages can breed with the
// player's (`compatibility`).

import { reconcile, setText } from './reconcile';
import type { GameModel } from '../model';
import type { ClientMsg, CompatibilityView, LedgerView, ListingView, MarketSort, MarketView, OrgId, ResultMsg, ShopView } from '../protocol';
import { parseCompatibilityView, parseLedgerView, parseMarketView, parseShopView } from '../protocol';
import { h, pct } from './dom';
import { traitName } from './traitNames';
import './market.css';
import {
  auctionText, expiryText, balanceText, bidProblem, buyProblem, compatRowText, DEFAULT_FILTER, shopText, expiresIn, ledgerRows, ledgerTotals, listingCompat,
  marketQuery, mergePages, ownAdults, pageText, traitSummary, type KindFilter, type MarketFilter, type RoleFilter,
} from './marketData';

export interface MarketDeps {
  model: GameModel;
  send: (m: ClientMsg) => Promise<ResultMsg>;
  /** Show an organism (center the camera, open the inspector). */
  onOrganism: (id: OrgId, x?: number, y?: number) => void;
  /** Measured real milliseconds per tick (for "≈ 2 min" next to game time). */
  msPerTick?: () => number;
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
  let shop: ShopView | null = null;
  let compat: CompatibilityView | null = null;
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

  // --- renown shop and who can breed with whom
  const shopEl = h('div', { id: 'market-shop' });
  const compatEl = h('div', { id: 'market-compat' });

  const el = h('div', { class: 'market' },
    balanceEl,
    h('h3', {}, 'Who your lineage can breed with'), compatEl,
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
    h('h3', {}, 'Renown shop'), shopEl,
    h('h3', {}, 'Ledger'), ledgerEl);

  // Options of the two organism pickers, rebuilt only when the adults change.
  let optionsKey = '';
  function renderPickers(): void {
    const adults = ownAdults(model.state);
    const listed = new Set(own.map((l) => l.organism));
    const key = adults.map((o) => `${o.id}${o.pregnant ? 'p' : ''}${listed.has(o.id) ? 'l' : ''}`).join(',');
    if (key === optionsKey) return;
    optionsKey = key;
    // Options updated in place: an open list stays open, the choice stays put.
    const label = (o: (typeof adults)[number] | null) => (o ? `#${o.id} ${o.sex}${o.pregnant ? ' (pregnant)' : ''} · gen ${o.generation}` : '(none: no compatibility)');
    const option = (o: (typeof adults)[number] | null) => h('option', { value: o ? o.id : '' }) as HTMLOptionElement;
    const fill = (opt: HTMLOptionElement, o: (typeof adults)[number] | null) => setText(opt, label(o));
    reconcile(mineSel, [null, ...adults], (o) => (o ? String(o.id) : ''), option, fill);
    reconcile(listOrg, adults.filter((o) => !listed.has(o.id)), (o) => String(o.id), option, fill);
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
          model.toast('info', `Bought a mating of #${mine.id} with ${l.owner_name}'s #${l.organism}: escort it from the Mates tab.`);
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
      a && h('div', { class: 'small auction-line' }, auctionText(l, model.player, tick, TICKS_PER_DAY, deps.msPerTick?.())),
      h('div', { class: 'small' }, traitSummary(l.traits)),
      h('div', { class: 'small muted' },
        `gen ${l.generation}, ${parents} · ${l.age_days.toFixed(0)} days · health ${l.health.toFixed(0)} · raised ${l.offspring_raised}`,
        ` · at ${l.x.toFixed(0)}, ${l.y.toFixed(0)}`,
        l.sales ? ` · ${l.sales} sold` : '',
        l.busy ? ' · promised' : '', l.pregnant ? ' · pregnant' : ''),
      days !== null && h('div', { class: 'small expires-line' }, expiryText(l, tick, TICKS_PER_DAY, deps.msPerTick?.()) ?? ''),
      h('div', { class: 'trait-bars' }, ...l.traits.filter((t) => t.value >= 0.5 || t.gene === 'body_size').slice(0, 6).map((t) =>
        h('span', { class: 'trait', title: `${traitName(t.gene)} ${t.value.toFixed(2)}` }, traitName(t.gene),
          h('span', { class: 'trait-bar' }, h('span', { style: `width:${pct(Math.max(0, Math.min(1, t.value)), 0)}` }))))),
      compat && h('div', { class: `small compat`, 'data-level': compat.level }, compat.text),
      l.compatible === false && !isOwn && h('div', { class: 'small compat', 'data-level': 'bad' }, 'Sterile with your lineage: body plans too different.'),
      h('div', { class: 'row' },
        action,
        h('button', { class: 'secondary', onclick: () => deps.onOrganism(l.organism, l.x, l.y) }, 'Show'),
        // Why Buy / Bid is off, in words next to it (not only in a hover title).
        !isOwn && problem && h('span', { class: 'small why-not' }, `Can't ${a ? 'bid' : 'buy'}: ${problem === 'pick one of your adults' ? 'pick your adult in "For my adult" above' : problem}`)));
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
    const key = JSON.stringify([shown.map((l) => [l.id, l.sales, l.busy, l.pregnant, l.preview?.fertility, l.preview?.compatible,
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
    const ms = deps.msPerTick?.();
    // The countdowns tick on between rebuilds of the cards.
    for (const card of root.querySelectorAll<HTMLElement>('.listing')) {
      const l = byId.get(Number(card.dataset.listing));
      if (!l) continue;
      const auctionLine = card.querySelector('.auction-line');
      if (auctionLine) setText(auctionLine, auctionText(l, model.player, tick, TICKS_PER_DAY, ms));
      const expires = card.querySelector('.expires-line');
      if (expires) setText(expires, expiryText(l, tick, TICKS_PER_DAY, ms) ?? '');
      const compat = card.querySelector<HTMLElement>('.compat');
      const c = listingCompat(l);
      if (compat && c) {
        setText(compat, c.text);
        compat.dataset.level = c.level;
      }
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

  function renderShop(): void {
    if (!shop) {
      shopEl.replaceChildren(h('p', { class: 'muted small' }, 'Loading…'));
      return;
    }
    const s = shop;
    const key = JSON.stringify(s);
    if (shopEl.dataset.key === key) return;
    shopEl.dataset.key = key;
    const slotBtn = h('button', { id: 'shop-slot', class: 'secondary', disabled: s.slot_price === undefined || s.balance < s.slot_price },
      s.slot_price === undefined ? 'Slot limit reached' : `Extra mating slot: ${s.slot_price} renown`);
    slotBtn.addEventListener('click', async () => {
      const r = await deps.send({ type: 'buy_mating_slot' });
      if (r.ok) {
        shop = parseShopView(r.data) ?? shop;
        model.toast('info', 'One more directed-mating slot this season.');
        render();
      }
    });
    const blocked = s.reinforce_blocked;
    const reinBtn = h('button', { id: 'shop-reinforce', class: 'secondary', disabled: blocked !== undefined || s.balance < s.reinforce_price,
      title: blocked ?? `${s.reinforce_size} adults copied from your own join your lineage` },
    `Reinforcements (${s.reinforce_size} adults): ${s.reinforce_price} renown`);
    reinBtn.addEventListener('click', async () => {
      const r = await deps.send({ type: 'reinforce' });
      if (r.ok) {
        const d = r.data as { added?: unknown[]; shop?: unknown } | undefined;
        shop = parseShopView(d?.shop) ?? shop;
        model.toast('info', `${d?.added?.length ?? 0} adults joined your lineage.`);
        render();
      }
    });
    shopEl.replaceChildren(
      h('p', { class: 'muted small' }, shopText(s)),
      h('div', { class: 'row' }, slotBtn, h('span', { class: 'muted small' }, `${s.slots_bought} bought this season; the price doubles with each.`)),
      h('div', { class: 'row' }, reinBtn, h('span', { class: 'muted small' },
        blocked ?? `For lineages of at most ${s.reinforce_max_alive}; the price doubles with each in a ranked season.`)));
  }

  function renderCompat(): void {
    const key = JSON.stringify(compat);
    if (compatEl.dataset.key === key) return;
    compatEl.dataset.key = key;
    if (!compat || compat.lineage === undefined) {
      compatEl.replaceChildren(h('p', { class: 'muted small' }, compat ? 'No living adult to compare.' : 'Loading…'));
      return;
    }
    const fertile = compat.rows.filter((r) => r.compatible);
    const sterile = compat.rows.filter((r) => !r.compatible);
    compatEl.replaceChildren(
      h('p', { class: 'muted small' }, 'Lineages whose diet, warmth, birth mode or size differ by 0.30 or more from yours are sterile with it: only the fertile ones are worth trading with.'),
      h('ul', { class: 'small compat-list' },
        ...fertile.map((r) => h('li', { 'data-lineage': r.lineage, class: 'good' }, compatRowText(r))),
        ...sterile.slice(0, 6).map((r) => h('li', { 'data-lineage': r.lineage, class: 'muted' }, compatRowText(r)))),
      ...(fertile.length === 0 ? [h('p', { class: 'small' }, 'No lineage can breed with yours right now.')] : []));
  }

  function render(): void {
    renderBalance();
    renderPickers();
    renderListings();
    renderShop();
    renderCompat();
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
      const [first, l, sh, cp] = await Promise.all([fetchPage(), deps.send({ type: 'ledger', limit: 50 }),
        deps.send({ type: 'renown_shop' }), deps.send({ type: 'compatibility' })]);
      if (l.ok) ledger = parseLedgerView(l.data) ?? ledger;
      if (sh.ok) shop = parseShopView(sh.data) ?? shop;
      if (cp.ok) compat = parseCompatibilityView(cp.data) ?? compat;
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

// The Market tab: renown balance, the stud marketplace (browse page by page
// with server-side filters, order and compatibility; buy, bid, list,
// auction and withdraw) and the ledger. Talks to the server with
// `browse_market`, `create_listing`, `create_auction`, `withdraw_listing`,
// `buy_listing`, `bid` and `ledger`; the agreements it creates show in the
// Matings tab like any other. Also the renown shop (`renown_shop`,
// `buy_mating_slot`, `reinforce`) and which lineages can breed with the
// player's (`compatibility`).

import { reconcile, setText } from './reconcile';
import { createPicker, type PickItem } from './picker';
import type { GameModel } from '../model';
import type { ClientMsg, CompatibilityView, LedgerView, ListingView, MarketSort, MarketView, OrgId, ResultMsg, ShopView, SurveyView } from '../protocol';
import { parseCompatibilityView, parseLedgerView, parseMarketView, parseShopView } from '../protocol';
import { h, pct } from './dom';
import { traitName } from './traitNames';
import { commandLabel } from '../errors';
import './market.css';
import {
  auctionText, expiryText, balanceText, bidProblem, buyProblem, COMPAT_CONFIDENCE, compatRowText, DEFAULT_FILTER, shopText, ledgerRows, ledgerTotals, listingCompat,
  marketQuery, mergePages, ownAdults, pageText, traitSummary, type KindFilter, type MarketFilter, type RoleFilter,
} from './marketData';

export interface MarketDeps {
  model: GameModel;
  send: (m: ClientMsg) => Promise<ResultMsg>;
  /** Show an organism (center the camera, open the inspector). */
  onOrganism: (id: OrgId, x?: number, y?: number) => void;
  /** Measured real milliseconds per tick (for "≈ 2 min" next to game time). */
  msPerTick?: () => number;
  /** The camera's centre, for "nearest first" in the organism pickers. */
  cam?: () => { x: number; y: number };
  /** The player's nameplate changed (refetch everyone's). */
  onNameplate?: () => void;
}

export interface MarketPanel {
  el: HTMLElement;
  /** Fetch the loaded listing pages and the ledger again. */
  refresh: () => Promise<void>;
  /** Re-render from the latest state (cheap; called on state pushes). */
  render: () => void;
  /** A price from the renown shop, once loaded. */
  shopPrice: (key: 'species_name_price' | 'nameplate_price') => number | undefined;
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
  // Searchable pickers, not <select>s: a lineage can have thousands of adults.
  const listPick = createPicker({ id: 'market-list-organism', label: 'Adult to list', placeholder: 'Search your adults: id or words', filters: ['sex', 'notPregnant'], noneLabel: 'Choose an adult to list…' });
  const listOrg = listPick.el;
  const listKind = h('select', { id: 'market-list-kind' },
    h('option', { value: 'fixed' }, 'fixed fee'), h('option', { value: 'auction' }, 'auction'));
  // Every listing is at least 5 renown (server LISTING_MIN_FEE).
  const fee = h('input', { id: 'market-fee', type: 'number', min: 5, step: 1, value: 10 });
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
      model.dismissErrors(commandLabel(msg.type));
      const note = (r.data as { note?: string } | undefined)?.note;
      model.toast('info', msg.type === 'create_auction'
        ? `#${organism} auctioned from ${price} renown${note ? ` (${note})` : ''}.` : `#${organism} listed for ${price} renown.`);
      await refresh();
    }
  });
  const myListingsEl = h('div', { id: 'my-listings' });

  // --- browse
  const minePick = createPicker({ id: 'market-mine', label: 'For my adult', placeholder: 'Search your adults: id or words', filters: ['sex', 'notPregnant'], noneLabel: '(none picked: compatibility by lineage only)', allowNone: true });
  const mineSel = minePick.el;
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
  const offerPrice = h('input', { id: 'market-offer-price', type: 'number', min: 0, step: 1, value: 20 });

  const el = h('div', { class: 'market' },
    balanceEl,
    h('h3', {}, 'Who your lineage can breed with'), compatEl,
    h('h3', {}, 'Stud marketplace'),
    h('div', { class: 'form market-filters' },
      h('div', { class: 'field' }, h('span', { class: 'field-label' }, 'For my adult'), mineSel),
      h('div', { class: 'row' }, h('label', {}, 'Show', roleSel), h('label', {}, 'Kind', kindSel)),
      h('div', { class: 'row' }, h('label', {}, 'Order', sortSel), h('label', {}, 'Max price', maxFee)),
      h('div', { class: 'row' }, h('label', { class: 'check' }, compatOnly, 'compatible only'), refreshBtn)),
    listingsEl,
    h('div', { class: 'row market-paging' }, pageEl, moreBtn),
    h('h3', {}, 'List one of my adults'),
    h('div', { class: 'form' },
      h('div', { class: 'field' }, h('span', { class: 'field-label' }, 'Adult'), listOrg),
      h('div', { class: 'row' }, h('label', {}, 'As', listKind)),
      h('div', { class: 'row' }, feeLabel, expiryLabel, daysLabel),
      h('div', { class: 'row' }, listBtn),
      h('p', { class: 'muted small' }, 'Anyone may buy one mating with a listed organism: the listing is your consent. The fee is held in escrow and paid to you when the two meet and mate; it is refunded if they never do. A dam takes one buyer at a time. An auction sells one mating to the highest bid at or above the reserve when it closes; bids are held in escrow and refunded when outbid.')),
    h('h4', {}, 'My listings'), myListingsEl,
    h('h3', {}, 'Renown shop'), shopEl,
    h('h3', {}, 'Ledger'), ledgerEl);

  // The two organism pickers: nearest the view first, searchable by id.
  function renderPickers(): void {
    const adults = ownAdults(model.state);
    const listed = new Set(own.map((l) => l.organism));
    const cam = deps.cam?.() ?? null;
    const item = (o: (typeof adults)[number]): PickItem => ({
      id: o.id, label: `#${o.id} ${o.sex} · gen ${o.generation}`,
      detail: [o.pregnant && 'pregnant', listed.has(o.id) && 'listed'].filter(Boolean).join(', ') || undefined,
      sex: o.sex, pregnant: o.pregnant, free: !o.pregnant,
      distance: cam ? Math.hypot(o.x - cam.x, o.y - cam.y) : undefined,
    });
    minePick.setItems(adults.map(item));
    listPick.setItems(adults.filter((o) => !listed.has(o.id)).map(item));
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

  // The latest data of every shown listing, by id: the cards' buttons act on
  // what is current when clicked, not on what it was when the card was made.
  const latest = new Map<number, ListingView>();
  const current = (id: number): ListingView | undefined => latest.get(id);

  /** A listing card's fixed skeleton and handlers; `updateCard` fills it (every render). */
  function makeCard(l: ListingView, isOwn: boolean): HTMLElement {
    const id = l.id;
    let action: HTMLElement;
    if (isOwn) {
      action = h('button', { class: 'secondary withdraw' }, 'Withdraw');
      action.addEventListener('click', async () => {
        const r = await deps.send({ type: 'withdraw_listing', listing: id });
        if (r.ok) await refresh();
      });
    } else if (l.auction) {
      const amount = h('input', { class: 'bid-amount', type: 'number', step: 1, value: l.auction.min_bid }) as HTMLInputElement;
      const bid = h('button', { class: 'bid' }, 'Bid');
      amount.addEventListener('input', () => { const c = current(id); if (c) checkBid(bid, amount, c); });
      bid.addEventListener('click', async () => {
        const mine = chosen();
        if (!mine) return;
        const value = Math.floor(Number(amount.value));
        const r = await deps.send({ type: 'bid', listing: id, amount: value, mine: mine.id });
        if (r.ok) {
          model.toast('info', `Bid ${value} renown on auction #${id} for #${mine.id} (held in escrow).`);
          await refresh();
        }
      });
      action = h('span', { class: 'bid-form' }, amount, bid);
    } else {
      action = h('button', { class: 'buy' });
      action.addEventListener('click', async () => {
        const mine = chosen();
        const c = current(id);
        if (!mine || !c) return;
        const r = await deps.send({ type: 'buy_listing', listing: id, mine: mine.id });
        if (r.ok) {
          model.toast('info', `Bought a mating of #${mine.id} with ${c.owner_name}'s #${c.organism}: escort it from the Mates tab.`);
          await refresh();
        }
      });
    }
    return h('div', { class: 'listing', 'data-listing': id, 'data-organism': l.organism, 'data-kind': l.kind },
      h('div', { class: 'listing-head' },
        h('strong', {}, `${l.role === 'stud' ? 'Stud' : 'Dam'} #${l.organism}`),
        h('span', { class: 'pregnant-tag', hidden: true }, ' · pregnant: buyers wait until she has given birth'),
        h('span', { class: 'muted' }, ` · ${l.owner_name}'s ${l.lineage_name}`),
        h('span', { class: 'fee' })),
      l.auction && h('div', { class: 'small auction-line' }),
      h('div', { class: 'small traits-line' }),
      h('div', { class: 'small muted info-line' }),
      l.kind !== 'auction' && h('div', { class: 'small expires-line' }),
      h('div', { class: 'trait-bars' }),
      h('div', { class: 'small compat', hidden: true }),
      !isOwn && h('div', { class: 'small compat sterile', 'data-level': 'bad', hidden: true }, 'Sterile with your lineage: body plans too different.'),
      h('div', { class: 'row' },
        action,
        h('button', { class: 'secondary', onclick: () => { const c = current(id) ?? l; deps.onOrganism(c.organism, c.x, c.y); } }, 'Show'),
        // Why Buy / Bid is off, in words next to it (not only in a hover title).
        !isOwn && h('span', { class: 'small why-not', hidden: true })));
  }

  function checkBid(bid: HTMLElement, amount: HTMLInputElement, l: ListingView): void {
    const s = model.state;
    const why = bidProblem(l, model.player, s?.balance ?? market?.balance ?? 0, s?.slots_left ?? 0, chosen(), Math.floor(Number(amount.value)));
    (bid as HTMLButtonElement).disabled = why !== null;
    bid.title = why ?? `Bid ${amount.value} renown (held in escrow, refunded if outbid)`;
  }

  /** Everything on a card that can change, set in place. */
  function updateCard(card: HTMLElement, l: ListingView, isOwn: boolean): void {
    const s = model.state;
    const tick = s?.clock.tick ?? 0;
    const ms = deps.msPerTick?.();
    const mine = chosen();
    const balance = s?.balance ?? market?.balance ?? 0;
    const slots = s?.slots_left ?? 0;
    const a = l.auction;
    card.classList.toggle('busy', l.busy);
    const preg = card.querySelector<HTMLElement>('.pregnant-tag');
    if (preg) preg.hidden = !(l.pregnant && l.sex === 'female');
    card.classList.toggle('auction', !!a);
    setText(card.querySelector('.fee')!, a ? `${a.high_bid ?? a.reserve} renown` : `${l.fee} renown`);
    const auctionLine = card.querySelector('.auction-line');
    if (auctionLine) setText(auctionLine, auctionText(l, model.player, tick, TICKS_PER_DAY, ms));
    setText(card.querySelector('.traits-line')!, traitSummary(l.traits));
    const parents = l.mother !== undefined || l.father !== undefined ? `of #${l.mother ?? '?'} × #${l.father ?? '?'}` : 'a founder';
    setText(card.querySelector('.info-line')!, `gen ${l.generation}, ${parents} · ${l.age_days.toFixed(0)} days · health ${l.health.toFixed(0)} · raised ${l.offspring_raised}`
      + ` · at ${l.x.toFixed(0)}, ${l.y.toFixed(0)}${l.sales ? ` · ${l.sales} sold` : ''}${l.busy ? ' · promised' : ''}${l.pregnant ? ' · pregnant' : ''}`);
    const expires = card.querySelector('.expires-line');
    if (expires) setText(expires, expiryText(l, tick, TICKS_PER_DAY, ms) ?? '');
    const bars = card.querySelector<HTMLElement>('.trait-bars')!;
    const top = l.traits.filter((t) => t.value >= 0.5 || t.gene === 'body_size').slice(0, 6);
    reconcile(bars, top, (t) => t.gene, (t) => h('span', { class: 'trait' }, traitName(t.gene), h('span', { class: 'trait-bar' }, h('span', {}))), (el, t) => {
      el.title = `${traitName(t.gene)} ${t.value.toFixed(2)}`;
      (el.querySelector('.trait-bar > span') as HTMLElement).style.width = pct(Math.max(0, Math.min(1, t.value)), 0);
    });
    const compatEl2 = card.querySelector<HTMLElement>('.compat:not(.sterile)')!;
    const c = listingCompat(l);
    compatEl2.hidden = !c;
    if (c) {
      setText(compatEl2, c.text);
      compatEl2.dataset.level = c.level;
    }
    const sterile = card.querySelector<HTMLElement>('.sterile');
    if (sterile) {
      sterile.hidden = l.compatible !== false;
      const f = l.body_plan_fertility;
      setText(sterile, `Sterile with your lineage: body plans too different${f !== undefined ? ` (mean fertility ${Math.round(f * 100)}% over sampled pairs; warm and cold blood, diet, birth mode or size apart)` : ''}.`);
    }
    // A sterile listing offers no Buy or Bid at all, only the reason.
    for (const b of card.querySelectorAll<HTMLElement>('.buy, .bid, .bid-amount')) b.hidden = l.compatible === false;
    let problem: string | null = null;
    if (isOwn) {
      const btn = card.querySelector<HTMLButtonElement>('.withdraw')!;
      const hasBids = a?.high_bid !== undefined;
      btn.disabled = hasBids;
      btn.title = hasBids ? 'An auction with a bid runs to its close' : 'Take it off the market';
    } else if (a) {
      const amount = card.querySelector<HTMLInputElement>('.bid-amount')!;
      amount.min = String(a.min_bid);
      if (document.activeElement !== amount && Number(amount.value) < a.min_bid) amount.value = String(a.min_bid);
      checkBid(card.querySelector('.bid')!, amount, l);
      problem = bidProblem(l, model.player, balance, slots, mine, a.min_bid);
    } else {
      const buy = card.querySelector<HTMLButtonElement>('.buy')!;
      problem = buyProblem(l, model.player, balance, slots, mine);
      buy.disabled = problem !== null;
      buy.title = problem ?? `Buy one mating for ${l.fee} renown`;
      setText(buy, `Buy for ${l.fee}`);
    }
    const whyNot = card.querySelector<HTMLElement>('.why-not');
    if (whyNot) {
      whyNot.hidden = !problem;
      if (problem) setText(whyNot, `Can't ${a ? 'bid' : 'buy'}: ${problem === 'pick one of your adults' ? 'pick your adult in "For my adult" above' : problem}`);
    }
  }

  /** Cards kept by listing id and updated in place: a Buy or Bid button is never replaced under a click. */
  function renderCards(root: HTMLElement, ls: ListingView[], isOwn: boolean, empty: string): void {
    for (const l of ls) latest.set(l.id, l);
    const placeholder = root.previousElementSibling as HTMLElement | null;
    const note = market === null && !isOwn ? 'Loading…' : empty;
    if (placeholder?.classList.contains('listings-none')) {
      placeholder.hidden = ls.length > 0 && market !== null;
      setText(placeholder, note);
    } else {
      root.before(h('p', { class: 'muted small listings-none', hidden: ls.length > 0 && market !== null }, note));
    }
    reconcile(root, ls, (l) => `${l.id}:${l.kind}`, (l) => makeCard(l, isOwn), (card, l) => updateCard(card, l, isOwn));
  }

  function renderListings(): void {
    const shown = mergePages(pages);
    // Your own listings never show among the ones to buy: say where they are.
    const ownNote = own.length > 0 ? ` Your own ${own.length} listing${own.length === 1 ? ' is' : 's are'} under "My listings" below.` : '';
    renderCards(listingsEl, market === null ? [] : shown, false, `No listings match the filters.${ownNote}`);
    renderCards(myListingsEl, own, true, 'None.');
    pageEl.textContent = pageText(shown.length, market);
    moreBtn.hidden = !market?.next_cursor;
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
      h('p', { class: 'muted small' }, `Recent: earned ${t.earned} (royalties ${t.royalties}), paid into escrow ${t.spent}, sales ${t.sold}, refunds ${t.refunded}, spent in the shop ${t.shop}.`),
      rows.length === 0 ? h('p', { class: 'muted small' }, 'No entries yet.')
        : h('table', { class: 'board ledger' },
          h('thead', {}, h('tr', {}, h('th', {}, 'Day'), h('th', {}, ''), h('th', {}, 'Renown'), h('th', {}, 'What'), h('th', {}, 'Balance'))),
          h('tbody', {}, ...rows.map((r) => h('tr', { 'data-seq': r.seq, class: `ledger-${r.kind}` },
            h('td', {}, String(r.day + 1)), h('td', {}, h('span', { class: 'feed-kind' }, r.kind)),
            h('td', { class: r.positive ? 'good' : 'bad' }, r.amount), h('td', { class: 'small' }, r.text), h('td', {}, String(r.balance)))))));
  }

  // The renown shop: built once, then updated in place (the balance changes
  // daily, and a rebuild replaced the buttons under a click).
  const shopText_ = h('p', { class: 'muted small' });
  const slotBtn = h('button', { id: 'shop-slot', class: 'secondary' }) as HTMLButtonElement;
  const slotNote = h('span', { class: 'muted small' });
  const reinBtn = h('button', { id: 'shop-reinforce', class: 'secondary' }) as HTMLButtonElement;
  const reinNote = h('span', { class: 'muted small' });
  slotBtn.addEventListener('click', async () => {
    const r = await deps.send({ type: 'buy_mating_slot' });
    if (r.ok) {
      shop = parseShopView(r.data) ?? shop;
      model.toast('info', 'One more directed-mating slot this season.');
      render();
    }
  });
  reinBtn.addEventListener('click', async () => {
    const s = shop;
    if (s && !window.confirm(`Buy reinforcements for ${s.reinforce_price} renown? ${s.reinforce_size} young adults copied from your own join your lineage. The price doubles with each purchase, at most ${s.reinforcements_max ?? 2} within a ranked season's length.`)) return;
    const r = await deps.send({ type: 'reinforce' });
    if (r.ok) {
      const d = r.data as { added?: unknown[]; shop?: unknown; at?: [number, number]; group?: number } | undefined;
      shop = parseShopView(d?.shop) ?? shop;
      const where = d?.at ? ` at (${Math.round(d.at[0])}, ${Math.round(d.at[1])}), beside ${d.group ?? 0} of your animals` : '';
      model.toast('info', `${d?.added?.length ?? 0} adults joined your lineage${where}.`);
      render();
    }
  });
  // A provision (a sink for small lineages): feeds and waters the neediest adults.
  const provBtn = h('button', { id: 'shop-provision', class: 'secondary' }) as HTMLButtonElement;
  const provNote = h('span', { class: 'muted small' });
  provBtn.addEventListener('click', async () => {
    // Quote from a fresh shop: the lineage may have grown since it was shown (playtest 6: quoted 120, fed 140).
    const fresh = await deps.send({ type: 'renown_shop' });
    shop = (fresh.ok && parseShopView(fresh.data)) || shop;
    const s = shop;
    if (s && !window.confirm(`Buy a provision for ${s.provision_price} renown? Your lineage's ${s.provision_size ?? 20} hungriest or thirstiest adults get 50 off their hunger and thirst, once.`)) return;
    const r = await deps.send({ type: 'provision' });
    if (r.ok) {
      const d = r.data as { fed?: number; shop?: unknown } | undefined;
      shop = parseShopView(d?.shop) ?? shop;
      model.toast('info', `${d?.fed ?? 0} adults fed and watered.`);
      render();
    }
  });
  // A survey (a sink that scales): rival groups near your main group, now.
  const surveyBtn = h('button', { id: 'shop-survey', class: 'secondary' }) as HTMLButtonElement;
  const surveyNote = h('span', { class: 'muted small' }, 'Where rivals\' groups and territories are within 64 tiles of your main group, roughly, right now.');
  const surveyOut = h('ul', { id: 'shop-survey-result', class: 'small', hidden: true });
  surveyBtn.addEventListener('click', async () => {
    const s = shop;
    if (s && !window.confirm(`Survey for ${s.survey_price ?? '?'} renown? You see where rivals' groups and territories near your main group are now (to the nearest 8 tiles).`)) return;
    const r = await deps.send({ type: 'survey' });
    if (!r.ok) return;
    const v = r.data as SurveyView;
    shop = parseShopView(v.shop) ?? shop;
    surveyOut.hidden = false;
    surveyOut.replaceChildren(...(v.groups.length
      ? v.groups.map((g) => h('li', {}, `${g.name}: about ${g.members} animals near ${g.x}, ${g.y}${g.territory ? ` · territory at ${g.territory[0]}, ${g.territory[1]} (radius ${g.territory[2]})` : ''}`))
      : [h('li', { class: 'muted' }, `No rival groups within ${v.radius} tiles of ${v.x}, ${v.y}.`)]));
    render();
  });
  // The cosmetic nameplate (a sink for leaders): built once, updated in place.
  const plateColor = h('select', { id: 'shop-nameplate-color' }) as HTMLSelectElement;
  const plateTitle = h('input', { id: 'shop-nameplate-title', maxlength: 24, placeholder: 'Title' }) as HTMLInputElement;
  const plateBtn = h('button', { id: 'shop-nameplate', class: 'secondary' }) as HTMLButtonElement;
  const plateNote = h('span', { class: 'muted small' });
  const plateRow = h('div', { class: 'row', hidden: true }, plateColor, plateTitle, plateBtn, plateNote);
  plateBtn.addEventListener('click', async () => {
    const s = shop;
    const title = plateTitle.value.trim();
    if (s && !window.confirm(`${s.nameplate ? 'Change' : 'Buy'} your nameplate for ${s.nameplate_price ?? '?'} renown? ${plateColor.value}${title ? `, "${title}"` : ', no title'}: it is cosmetic, shown with your name.`)) return;
    const r = await deps.send({ type: 'set_nameplate', color: plateColor.value, title: plateTitle.value });
    if (r.ok) {
      const d = r.data as { shop?: unknown } | undefined;
      shop = parseShopView(d?.shop) ?? shop;
      model.toast('info', 'Nameplate set: it shows next to your name in Ranks, the hall of fame, the leaderboard and on your animals.');
      deps.onNameplate?.();
      render();
    }
  });
  function renderNameplate(s: ShopView): void {
    plateRow.hidden = !s.nameplate_colors;
    if (!s.nameplate_colors) return;
    const colors = s.nameplate_colors.join(',');
    if (plateColor.dataset.colors !== colors) {
      plateColor.dataset.colors = colors;
      plateColor.replaceChildren(...s.nameplate_colors.map((c) => h('option', { value: c }, c)));
      if (s.nameplate) plateColor.value = s.nameplate.color;
      if (s.nameplate && document.activeElement !== plateTitle) plateTitle.value = s.nameplate.title;
    }
    plateBtn.disabled = s.nameplate_price === undefined || s.balance < (s.nameplate_price ?? 0);
    setText(plateBtn, `${s.nameplate ? 'Change' : 'Buy'} nameplate: ${s.nameplate_price ?? '?'} renown`);
    setText(plateNote, `Cosmetic: shown with your name.${s.species_name_price ? ` Naming a species your lineage founded costs ${s.species_name_price}.` : ''}`);
  }
  const provRow = h('div', { class: 'row', hidden: true }, provBtn, provNote);
  const surveyRow = h('div', { hidden: true }, h('div', { class: 'row' }, surveyBtn, surveyNote), surveyOut);
  const shopBody = h('div', { hidden: true }, shopText_, h('div', { class: 'row' }, slotBtn, slotNote), h('div', { class: 'row' }, reinBtn, reinNote), provRow, surveyRow, plateRow);
  const shopLoading = h('p', { class: 'muted small' }, 'Loading…');
  shopEl.replaceChildren(shopLoading, shopBody);

  function renderShop(): void {
    shopLoading.hidden = !!shop;
    shopBody.hidden = !shop;
    if (!shop) return;
    const s = shop;
    setText(shopText_, shopText(s));
    slotBtn.disabled = s.slot_price === undefined || s.balance < s.slot_price;
    setText(slotBtn, s.slot_price === undefined ? 'Slot limit reached' : `Extra mating slot: ${s.slot_price} renown`);
    setText(slotNote, `${s.slots_bought} bought this season; the price doubles with each.`);
    const blocked = s.reinforce_blocked;
    reinBtn.disabled = blocked !== undefined || s.balance < s.reinforce_price;
    reinBtn.title = blocked ?? `${s.reinforce_size} adults copied from your own join your lineage`;
    setText(reinBtn, `Reinforcements (${s.reinforce_size} adults): ${s.reinforce_price} renown`);
    setText(reinNote, blocked ?? `For lineages of at most ${s.reinforce_max_alive}; the price doubles with each, at most ${s.reinforcements_max ?? '?'} a ranked season (${s.reinforcements_bought ?? 0} bought).`);
    provRow.hidden = s.provision_price === undefined;
    if (s.provision_price !== undefined) {
      provBtn.disabled = s.provision_blocked !== undefined || s.balance < s.provision_price;
      const n = s.provision_size ?? 20;
      provBtn.title = s.provision_blocked ?? `The ${n} neediest adults get 50 off hunger and thirst`;
      setText(provBtn, `Provision (${n} adults fed): ${s.provision_price} renown`);
      setText(provNote, s.provision_blocked ?? `Priced per adult fed (twice as much above ${s.reinforce_max_alive} alive); at most ${s.provisions_max ?? '?'} a ranked season (${s.provisions_bought ?? 0} bought).`);
    }
    surveyRow.hidden = s.survey_price === undefined;
    if (s.survey_price !== undefined) {
      surveyBtn.disabled = s.balance < s.survey_price;
      setText(surveyBtn, `Survey nearby rivals: ${s.survey_price} renown`);
    }
    renderNameplate(s);
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
    const offerBtn = (r: CompatibilityView['rows'][number]) => {
      const b = h('button', { class: 'secondary offer-lineage', title: "Offer a mating with one of their animals: the owner picks one that's free" }, 'Offer a mating');
      b.addEventListener('click', async () => {
        const mine = Number(mineSel.value);
        if (!mine) return model.toast('error', 'Pick one of your adults under "For my adult" first.');
        const price = Math.max(0, Math.floor(Number(offerPrice.value) || 0));
        const res = await deps.send({ type: 'offer_to_lineage', mine, lineage: r.lineage, ...(price > 0 ? { price } : {}) });
        if (res.ok) model.toast('info', `Offered #${mine} to ${r.owner_name}'s ${r.lineage_name}${price ? ` for ${price} renown` : ''}: they pick the partner.`);
      });
      return b;
    };
    compatEl.replaceChildren(
      h('p', { class: 'muted small' }, 'Lineages whose diet, warmth, birth mode or size differ by 0.30 or more from yours are sterile with it: only the fertile ones are worth trading with. "Offer a mating" lets the owner pick one of their animals that is free (no guessing which females are pregnant).'),
      h('label', {}, 'Price for an offer (renown)', offerPrice),
      h('ul', { class: 'small compat-list' },
        ...fertile.map((r) => h('li', { 'data-lineage': r.lineage, class: 'good' }, compatRowText(r), r.owner !== 0 ? offerBtn(r) : '')),
        ...sterile.slice(0, 6).map((r) => h('li', { 'data-lineage': r.lineage, class: 'muted' }, compatRowText(r)))),
      h('p', { class: 'muted small compat-confidence', id: 'market-compat-note' }, COMPAT_CONFIDENCE),
      ...(fertile.length === 0 ? [h('p', { class: 'small' }, 'None of the sampled pairs can breed with yours right now (animals vary, so a rare pair may still conceive).')] : []));
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
  return { el, refresh, render, shopPrice: (key) => shop?.[key] };
}

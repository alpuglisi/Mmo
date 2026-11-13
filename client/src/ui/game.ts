// The play screen: world view (tile map), top bar (clock, weather,
// connection) and a sidebar with the organism inspector, matings, the
// genetics dashboard with the alerts feed, the lineage/leaderboards tab and
// the world chronicle (species, eras, catastrophes, achievements).

import { api, ApiError } from '../api';
import { alertEntry, type GameModel } from '../model';
import type { ClientMsg, LineageStats, NearbyOrganism, OrgId, OwnOrganism, ResultMsg, SeasonBoard, StateView } from '../protocol';
import { WILD } from '../protocol';
import { WorldView } from '../world/worldView';
import { applyDynamic, decodeTerrain, terrainSummary, type TerrainGrid } from '../world/terrain';
import { cssColor, ownerColor, OWN_COLOR } from '../world/colors';
import { append, clear, h, label, pct } from './dom';
import { renderDashboard, renderFeed } from './dashboard';
import { catastropheBanner, renderWorld, type Chronicle } from './chronicle';

export interface GameDeps {
  model: GameModel;
  send: (m: ClientMsg) => Promise<ResultMsg>;
  onLogout: () => void;
  /** The session token (for the authenticated dashboard endpoint). */
  token: string;
  /** The server refused the token (session expired). */
  onUnauthorized: () => void;
  /** Put the screen in the document (the canvas needs a laid-out parent). */
  mountInto: (el: HTMLElement) => void;
}

const LOCAL_CODES = new Set(['not_connected', 'timeout', 'disconnected', 'closed']);
/** The dynamic terrain layers change every simulated hour. */
const TERRAIN_REFRESH_MS = 4000;
const STATS_REFRESH_MS = 5000;
const SEASONS_REFRESH_MS = 15000;
const CHRONICLE_REFRESH_MS = 10000;

type Tab = 'organism' | 'matings' | 'genetics' | 'lineage' | 'world';
const TAB_LABELS: Record<Tab, string> = { organism: 'Organism', matings: 'Matings', genetics: 'Genetics', lineage: 'Lineage', world: 'World' };
type Mode = 'select' | 'migrate';

export interface GameScreen {
  el: HTMLElement;
  /** Exposed for the end-to-end test and debugging (`window.evo`). */
  view: WorldView;
  destroy: () => void;
}

export async function gameScreen(deps: GameDeps): Promise<GameScreen> {
  const { model } = deps;
  const cleanups: (() => void)[] = [];

  const send = async (m: ClientMsg): Promise<ResultMsg> => {
    const r = await deps.send(m);
    if (!r.ok && r.error && LOCAL_CODES.has(r.error.code)) model.toast('error', `${m.type}: ${r.error.message}`);
    return r;
  };

  // ---------------------------------------------------------------- layout
  const connPill = h('span', { class: 'pill', id: 'conn-state' });
  const clockEl = h('span', { id: 'clock' });
  const weatherEl = h('span', { class: 'muted', id: 'weather' });
  const bannerEl = h('span', { class: 'catastrophe-pill', id: 'catastrophe-banner', hidden: true });
  const top = h('header', { class: 'topbar' },
    h('strong', {}, 'Evolution MMO'), h('span', { class: 'muted' }, model.name), clockEl, weatherEl, bannerEl,
    h('span', { class: 'spacer' }), connPill,
    h('button', { class: 'secondary', id: 'logout', onclick: deps.onLogout }, 'Log out'));

  const stage = h('div', { class: 'stage', id: 'stage' });
  const followBtn = h('button', { class: 'tool', id: 'follow', title: 'Keep the camera on the view window (your possessed organism or your lineage)' }, 'Follow');
  const migrateBtn = h('button', { class: 'tool', id: 'migrate', title: 'Click the map to send your adults there' }, 'Migrate…');
  const clearMigBtn = h('button', { class: 'tool', id: 'clear-migration' }, 'Clear migration');
  const pinBtn = h('button', { class: 'tool', id: 'pin-view', title: 'Pin the server view window (what you can see of others) to the camera center' }, 'Pin view here');
  const hint = h('div', { class: 'hint', id: 'hint' });
  const toasts = h('div', { class: 'toasts', id: 'toasts', 'aria-live': 'polite' });
  const toolbar = h('div', { class: 'toolbar' },
    followBtn,
    h('button', { class: 'tool', title: 'Zoom in', onclick: () => view.zoomBy(1.4) }, '+'),
    h('button', { class: 'tool', title: 'Zoom out', onclick: () => view.zoomBy(1 / 1.4) }, '−'),
    migrateBtn, clearMigBtn, pinBtn);
  const legend = h('div', { class: 'legend' },
    h('span', {}, h('i', { class: 'dot', style: `background:${cssColor(OWN_COLOR)}` }), 'yours'),
    h('span', {}, h('i', { class: 'dot ring', style: 'border-color:#ff7b72' }), 'possessed'),
    h('span', {}, h('i', { class: 'dot ring', style: 'border-color:#7ee787' }), 'pregnant'),
    h('span', {}, h('i', { class: 'dot', style: `background:${cssColor(ownerColor(WILD))}` }), 'wild'),
    h('span', {}, h('i', { class: 'dot', style: `background:${cssColor(ownerColor(1))}` }), 'other players'),
    h('span', {}, h('i', { class: 'dot square', style: 'border-color:#58a6ff' }), 'view window'));
  const terrainLegend = h('div', { class: 'legend terrain-legend', id: 'terrain-legend' });
  const stageWrap = h('div', { class: 'stage-wrap' }, stage, toolbar, hint, legend, terrainLegend, toasts);

  const tabs: Record<Tab, HTMLElement> = {
    organism: h('div', { class: 'panel', id: 'panel-organism' }),
    matings: h('div', { class: 'panel', id: 'panel-matings' }),
    genetics: h('div', { class: 'panel', id: 'panel-genetics' }),
    lineage: h('div', { class: 'panel', id: 'panel-lineage' }),
    world: h('div', { class: 'panel', id: 'panel-world' }),
  };
  const tabBtns = (Object.keys(tabs) as Tab[]).map((t) =>
    h('button', { class: 'tab', 'data-tab': t, id: `tab-${t}`, onclick: () => showTab(t) }, TAB_LABELS[t]));
  const alertBadge = h('span', { class: 'badge', id: 'alert-badge', hidden: true });
  tabBtns[2].append(alertBadge);
  let seenFeed = 0;
  const sidebar = h('aside', { class: 'sidebar' }, h('nav', { class: 'tabs' }, ...tabBtns), ...Object.values(tabs));
  const el = h('div', { class: 'screen game' }, top, h('div', { class: 'game-body' }, stageWrap, sidebar));

  let tab: Tab = 'lineage';
  function showTab(t: Tab): void {
    tab = t;
    for (const b of tabBtns) b.classList.toggle('active', b.dataset.tab === t);
    for (const [k, p] of Object.entries(tabs)) p.hidden = k !== t;
    if (t === 'genetics') void refreshStats();
    if (t === 'lineage') void refreshSeasons();
    if (t === 'world') void refreshChronicle();
    renderPanels(true);
  }

  // ---------------------------------------------------------------- world view
  const view = new WorldView();
  deps.mountInto(el);
  await new Promise((r) => requestAnimationFrame(r));
  await view.init(stage);

  let mode: Mode = 'select';
  function setMode(m: Mode): void {
    mode = m;
    migrateBtn.classList.toggle('active', m === 'migrate');
    updateHint();
  }
  function possessed(): OwnOrganism | undefined {
    return model.state?.organisms.find((o) => o.possessed);
  }
  function updateHint(): void {
    const p = possessed();
    hint.textContent = mode === 'migrate'
      ? 'Click the map to set the migration target (Esc cancels).'
      : p ? `Possessing #${p.id}: click the map to walk there.` : 'Click an organism to inspect it. Drag to pan, wheel to zoom.';
  }

  view.onFollowChange = (f) => followBtn.classList.toggle('active', f);
  followBtn.classList.add('active');
  followBtn.addEventListener('click', () => view.setFollow(!view.follow));
  migrateBtn.addEventListener('click', () => setMode(mode === 'migrate' ? 'select' : 'migrate'));
  clearMigBtn.addEventListener('click', () => void send({ type: 'clear_migration' }));
  let pinned = false;
  pinBtn.addEventListener('click', async () => {
    const r = pinned ? await send({ type: 'clear_view' }) : await send({ type: 'set_view', x: view.cam.x, y: view.cam.y, radius: 24 });
    if (r.ok) {
      pinned = !pinned;
      pinBtn.textContent = pinned ? 'Unpin view' : 'Pin view here';
      pinBtn.classList.toggle('active', pinned);
    }
  });
  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'Escape') setMode('select');
  };
  window.addEventListener('keydown', onKey);
  cleanups.push(() => window.removeEventListener('keydown', onKey));

  view.onClick = async (c) => {
    if (mode === 'migrate') {
      setMode('select');
      const r = await send({ type: 'migrate', x: c.x, y: c.y });
      // The server clamps the target to the map and echoes it.
      const t = (r.data as { x: number; y: number } | undefined) ?? c;
      if (r.ok) model.toast('info', `Adults migrating toward (${t.x.toFixed(0)}, ${t.y.toFixed(0)})`);
      return;
    }
    if (c.organism !== null) {
      model.select(c.organism);
      showTab('organism');
      return;
    }
    const p = possessed();
    if (p) {
      view.setMoveTarget({ x: c.x, y: c.y });
      await send({ type: 'order_move', organism: p.id, x: c.x, y: c.y });
    } else {
      model.select(null);
    }
  };

  // Tile map: the whole grid once, then the dynamic layers periodically.
  let terrain: TerrainGrid | null = null;
  function showTerrain(g: TerrainGrid): void {
    view.setTerrain(g);
    const t = terrainSummary(g);
    const chip = (color: string, text: string) => h('span', {}, h('i', { class: 'dot square-fill', style: `background:${color}` }), text);
    terrainLegend.replaceChildren(
      chip('#4682e1', 'water'), chip('#467ccf', 'river'),
      chip('#c4aa68', `drought ${pct(t.drought, 0)}`), chip('#486c96', `flooded ${pct(t.flood, 1)}`),
      chip('#f5f7fa', `snow ${pct(t.snow, 0)}`), chip('#c4dcee', `ice ${pct(t.ice, 0)}`),
      h('span', { class: 'muted' }, `${g.season}, terrain at ${String(g.hour).padStart(2, '0')}:00`));
  }
  async function loadTerrain(): Promise<void> {
    try {
      terrain = decodeTerrain(await api.terrain());
      showTerrain(terrain);
    } catch {
      /* try again on the next refresh */
    }
  }
  async function refreshTerrain(): Promise<void> {
    if (!terrain) return loadTerrain();
    try {
      const before = terrain.tick;
      applyDynamic(terrain, await api.terrainDynamic());
      if (terrain.tick !== before) showTerrain(terrain);
    } catch {
      /* try again next time */
    }
  }
  try {
    const w = await api.world();
    view.setWorldSize(w.width, w.height);
    view.centerOn(w.width / 2, w.height / 2);
  } catch {
    /* defaults */
  }
  await loadTerrain();
  const terrainTimer = setInterval(() => void refreshTerrain(), TERRAIN_REFRESH_MS);
  cleanups.push(() => clearInterval(terrainTimer));

  // Genetics dashboard: fetched while its tab is open (and once now, for the feed).
  let stats: LineageStats | null = null;
  async function refreshStats(): Promise<void> {
    try {
      stats = await api.lineageStats(deps.token);
      model.addFeed(stats.alerts.map(alertEntry));
      if (tab === 'genetics') renderGenetics();
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) deps.onUnauthorized();
    }
  }
  void refreshStats();
  const statsTimer = setInterval(() => {
    if (tab === 'genetics') void refreshStats();
  }, STATS_REFRESH_MS);
  cleanups.push(() => clearInterval(statsTimer));

  let seasons: SeasonBoard[] | null = null;
  async function refreshSeasons(): Promise<void> {
    try {
      seasons = await api.seasons(6);
      if (tab === 'lineage') renderSeasons();
    } catch {
      /* next time */
    }
  }
  const seasonsTimer = setInterval(() => {
    if (tab === 'lineage') void refreshSeasons();
  }, SEASONS_REFRESH_MS);
  cleanups.push(() => clearInterval(seasonsTimer));

  // World chronicle: fetched while its tab is open.
  let chronicle: Chronicle | null = null;
  async function refreshChronicle(): Promise<void> {
    try {
      const [species, eras, catastrophes, achievements] = await Promise.all([api.species(), api.eras(), api.catastrophes(), api.achievements()]);
      chronicle = { species, eras, catastrophes, achievements };
      if (tab === 'world') renderWorld(tabs.world, chronicle, model.player, model.state?.clock.day ?? species.day);
    } catch {
      /* next time */
    }
  }
  const chronicleTimer = setInterval(() => {
    if (tab === 'world') void refreshChronicle();
  }, CHRONICLE_REFRESH_MS);
  cleanups.push(() => clearInterval(chronicleTimer));

  // ---------------------------------------------------------------- panels
  let renderQueued = false;
  let lastRender = 0;
  function renderPanels(now = false): void {
    if (now) {
      lastRender = performance.now();
      renderTopbar();
      if (tab === 'organism') renderInspector();
      if (tab === 'matings') renderMatings();
      if (tab === 'lineage') renderLineage();
      if (tab === 'genetics') renderFeedPanel();
      if (tab === 'world' && !chronicle) renderWorld(tabs.world, null, model.player, 0);
      return;
    }
    if (renderQueued) return;
    renderQueued = true;
    // At most 4 panel refreshes a second, so buttons stay clickable.
    const wait = Math.max(0, 250 - (performance.now() - lastRender));
    setTimeout(() => {
      renderQueued = false;
      renderPanels(true);
    }, wait);
  }

  function renderTopbar(): void {
    const s = model.state;
    if (!s) return;
    const c = s.clock;
    clockEl.textContent = `Year ${c.year + 1} · day ${c.day + 1} · ${String(c.hour).padStart(2, '0')}:00 · ${c.season} · ${c.daytime ? 'day' : 'night'}`;
    const w = s.weather;
    weatherEl.textContent = `wind ${pct(w.mean_wind, 0)}${w.storm ? ' · STORM' : ''} · drought ${pct(w.drought, 0)} · flood ${pct(w.flood, 0)}`;
    const banner = catastropheBanner(w.catastrophe, c.day);
    bannerEl.hidden = banner === null;
    bannerEl.textContent = banner ?? '';
  }

  function renderConn(): void {
    const s = model.conn;
    connPill.className = `pill conn-${s}`;
    connPill.textContent = s === 'open' ? 'connected'
      : s === 'reconnecting' ? `reconnecting${model.retryInMs ? ` in ${(model.retryInMs / 1000).toFixed(1)} s` : '…'}`
      : s === 'connecting' ? 'connecting…' : 'offline';
  }

  // Rebuild a block only when its key changes, so buttons aren't replaced
  // under the cursor on every state push.
  const keys = new WeakMap<HTMLElement, string>();
  function keyed(container: HTMLElement, key: string, build: () => (Node | string | null | false)[]): void {
    if (keys.get(container) === key) return;
    keys.set(container, key);
    clear(container);
    append(container, build());
  }

  // --- Organism inspector
  const inspDetails = h('div');
  const inspActions = h('div', { class: 'row wrap', id: 'inspector-actions' });
  append(tabs.organism, [inspDetails, inspActions]);

  function findOrg(id: OrgId | null): { own?: OwnOrganism; other?: NearbyOrganism } {
    const s = model.state;
    if (!s || id === null) return {};
    const own = s.organisms.find((o) => o.id === id);
    if (own) return { own };
    const other = s.nearby.find((o) => o.id === id);
    return other ? { other } : {};
  }

  function renderInspector(): void {
    const { own, other } = findOrg(model.selected);
    view.selected = model.selected;
    if (!own && !other) {
      keyed(inspActions, 'none', () => []);
      inspDetails.replaceChildren(h('p', { class: 'muted' }, model.selected === null
        ? 'Click an organism on the map, or pick one from the Lineage tab.'
        : `#${model.selected} is out of sight (or dead).`));
      return;
    }
    if (own) {
      const n = own.needs;
      inspDetails.replaceChildren(
        h('h3', {}, h('span', { class: 'swatch', style: `background:${cssColor(OWN_COLOR)}` }), `#${own.id} · yours`),
        dl([
          ['Action', label(own.action)], ['Sex', own.sex], ['Age', `${own.age_days.toFixed(1)} days`],
          ['Stage', own.matured ? 'adult' : `juvenile (growth ${pct(own.growth, 0)})`],
          ['Health', meter(own.health, 100, 'good')], ['Lineage', `${own.lineage} · generation ${own.generation}`],
          ['Position', `${own.x.toFixed(1)}, ${own.y.toFixed(1)}`],
          ['Status', [own.possessed && 'possessed', own.pregnant && 'pregnant'].filter(Boolean).join(', ') || '—'],
        ]),
        h('h4', {}, 'Needs'),
        dl([['Hunger', meter(n.hunger, 100, 'bad')], ['Thirst', meter(n.thirst, 100, 'bad')], ['Energy', meter(n.energy, 100, 'good')],
          ['Threat', meter(n.safety, 100, 'bad')], ['Too cold', meter(n.cold, 20, 'bad', '°')], ['Too hot', meter(n.heat, 20, 'bad', '°')],
          ['Breeding urge', meter(n.breeding, 100, 'neutral')], ['Social', meter(n.social, 100, 'neutral')]]),
      );
      const other = possessed();
      keyed(inspActions, `own:${own.id}:${own.possessed}:${other?.id ?? ''}`, () => [
        own.possessed
          ? h('button', { id: 'btn-release', onclick: () => void send({ type: 'release', organism: own.id }) }, 'Release')
          : h('button', { id: 'btn-possess', onclick: async () => {
              const r = await send({ type: 'possess', organism: own.id });
              if (r.ok) {
                view.setFollow(true);
                model.toast('info', `Possessing #${own.id}. Click the map to walk.`);
              }
            } }, other ? `Possess (releases #${other.id})` : 'Possess'),
        h('button', { class: 'secondary', onclick: () => { view.setFollow(false); view.centerOn(own.x, own.y); } }, 'Center'),
        own.matured && h('button', { class: 'secondary', onclick: () => { matingForm.setMine(own.id); showTab('matings'); } }, 'Mate this one…'),
      ]);
      return;
    }
    const o = other!;
    const wild = o.owner === WILD;
    const ownerName = wild ? 'wild' : model.state?.leaderboard.rows.find((r) => r.player === o.owner)?.name ?? `player ${o.owner}`;
    inspDetails.replaceChildren(
      h('h3', {}, h('span', { class: 'swatch', style: `background:${cssColor(ownerColor(o.owner))}` }), `#${o.id} · ${ownerName}`),
      dl([['Action', label(o.action)], ['Sex', o.sex], ['Stage', o.matured ? 'adult' : 'juvenile'], ['Lineage', String(o.lineage)],
        ['Position', `${o.x.toFixed(1)}, ${o.y.toFixed(1)}`]]),
      h('p', { class: 'muted small' }, 'Only what can be seen: needs and health of other players\' organisms are hidden.'),
    );
    keyed(inspActions, `other:${o.id}:${wild}`, () => [
      wild ? h('p', { class: 'muted small' }, 'Wild organisms have no owner to consent to a mating.')
        : h('button', { id: 'btn-ask-mating', onclick: () => { matingForm.setPartner(o.id); showTab('matings'); } }, 'Request mating…'),
      h('button', { class: 'secondary', onclick: () => { view.setFollow(false); view.centerOn(o.x, o.y); } }, 'Center'),
    ]);
  }

  // --- Matings
  const matingForm = buildMatingForm();
  const incomingEl = h('div', { id: 'incoming' });
  const openEl = h('div', { id: 'open-offers' });
  const closedEl = h('div', { id: 'closed-offers' });
  append(tabs.matings, [
    h('h3', {}, 'Request a directed mating'), matingForm.el,
    h('h3', {}, 'Incoming requests'), incomingEl,
    h('h3', {}, 'My open offers'), openEl,
    h('h3', {}, 'Recent outcomes'), closedEl,
  ]);

  function buildMatingForm() {
    const mine = h('select', { id: 'mating-mine' });
    const partner = h('input', { id: 'mating-partner', type: 'number', min: 1, placeholder: 'organism id' });
    const price = h('input', { id: 'mating-price', type: 'number', min: 0, placeholder: 'optional' });
    const submit = h('button', { id: 'mating-submit' }, 'Send request');
    const slots = h('p', { class: 'muted small' });
    let optionKey = '';
    let wantMine: OrgId | null = null;
    submit.addEventListener('click', async () => {
      const m = Number(mine.value);
      const p = Number(partner.value);
      if (!m || !p) return model.toast('error', 'Pick one of your adults and a partner id.');
      const msg: ClientMsg = { type: 'request_mating', mine: m, partner: p };
      if (price.value !== '') msg.price = Math.max(0, Math.floor(Number(price.value)));
      const r = await send(msg);
      if (r.ok) {
        const d = r.data as { offer?: { id: number } } | undefined;
        model.toast('info', d?.offer ? `Offer ${d.offer.id} sent; waiting for consent.` : 'Mating done within your lineage.');
      }
    });
    function refresh(s: StateView): void {
      const adults = s.organisms.filter((o) => o.matured).sort((a, b) => a.id - b.id);
      const key = adults.map((o) => `${o.id}${o.pregnant ? 'p' : ''}`).join(',');
      if (key !== optionKey) {
        optionKey = key;
        const keep = wantMine ?? Number(mine.value);
        mine.replaceChildren(...adults.map((o) => h('option', { value: o.id }, `#${o.id} ${o.sex}${o.pregnant ? ' (pregnant)' : ''} · gen ${o.generation}`)));
        if (adults.some((o) => o.id === keep)) mine.value = String(keep);
        wantMine = null;
      }
      slots.textContent = `${s.slots_left} directed-mating slot${s.slots_left === 1 ? '' : 's'} left this season.`;
    }
    const el = h('div', { class: 'form' },
      h('label', {}, 'My adult', mine),
      h('div', { class: 'row' }, h('label', {}, 'Partner organism', partner), h('label', {}, 'Price', price)),
      h('div', { class: 'row' }, submit), slots,
      h('p', { class: 'muted small' }, 'Pick a partner on the map and press "Request mating…". Pairs within your own lineage run at once; another player must consent, then the two must meet.'));
    return {
      el,
      refresh,
      setMine: (id: OrgId) => { wantMine = id; mine.value = String(id); optionKey = ''; },
      setPartner: (id: OrgId) => { partner.value = String(id); },
    };
  }

  function offerLine(o: { id: number; mine: OrgId; partner: OrgId; from_name: string; to_name: string; price?: number; status: string }): string {
    return `#${o.id}: ${o.from_name}'s #${o.mine} × ${o.to_name}'s #${o.partner}${o.price !== undefined ? ` · price ${o.price}` : ''} · ${o.status}`;
  }

  /** Possess `mine` and walk it to `partner`'s last seen position: the trip to the meeting. */
  async function escort(mine: OrgId, partner: OrgId): Promise<void> {
    const s = model.state;
    const own = s?.organisms.find((o) => o.id === mine);
    if (!s || !own) return model.toast('error', `#${mine} is not alive`);
    if (!own.possessed) {
      const r = await send({ type: 'possess', organism: mine });
      if (!r.ok) return;
    }
    model.select(mine);
    view.setFollow(true);
    const p = s.nearby.find((o) => o.id === partner) ?? s.organisms.find((o) => o.id === partner);
    if (!p) return model.toast('info', `Possessing #${mine}. The partner #${partner} is out of sight: click the map to walk.`);
    view.setMoveTarget({ x: p.x, y: p.y });
    const r = await send({ type: 'order_move', organism: mine, x: p.x, y: p.y });
    if (r.ok) model.toast('info', `#${mine} is walking to #${partner}.`);
  }

  function renderMatings(): void {
    const s = model.state;
    if (s) matingForm.refresh(s);
    const all = [...model.offers.values()].sort((a, b) => b.offer.id - a.offer.id);
    const me = model.player;
    const incoming = all.filter((e) => e.offer.to_player === me && e.offer.status === 'pending');
    const open = all.filter((e) => (e.offer.from_player === me || (e.offer.to_player === me && e.offer.status === 'accepted')) && ['pending', 'accepted'].includes(e.offer.status));
    const closed = all.filter((e) => !['pending', 'accepted'].includes(e.offer.status));
    keyed(incomingEl, incoming.map((e) => `${e.offer.id}`).join(','), () => incoming.length === 0 ? [h('p', { class: 'muted small' }, 'None.')] : incoming.map((e) =>
      h('div', { class: 'offer', 'data-offer': e.offer.id }, h('span', {}, offerLine(e.offer)),
        h('div', { class: 'row' },
          h('button', { class: 'accept', onclick: () => void send({ type: 'respond_mating', request: e.offer.id, accept: true }) }, 'Accept'),
          h('button', { class: 'secondary', onclick: () => void send({ type: 'respond_mating', request: e.offer.id, accept: false }) }, 'Decline')))));
    keyed(openEl, open.map((e) => `${e.offer.id}:${e.offer.status}:${e.error?.code ?? ''}`).join(','), () => open.length === 0 ? [h('p', { class: 'muted small' }, 'None.')] : open.map((e) => {
      const mineId = e.offer.from_player === me ? e.offer.mine : e.offer.partner;
      return h('div', { class: 'offer', 'data-offer': e.offer.id }, h('span', {}, offerLine(e.offer)),
        e.offer.status === 'accepted' && h('small', { class: 'muted' }, `Agreed${e.error ? ` (${e.error.message})` : ''}: escort #${mineId} to the partner; it completes when they meet.`),
        h('div', { class: 'row' },
          e.offer.status === 'accepted' && h('button', { onclick: () => void send({ type: 'complete_mating', request: e.offer.id }) }, 'Complete now'),
          e.offer.status === 'accepted' && h('button', { class: 'secondary', title: 'Possess your organism and walk it to the partner (if the partner is in sight)', onclick: () => void escort(mineId, e.offer.from_player === me ? e.offer.partner : e.offer.mine) }, `Escort #${mineId}`),
          h('button', { class: 'secondary', onclick: () => void send({ type: 'cancel_mating', request: e.offer.id }) }, 'Cancel')));
    }));
    keyed(closedEl, closed.map((e) => `${e.offer.id}:${e.offer.status}`).join(','), () => closed.length === 0 ? [h('p', { class: 'muted small' }, 'None yet.')] : closed.map((e) =>
      h('div', { class: `offer closed status-${e.offer.status}` }, h('span', {}, offerLine(e.offer)),
        e.outcome && h('small', {}, `${e.outcome.conceived ? 'Conceived' : 'Not conceived'} · mismatch ${e.outcome.mismatch.toFixed(2)} · fertility ${pct(e.outcome.fertility, 0)} · failure chance ${pct(e.outcome.failure_chance, 0)}`),
        e.error && h('small', { class: 'error' }, e.error.message))));
  }

  // --- Genetics dashboard and alerts feed
  const dashEl = h('div', { id: 'dashboard' });
  const feedEl = h('div', { id: 'alerts-feed', class: 'feed' });
  append(tabs.genetics, [dashEl, h('h3', {}, 'Alerts'), feedEl]);

  function renderGenetics(): void {
    renderDashboard(dashEl, stats);
    renderFeedPanel();
  }

  function renderFeedPanel(): void {
    seenFeed = model.feed.length;
    alertBadge.hidden = true;
    keyed(feedEl, `${model.feed.length}|${model.feed[model.feed.length - 1]?.text ?? ''}`, () => [renderFeed(model.feed, (id) => {
      model.select(id);
      showTab('organism');
    })]);
  }

  function onAlerts(): void {
    if (tab === 'genetics') return renderFeedPanel();
    const unseen = model.feed.length - seenFeed;
    alertBadge.hidden = unseen <= 0;
    alertBadge.textContent = unseen > 9 ? '9+' : String(unseen);
  }

  // --- Lineage, leaderboards
  const statsEl = h('div', { id: 'lineage-stats' });
  const boardEl = h('div', { id: 'leaderboard' });
  const seasonsEl = h('div', { id: 'season-boards' });
  const listEl = h('div', { id: 'organism-list', class: 'org-list' });
  append(tabs.lineage, [statsEl, h('h3', {}, 'Legacy leaderboard'), boardEl, h('h3', {}, 'Seasonal leaderboards'), seasonsEl,
    h('h3', {}, 'My organisms'), listEl]);

  function renderSeasons(): void {
    const list = seasons;
    keyed(seasonsEl, JSON.stringify(list), () => {
      if (!list) return [h('p', { class: 'muted small' }, 'Loading…')];
      if (list.length === 0) return [h('p', { class: 'muted small' }, 'The first season standings come with the first daily record.')];
      return list.map((b) => h('details', { class: 'season', open: b.current || undefined },
        h('summary', {}, `Year ${b.year + 1} ${b.season}${b.current ? ' (now)' : ''} · days ${b.first_day + 1}–${b.last_day + 1}`),
        h('table', { class: 'board' },
          h('thead', {}, h('tr', {}, h('th', {}, '#'), h('th', {}, 'Player'), h('th', { title: 'Mean legacy share over the recorded days' }, 'Mean'),
            h('th', {}, 'Final'), h('th', {}, 'Peak'), h('th', { title: 'Most organisms alive on a recorded day' }, 'Peak alive'))),
          h('tbody', {}, ...b.rows.slice(0, 8).map((r, k) => h('tr', { class: r.player === model.player ? 'me' : '' },
            h('td', {}, String(k + 1)),
            h('td', {}, h('span', { class: 'swatch', style: `background:${cssColor(r.player === model.player ? OWN_COLOR : ownerColor(r.player))}` }), r.name),
            h('td', {}, pct(r.mean_share)), h('td', {}, pct(r.final_share)), h('td', {}, pct(r.peak_share)), h('td', {}, String(r.peak_alive))))))));
    });
  }

  function renderLineage(): void {
    const s = model.state;
    if (!s) {
      statsEl.replaceChildren(h('p', { class: 'muted' }, 'Waiting for the first state…'));
      return;
    }
    const orgs = s.organisms;
    const adults = orgs.filter((o) => o.matured).length;
    const maxGen = orgs.reduce((m, o) => Math.max(m, o.generation), 0);
    statsEl.replaceChildren(dl([
      ['Lineage', s.lineage === null ? 'not founded' : `#${s.lineage}`],
      ['Alive', h('span', {}, h('strong', { id: 'lineage-size' }, String(s.population)),
        s.population > orgs.length
          ? h('span', { class: 'muted small', title: 'Level of detail: members far from any player run as statistical cohorts and are not drawn' }, ` · ${orgs.length} on the map`)
          : null)],
      ['Adults / young', `${adults} / ${orgs.length - adults}`],
      ['Pregnant', String(orgs.filter((o) => o.pregnant).length)],
      ['Newest generation', String(maxGen)],
      ['Mating slots left', h('span', { id: 'slots-left' }, String(s.slots_left))],
      ['Migration', s.migration ? `toward ${s.migration[0].toFixed(0)}, ${s.migration[1].toFixed(0)}` : 'none'],
    ]));
    const lb = s.leaderboard;
    keyed(boardEl, JSON.stringify(lb), () => lb.rows.length === 0
      ? [h('p', { class: 'muted small' }, lb.day === null ? 'The first legacy snapshot comes at the end of day 1.' : 'No breeding adults.')]
      : [h('p', { class: 'muted small' }, `Day ${(lb.day ?? 0) + 1} snapshot · ${lb.breeding_adults} breeding adults`),
        h('table', { class: 'board' },
          h('thead', {}, h('tr', {}, h('th', {}, 'Player'), h('th', {}, 'Share'), h('th', { title: 'Weighted by offspring raised to adulthood' }, 'Weighted'), h('th', {}, 'Alive'))),
          h('tbody', {}, ...lb.rows.map((r) => h('tr', { class: r.player === model.player ? 'me' : '' },
            h('td', {}, h('span', { class: 'swatch', style: `background:${cssColor(r.player === model.player ? OWN_COLOR : ownerColor(r.player))}` }), r.name),
            h('td', {}, h('span', { class: 'share-bar', style: `--w:${(r.share * 100).toFixed(1)}%` }, pct(r.share))),
            h('td', {}, pct(r.weighted_share)), h('td', {}, String(r.alive))))))]);
    const shown = [...orgs].sort((a, b) => Number(b.possessed) - Number(a.possessed) || Number(b.matured) - Number(a.matured) || a.id - b.id).slice(0, 60);
    keyed(listEl, shown.map((o) => `${o.id}${o.possessed ? 'P' : ''}${o.matured ? 'a' : ''}${o.pregnant ? 'g' : ''}${o.action}`).join(','), () => shown.map((o) =>
      h('button', { class: `org-item${o.possessed ? ' possessed' : ''}`, 'data-org': o.id, onclick: () => {
        model.select(o.id);
        view.setFollow(false);
        view.centerOn(o.x, o.y);
        showTab('organism');
      } }, `#${o.id} ${o.sex === 'female' ? 'F' : 'M'}${o.matured ? '' : ' juv'}${o.pregnant ? ' preg' : ''} · ${label(o.action)}`)));
    if (orgs.length > shown.length) append(listEl, [h('p', { class: 'muted small' }, `…and ${orgs.length - shown.length} more`)]);
  }

  function renderToasts(): void {
    clear(toasts);
    for (const t of model.toasts.slice(-4)) append(toasts, [h('div', { class: `toast ${t.kind}` }, t.text)]);
  }
  let toastTimer: ReturnType<typeof setTimeout> | null = null;

  cleanups.push(
    model.on('state', () => {
      view.setState(model.state!);
      updateHint();
      renderPanels();
    }),
    model.on('select', () => { view.selected = model.selected; renderPanels(); }),
    model.on('offers', () => renderPanels()),
    model.on('conn', renderConn),
    model.on('alerts', onAlerts),
    model.on('toast', () => {
      renderToasts();
      if (toastTimer) clearTimeout(toastTimer);
      toastTimer = setTimeout(() => { model.toasts = []; renderToasts(); }, 6000);
    }),
  );
  if (model.state) view.setState(model.state);
  renderConn();
  updateHint();
  showTab('lineage');

  return {
    el,
    view,
    destroy: () => {
      for (const c of cleanups) c();
      if (toastTimer) clearTimeout(toastTimer);
      view.destroy();
    },
  };
}

function dl(rows: [string, Node | string][]): HTMLElement {
  return h('dl', { class: 'kv' }, ...rows.flatMap(([k, v]) => [h('dt', {}, k), h('dd', {}, v)]));
}

/**
 * A bar for a value in 0..max (the sim's needs and health are on a 0–100
 * scale, cold and heat in degrees outside the comfort range). `kind` says
 * whether a high value is good, bad or neither.
 */
function meter(v: number, max: number, kind: 'good' | 'bad' | 'neutral', unit = ''): HTMLElement {
  const x = Math.max(0, Math.min(1, v / max));
  const bad = kind === 'bad' ? x : kind === 'good' ? 1 - x : 0;
  const cls = kind === 'neutral' ? 'neutral' : bad > 0.7 ? 'bad' : bad > 0.4 ? 'warn' : 'good';
  return h('span', { class: 'meter' }, h('span', { class: `meter-fill ${cls}`, style: `width:${(x * 100).toFixed(0)}%` }),
    h('span', { class: 'meter-num' }, `${v.toFixed(0)}${unit}`));
}

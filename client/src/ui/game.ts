// The play screen: world view (tile map), top bar (clock, weather,
// connection) and a sidebar with the organism inspector, matings, the
// genetics dashboard with the alerts feed, the lineage/leaderboards tab, the
// ranked seasons (hall of fame) and the world chronicle (species, eras,
// catastrophes, achievements).

import { commandError } from '../errors';
import { DEFAULT_TICKS_PER_DAY, gameAndReal, ticksToNextSeason } from './duration';
import { reconcile, setText } from './reconcile';
import { friendlyError, MEETING_TILES } from './friendlyErrors';
import { historyRows, offerLine } from './matingHistory';
import { api, ApiError } from '../api';
import { alertEntry, type GameModel } from '../model';
import { buildFamilyTree, type FamilyModel } from '../familyTree';
import { actionText, capInfo, grouped, migrationText } from '../lineageInfo';
import { renderFamilyTree } from './familyTree';
import { exposeCards, familyShareBar } from '../share/familyShare';
import type { AlertPrefs, ClientMsg, LineageStats, MatingOutcome, MatingPreviewView, NearbyOrganism, OrgId, OwnOrganism, ResultMsg, SeasonBoard, StateView } from '../protocol';
import { WILD } from '../protocol';
import { WorldView } from '../world/worldView';
import { applyDynamic, decodeTerrain, FOOD_BANDS, terrainSummary, type TerrainGrid } from '../world/terrain';
import { cssColor, ownerColor, OWN_COLOR } from '../world/colors';
import { GROUND, ICE, SNOW, toCss, WATER, type Rgb } from '../world/palette';
import { describeLook } from '../world/creature';
import { visionLines, visionStats } from '../world/visionCones';
import { visionSummary } from '../vision';
import { append, clear, h, pct } from './dom';
import { icon, type IconName } from './icons';
import { renderAlertPrefs, renderDashboard, renderFeed } from './dashboard';
import { catastropheBanner, catastropheEffect, catastropheLabel, renderWorld, type Chronicle } from './chronicle';
import { renderCurrentSeason, renderSeasons as renderRanked, seasonKey, type SeasonsData } from './seasons';
import { compatSummary, type PairContext } from './mating';
import { buildMarket } from './market';
import { balanceText } from './marketData';
import { createOnboarding } from './onboarding';
import { onboardingDisabled } from '../onboarding';
import { pushSettings } from './push';
import {
  canSplit, claimWait, clearMigrationMsg, groupColor, groupName, groupObedience, groupOptions, groupOrders, groupsOf, migrateMsgs, parseChoice,
  homeRangeNote, rallyLeft, rallyMsg, targetGroup, territoryTap, validChoice, type GroupChoice, type TerritoryDraft,
} from '../groups';

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
  /** The world played in ('' for the server's default); named in the top bar when the server has several. */
  world?: string;
  /** Open the design screen for a new lineage (offered once the player's lineage has died out). */
  onNewLineage?: () => void;
}

const LOCAL_CODES = new Set(['not_connected', 'timeout', 'disconnected', 'closed']);
/** How far an attack order reaches (the server's `World::attack_range`, tiles). */
const ATTACK_RANGE = 8;

/**
 * Every organism of the player's lineage has died: say so, point to what
 * killed them, and offer a new lineage (renown and records stay).
 */
function extinctNoticeEl(id: string, onNewLineage: (() => void) | undefined): HTMLElement {
  return h('div', { class: 'notice bad', id, role: 'status' },
    h('p', {}, h('strong', {}, 'Your lineage has died out.'), ' None of your organisms are alive. The Genes tab charts what killed them.'),
    onNewLineage ? h('p', {}, h('button', { type: 'button', class: 'new-lineage', id: `${id}-new`, onclick: onNewLineage }, 'Start a new lineage'),
      h('span', { class: 'muted small' }, ' Your renown, records and titles stay.')) : null);
}
/** The dynamic terrain layers change every simulated hour. */
const TERRAIN_REFRESH_MS = 4000;
const STATS_REFRESH_MS = 5000;
const SEASONS_REFRESH_MS = 15000;
const CHRONICLE_REFRESH_MS = 10000;
/**
 * The seasons tab's fallback refresh. The standings under way are pushed over the socket while the tab is open
 * (`watch_season`); finished seasons only change at a rollover, which a push shows.
 */
const RANKED_REFRESH_MS = 30000;
/** Least time between two fetches of the finished seasons after rollovers. */
const RANKED_ROLLOVER_MS = 5000;
/** Rows per category of the season under way. */
const RANKED_ROWS = 5;
/** Finished seasons listed at first; "Older seasons" adds as many again. */
const RANKED_PAGE = 10;
const MARKET_REFRESH_MS = 5000;

type Tab = 'organism' | 'matings' | 'market' | 'genetics' | 'family' | 'lineage' | 'seasons' | 'world';
const TAB_LABELS: Record<Tab, string> = {
  // One name per tab on every screen (the tour and Help use the same words).
  organism: 'Animal', matings: 'Mates', market: 'Market', genetics: 'Genes', family: 'Family', lineage: 'Lineage', seasons: 'Ranks', world: 'World',
};
/** What each tab holds (its tooltip). */
const TAB_TITLES: Record<Tab, string> = {
  organism: 'Animal: the selected organism, its needs and actions',
  matings: 'Mates: directed matings and requests',
  market: 'Market: renown, stud listings and auctions',
  genetics: 'Genes: the genetics dashboard, whose genes your lineage carries, and alerts',
  family: 'Family: your lineage\'s family tree',
  lineage: 'Lineage: counts, groups, the legacy leaderboard and your organisms',
  seasons: 'Ranks: the ranked season under way and the hall of fame',
  world: 'World: catastrophes, eras, species and achievements',
};
/** Remembers the Food layer switch. */
const FOOD_KEY = 'evo.foodLayer';
/** The family tree's refresh while its tab is open (it changes once a recorded day). */
const FAMILY_REFRESH_MS = 20000;
type Mode = 'select' | 'migrate' | 'territory' | 'rally' | 'attack';
/** Narrow screens (phones): the sidebar becomes a bottom sheet over the map with a tab bar. */
const NARROW = '(max-width: 760px)';

export interface GameScreen {
  el: HTMLElement;
  /** Exposed for the end-to-end test and debugging (`window.evo`). */
  view: WorldView;
  /** Open a tab by name ('matings', 'market', ...; unknown names are ignored), as a notification click asks. */
  showTab: (tab: string) => void;
  destroy: () => void;
}

export async function gameScreen(deps: GameDeps): Promise<GameScreen> {
  const { model } = deps;
  const cleanups: (() => void)[] = [];
  /** Living organisms one player may have (`/api/world`), for the Lineage tab's cap row. */
  let worldCap: number | undefined;
  /** The genetics dashboard's data (declared up here: the Lineage tab reads it too, possibly before its fetch is set up). */
  let stats: LineageStats | null = null;
  const inspExtinct = extinctNoticeEl('inspector-extinct', deps.onNewLineage);
  const lineageExtinct = extinctNoticeEl('lineage-extinct', deps.onNewLineage);
  lineageExtinct.hidden = true;

  const send = async (m: ClientMsg): Promise<ResultMsg> => {
    const r = await deps.send(m);
    if (!r.ok && r.error && LOCAL_CODES.has(r.error.code)) model.toast('error', commandError(m.type, r.error));
    return r;
  };

  // ---------------------------------------------------------------- layout
  const connPill = h('span', { class: 'pill', id: 'conn-state' });
  const clockEl = h('span', { id: 'clock' });
  const weatherEl = h('span', { class: 'muted', id: 'weather' });
  const bannerEl = h('span', { class: 'catastrophe-pill', id: 'catastrophe-banner', hidden: true });
  const worldEl = h('span', { class: 'pill world-pill', id: 'world-name', hidden: true });
  if (deps.world) {
    void api.worlds().then((l) => {
      const w = l.worlds.find((x) => x.id === deps.world);
      if (!w || l.worlds.length < 2) return;
      worldEl.textContent = w.name;
      worldEl.title = `You are playing in the world ${w.name} (log out to pick another)`;
      worldEl.hidden = false;
    }).catch(() => {});
  }
  const balanceEl = h('button', { class: 'pill balance-pill', id: 'balance', title: 'Renown: earned from your legacy share, spent on stud fees (open the Market tab)', onclick: () => showTab('market') });
  const helpBtn = h('button', { class: 'secondary help-btn', id: 'help-btn', type: 'button', title: 'Help: controls and the guided tour', 'aria-label': 'Help and guided tour', 'aria-haspopup': 'dialog' },
    h('span', { class: 'help-word' }, 'Help'), h('span', { class: 'help-mark', 'aria-hidden': 'true' }, '?'));
  const top = h('header', { class: 'topbar' },
    h('strong', {}, 'Evolution MMO'), h('span', { class: 'muted' }, model.name), worldEl, clockEl, weatherEl, bannerEl,
    h('span', { class: 'spacer' }), balanceEl, connPill, helpBtn,
    h('button', { class: 'secondary', id: 'logout', onclick: deps.onLogout }, 'Log out'));

  const stage = h('div', { class: 'stage', id: 'stage' });
  // Map toolbar: icons with labels, in groups (camera; group commands; view
  // window; legend). On phones the camera group stays and the rest folds
  // into the Tools menu (`#tools-more`).
  const tool = (id: string, ic: IconName, text: string, title: string, extra = '') =>
    h('button', { class: `tool${extra}`, id, title, 'aria-label': title }, icon(ic), h('span', { class: 'tool-label' }, text));
  const followBtn = tool('follow', 'follow', 'Follow', 'Keep the camera on the view window (your possessed organism or your lineage)');
  const zoomInBtn = tool('zoom-in', 'plus', 'Zoom in', 'Zoom in', ' icon-only');
  const zoomOutBtn = tool('zoom-out', 'minus', 'Zoom out', 'Zoom out', ' icon-only');
  zoomInBtn.addEventListener('click', () => view.zoomBy(1.4));
  zoomOutBtn.addEventListener('click', () => view.zoomBy(1 / 1.4));
  const migrateBtn = tool('migrate', 'migrate', 'Migrate…', 'Click the map to send your adults there (the group picked in the group menu, or all)');
  const clearMigBtn = tool('clear-migration', 'clear', 'Clear', 'Cancel the migration (the group picked in the group menu, or all)');
  // Group commands: pick a group, claim ground for it, split it.
  const groupSelect = h('select', { class: 'tool group-select', id: 'group-select', title: 'Which of your groups the group commands act on', 'aria-label': 'Group' });
  const territoryBtn = tool('territory', 'territory', 'Territory…', 'Claim a territory for the group: tap its center, then its edge');
  const rallyBtn = tool('rally', 'rally', 'Rally…', 'Gather the group at a point: tap where; it holds there for a few days');
  const splitBtn = tool('split-group', 'split', 'Split', 'Split the group in two (the far half across its widest spread)');
  const pinBtn = tool('pin-view', 'pin', 'Pin view', 'Pin the server view window (what you can see of others) to the camera center');
  const pinLabel = pinBtn.querySelector('.tool-label')!;
  const hint = h('div', { class: 'hint', id: 'hint' });
  const toasts = h('div', { class: 'toasts', id: 'toasts', 'aria-live': 'polite' });
  const moreBtn = tool('tools-more', 'more', 'Tools', 'Group commands and the view window', ' narrow-only');
  moreBtn.setAttribute('aria-haspopup', 'true');
  moreBtn.setAttribute('aria-expanded', 'false');
  const legendBtn = tool('legend-toggle', 'legend', 'Legend', 'Show or hide the map legend');
  // The Food layer: where plants (and so grazers) are, from bare to rich.
  const foodBtn = tool('food-toggle', 'food', 'Food', 'Show or hide the Food layer: plant food on the ground, from bare (red) to rich (green)');
  foodBtn.setAttribute('aria-pressed', 'false');
  legendBtn.setAttribute('aria-expanded', 'false');
  const toolsMenu = h('div', { class: 'tools-menu', id: 'tools-menu' },
    h('div', { class: 'tool-group', role: 'group', 'aria-label': 'Group commands' }, groupSelect, migrateBtn, clearMigBtn, territoryBtn, rallyBtn, splitBtn),
    h('div', { class: 'tool-group', role: 'group', 'aria-label': 'View window' }, pinBtn));
  const toolbar = h('div', { class: 'toolbar', role: 'toolbar', 'aria-label': 'Map tools' },
    h('div', { class: 'tool-group', role: 'group', 'aria-label': 'Camera' }, followBtn, zoomInBtn, zoomOutBtn),
    toolsMenu, moreBtn, foodBtn, legendBtn);
  function setToolsMenu(open: boolean): void {
    toolbar.classList.toggle('more-open', open);
    moreBtn.classList.toggle('active', open);
    moreBtn.setAttribute('aria-expanded', String(open));
  }
  moreBtn.addEventListener('click', () => setToolsMenu(!toolbar.classList.contains('more-open')));
  // An action picked from the menu closes it (the group menu stays open to pick an action next).
  toolsMenu.addEventListener('click', (e) => {
    if ((e.target as Element).closest('button')) setToolsMenu(false);
  });
  const swatch = (cls: string, style = '') => h('i', { class: `key ${cls}`, style });
  const item = (key: HTMLElement, text: string, title?: string) => h('span', { class: 'key-item', ...(title ? { title } : {}) }, key, text);
  const legend = h('div', { class: 'legend', id: 'legend' },
    h('h4', {}, 'Creatures'),
    item(swatch('foot', `--c:${cssColor(OWN_COLOR)}`), 'yours'),
    item(swatch('foot', `--c:${cssColor(ownerColor(1))}`), 'other players'),
    item(swatch('foot faint', `--c:${cssColor(ownerColor(WILD))}`), 'wild'),
    item(swatch('foot double', '--c:#ff7b72'), 'possessed'),
    item(swatch('pip', '--c:#7ee787'), 'pregnant'),
    item(swatch('foot', '--c:#ffffff'), 'selected'),
    h('span', { class: 'key-item wide', title: 'Body size, diet (muzzle, snout or fangs), warm or cold-blooded build, fur/scales/armor, colors, wings or fins; juveniles are drawn smaller' }, 'smaller = juvenile'),
    h('span', { class: 'key-item wide muted' }, '! fleeing · ⌖ hunting · z resting · ♥ courting (yours)'),
    h('h4', {}, 'Map'),
    item(swatch('brackets'), 'view window'),
    item(swatch('disc', `--c:${cssColor(groupColor(0))}`), 'territory', 'Claimed with Territory…; each group has its own color'),
    item(swatch('ring', `--c:${cssColor(groupColor(0))}`), 'rally', 'Set with Rally…: the group gathers in the ring around the point'),
    item(swatch('ring home-ring'), '⌂ home range', 'Your largest group\'s territory: while you are offline its members inside are hunted less'));
  const terrainLegend = h('div', { class: 'legend terrain-legend', id: 'terrain-legend' });
  // Possess / release the selected own organism without opening the panel (phones).
  const fab = h('button', { class: 'fab', id: 'fab-possess', hidden: true });
  // While possessing: who, how to walk and select, and Release always in reach (not only in the inspector).
  const possessText = h('span', { class: 'possess-text' });
  const possessInspect = h('button', { class: 'secondary small', id: 'possess-inspect', type: 'button' }, 'Inspect');
  const possessRelease = h('button', { class: 'small danger', id: 'possess-release', type: 'button' }, 'Release');
  // Attack mode: the next tap on an animal sends the possessed one after it (a plain tap walks).
  const possessAttack = h('button', { class: 'small', id: 'possess-attack', type: 'button', 'aria-pressed': 'false' }, 'Attack');
  const possessBar = h('div', { class: 'possess-bar', id: 'possess-bar', hidden: true, role: 'status' }, possessText, possessAttack, possessInspect, possessRelease);
  // A tap on overlapping animals (touch): the most relevant one is picked; these chips pick another.
  const chooser = h('div', { class: 'pick-chooser', id: 'pick-chooser', hidden: true, role: 'group', 'aria-label': 'Animals here' });
  const legends = h('div', { class: 'legends', id: 'legends' }, terrainLegend, legend);
  // A catastrophe under way: pinned over the map (not in the capped feed),
  // wrapping on phones; it can be folded to one line but not dismissed.
  const catTitle = h('strong', { class: 'cat-title' });
  const catLeft = h('span', { class: 'cat-left' });
  const catEffect = h('span', { class: 'cat-effect' });
  const catFold = h('button', { class: 'secondary cat-fold', type: 'button', 'aria-expanded': 'true', title: 'Fold or unfold the catastrophe notice' }, 'Less');
  const catAlert = h('div', { class: 'catastrophe-alert', id: 'catastrophe-alert', role: 'alert', hidden: true },
    h('span', { class: 'cat-icon', 'aria-hidden': 'true' }, '⚠'), h('div', { class: 'cat-text' }, h('div', {}, catTitle, ' ', catLeft), catEffect), catFold);
  catFold.addEventListener('click', () => {
    const folded = catAlert.classList.toggle('folded');
    catFold.textContent = folded ? 'More' : 'Less';
    catFold.setAttribute('aria-expanded', String(!folded));
  });
  let catSeen: number | null = null;
  const stageWrap = h('div', { class: 'stage-wrap' }, stage, toolbar, catAlert, hint, legends, fab, possessBar, chooser, toasts);
  // The legend is collapsible: hidden by default (it covers part of the map;
  // the tour points to its button), the choice remembered on this device.
  function setLegend(on: boolean, remember: boolean): void {
    stageWrap.classList.toggle('show-legend', on);
    legendBtn.classList.toggle('active', on);
    legendBtn.setAttribute('aria-expanded', String(on));
    if (remember) {
      try { localStorage.setItem('evo.legend', on ? 'on' : 'off'); } catch { /* storage may be blocked */ }
    }
  }
  {
    let stored: string | null = null;
    try { stored = localStorage.getItem('evo.legend'); } catch { /* storage may be blocked */ }
    setLegend(stored === 'on', false);
  }
  legendBtn.addEventListener('click', () => setLegend(!stageWrap.classList.contains('show-legend'), true));

  const tabs: Record<Tab, HTMLElement> = {
    organism: h('div', { class: 'panel', id: 'panel-organism' }),
    matings: h('div', { class: 'panel', id: 'panel-matings' }),
    market: h('div', { class: 'panel', id: 'panel-market' }),
    genetics: h('div', { class: 'panel', id: 'panel-genetics' }),
    family: h('div', { class: 'panel', id: 'panel-family' }),
    lineage: h('div', { class: 'panel', id: 'panel-lineage' }),
    seasons: h('div', { class: 'panel', id: 'panel-seasons' }),
    world: h('div', { class: 'panel', id: 'panel-world' }),
  };
  const tabBtns = (Object.keys(tabs) as Tab[]).map((t) =>
    h('button', { class: 'tab', 'data-tab': t, id: `tab-${t}`, title: TAB_TITLES[t], onclick: () => tabClicked(t) }, TAB_LABELS[t]));
  const alertBadge = h('span', { class: 'badge', id: 'alert-badge', hidden: true });
  tabBtns.find((b) => b.dataset.tab === 'genetics')!.append(alertBadge);
  let seenFeed = 0;
  const sheetHandle = h('button', { class: 'sheet-handle narrow-only', id: 'sheet-handle', 'aria-label': 'Show or hide the panel' });
  const sidebar = h('aside', { class: 'sidebar', id: 'sidebar' }, sheetHandle, h('nav', { class: 'tabs', id: 'tabbar' }, ...tabBtns), ...Object.values(tabs));
  const el = h('div', { class: 'screen game starting' }, top, h('div', { class: 'game-body' }, stageWrap, sidebar));

  // On phones the panels are a bottom sheet: the tab bar always shows; the
  // sheet opens on a tab tap (or a selection) and closes on a second tap of
  // the open tab, the handle, or a tap on empty map.
  const narrowQuery = window.matchMedia(NARROW);
  const narrow = () => narrowQuery.matches;
  let sheetOpen = false;
  function setSheet(open: boolean): void {
    sheetOpen = open;
    el.classList.toggle('sheet-open', open);
  }
  sheetHandle.addEventListener('click', () => setSheet(!sheetOpen));
  const onNarrowChange = () => updateHint();
  narrowQuery.addEventListener('change', onNarrowChange);
  cleanups.push(() => narrowQuery.removeEventListener('change', onNarrowChange));

  let tab: Tab = 'lineage';
  /**
   * False until the whole screen is built. The tabs are on screen while it
   * awaits the canvas and the terrain, and the tab refreshes and panel
   * renderers use parts declared further down: before then a tab click only
   * switches the visible panel (once ready, the chosen tab loads), and the map
   * toolbar ignores clicks (`.starting`).
   */
  let ready = false;
  function tabClicked(t: Tab): void {
    if (narrow() && sheetOpen && tab === t) return setSheet(false);
    setSheet(true);
    showTab(t);
  }
  function showTab(t: Tab): void {
    tab = t;
    for (const b of tabBtns) b.classList.toggle('active', b.dataset.tab === t);
    for (const [k, p] of Object.entries(tabs)) p.hidden = k !== t;
    if (!ready) return;
    if (t === 'genetics') void refreshStats();
    if (t === 'lineage') void refreshSeasons();
    if (t === 'world') void refreshChronicle();
    if (t === 'family') void refreshFamily();
    if (t === 'seasons') void refreshRanked();
    watchSeason(t === 'seasons');
    if (t === 'market') void market.refresh();
    if (t === 'matings') void refreshMatingHistory();
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
    territoryBtn.classList.toggle('active', m === 'territory');
    rallyBtn.classList.toggle('active', m === 'rally');
    possessAttack.classList.toggle('active', m === 'attack');
    possessAttack.setAttribute('aria-pressed', String(m === 'attack'));
    if (m !== 'territory') setDraft(null);
    updateHint();
  }

  // --- Groups: the menu's choice, and a territory being drawn.
  let groupChoice: GroupChoice = 'all';
  let draft: TerritoryDraft | null = null;
  function setDraft(d: TerritoryDraft | null): void {
    draft = d;
    view.territoryDraft = d;
  }
  function setGroupChoice(c: GroupChoice): void {
    groupChoice = c;
    groupSelect.value = String(c);
    view.selectedGroup = c === 'all' ? null : c;
    renderPanels();
  }
  let groupMenuKey = '';
  function renderGroupMenu(): void {
    const groups = groupsOf(model.state);
    const opts = groupOptions(groups);
    const key = JSON.stringify(opts);
    if (key !== groupMenuKey) {
      groupMenuKey = key;
      // In place: member counts change at every birth, and a rebuild closed an open list.
      reconcile(groupSelect, opts, (o) => o.value, (o) => h('option', { value: o.value }) as HTMLOptionElement, (opt, o) => setText(opt, o.text));
    }
    const valid = validChoice(groupChoice, groups);
    if (valid !== groupChoice || groupSelect.value !== String(valid)) setGroupChoice(valid);
  }
  groupSelect.addEventListener('change', () => setGroupChoice(parseChoice(groupSelect.value)));
  /** The one group a tool acts on, or a toast asking to pick one. */
  function toolGroup(): number | null {
    const g = targetGroup(groupChoice, groupsOf(model.state));
    if (g === null) model.toast('error', 'Pick a group in the group menu first');
    return g;
  }
  async function splitGroup(id: number): Promise<void> {
    const r = await send({ type: 'split_group', group: id });
    if (!r.ok) return;
    const d = r.data as { group: number; moved: number; kept: number };
    model.toast('info', `${groupName(id)} split: ${d.moved} went to ${groupName(d.group)}, ${d.kept} stayed`);
    groupChoice = d.group; // picked once the next state lists it
  }
  territoryBtn.addEventListener('click', () => {
    if (mode === 'territory') return setMode('select');
    if (toolGroup() !== null) setMode('territory');
  });
  rallyBtn.addEventListener('click', () => {
    if (mode === 'rally') return setMode('select');
    if (toolGroup() !== null) setMode('rally');
  });
  splitBtn.addEventListener('click', () => {
    const id = toolGroup();
    if (id !== null) void splitGroup(id);
  });
  function possessed(): OwnOrganism | undefined {
    return model.state?.organisms.find((o) => o.possessed);
  }
  function updateHint(): void {
    const p = possessed();
    const touch = narrow() || window.matchMedia('(pointer: coarse)').matches;
    const tap = touch ? 'Tap' : 'Click';
    const territoryHint = () => {
      const groups = groupsOf(model.state);
      const id = targetGroup(groupChoice, groups);
      const who = id === null ? 'the group' : groupName(id);
      return draft
        ? `${tap} the edge of ${who}'s territory (radius up to ${draft.radius.toFixed(0)} tiles)${touch ? '' : ' (Esc cancels)'}.`
        : `${tap} the center of ${who}'s territory${touch ? '' : ' (Esc cancels)'}.`;
    };
    const rallyHint = () => {
      const id = targetGroup(groupChoice, groupsOf(model.state));
      return `${tap} the point where ${id === null ? 'the group' : groupName(id)} should gather${touch ? '' : ' (Esc cancels)'}.`;
    };
    hint.textContent = mode === 'territory' ? territoryHint()
      : mode === 'rally' ? rallyHint()
      : mode === 'migrate'
      ? `${tap} the map to set the migration target${touch ? '' : ' (Esc cancels)'}.`
      : mode === 'attack' && p
      ? `${tap} the animal #${p.id} should attack: another player's or a wild one in sight${touch ? '' : ' (Esc cancels)'}.`
      : p ? `Possessing #${p.id}: ${tap.toLowerCase()} the map to walk there.`
      : touch ? 'Tap an organism to inspect it, long-press yours to possess. Drag to pan, pinch to zoom.'
      : 'Click an organism to inspect it (hold to possess yours). Drag to pan, wheel to zoom.';
    // The possession bar says it all while possessing.
    hint.hidden = !!p && mode === 'select';
    renderFab();
    renderPossessBar();
  }

  function renderPossessBar(): void {
    const p = possessed();
    possessBar.hidden = !p;
    if (!p) {
      // Possession over: so is attack mode.
      if (mode === 'attack') {
        mode = 'select';
        possessAttack.classList.remove('active');
        possessAttack.setAttribute('aria-pressed', 'false');
      }
      return;
    }
    const touch = narrow() || window.matchMedia('(pointer: coarse)').matches;
    possessBar.dataset.org = String(p.id);
    setText(possessText, mode === 'attack'
      ? `#${p.id}: ${touch ? 'tap' : 'click'} the animal to attack.`
      : touch
      ? `Possessing #${p.id}: tap to walk (even onto other animals); long-press one to select it.`
      : `Possessing #${p.id}: click to walk (even onto other animals); Shift-click or hold to select one.`);
  }
  possessAttack.addEventListener('click', () => setMode(mode === 'attack' ? 'select' : 'attack'));
  /** Send the possessed organism `p` after `target`; the server says why not. */
  async function attackWith(p: OwnOrganism, target: OrgId): Promise<boolean> {
    const r = await send({ type: 'possess_attack', organism: p.id, target });
    // A refusal is toasted once, by the model, like every failed command.
    if (r.ok) {
      const warning = (r.data as { warning?: string } | undefined)?.warning;
      model.toast(warning ? 'error' : 'info', `#${p.id} is hunting #${target}; hungry pack-mates nearby join in${warning ? `. ${warning}` : ''}`);
      model.select(target);
    }
    return r.ok;
  }
  possessRelease.addEventListener('click', () => {
    const id = Number(possessBar.dataset.org);
    if (id) void send({ type: 'release', organism: id });
  });
  possessInspect.addEventListener('click', () => {
    const id = Number(possessBar.dataset.org);
    if (!id) return;
    model.select(id);
    if (narrow()) setSheet(true);
    showTab('organism');
  });

  let chooserTimer: ReturnType<typeof setTimeout> | undefined;
  function hideChooser(): void {
    clearTimeout(chooserTimer);
    chooser.hidden = true;
  }
  /** Several animals under a finger: chips to pick another than the one chosen. */
  function showChooser(ids: OrgId[], chosen: OrgId): void {
    const s = model.state;
    const label = (id: OrgId) => {
      const own = s?.organisms.find((o) => o.id === id);
      if (own) return `#${id} ${own.sex === 'female' ? '♀' : '♂'} yours`;
      const n = s?.nearby?.find((o) => o.id === id);
      return `#${id} ${n?.owner === WILD ? 'wild' : 'other player'}`;
    };
    chooser.replaceChildren(h('span', { class: 'muted small' }, 'Here:'),
      ...ids.slice(0, 6).map((id) => h('button', {
        type: 'button', class: `chip${id === chosen ? ' active' : ''}`, 'data-org': id,
        onclick: () => { model.select(id); hideChooser(); if (!narrow() || sheetOpen) showTab('organism'); },
      }, label(id))),
      h('button', { type: 'button', class: 'chip close', 'aria-label': 'Close', onclick: hideChooser }, '×'));
    chooser.hidden = false;
    clearTimeout(chooserTimer);
    chooserTimer = setTimeout(hideChooser, 8000);
  }

  /** The floating Possess / Release button for the selected own organism. */
  function renderFab(): void {
    const own = model.selected !== null ? model.state?.organisms.find((o) => o.id === model.selected) : undefined;
    fab.hidden = !own;
    if (!own) return;
    const text = own.possessed ? `Release #${own.id}` : `Possess #${own.id}`;
    if (fab.textContent !== text) fab.textContent = text;
    fab.classList.toggle('active', own.possessed);
    fab.dataset.org = String(own.id);
  }
  fab.addEventListener('click', async () => {
    const id = Number(fab.dataset.org);
    const own = model.state?.organisms.find((o) => o.id === id);
    if (!own) return;
    if (own.possessed) await send({ type: 'release', organism: id });
    else await possess(id);
  });
  async function possess(id: OrgId): Promise<boolean> {
    const r = await send({ type: 'possess', organism: id });
    if (r.ok) {
      view.setFollow(true);
      model.toast('info', `Possessing #${id}. ${narrow() ? 'Tap' : 'Click'} the map to walk.`);
    }
    return r.ok;
  }

  view.onFollowChange = (f) => followBtn.classList.toggle('active', f);
  followBtn.classList.add('active');
  followBtn.addEventListener('click', () => view.setFollow(!view.follow));
  migrateBtn.addEventListener('click', () => setMode(mode === 'migrate' ? 'select' : 'migrate'));
  clearMigBtn.addEventListener('click', () => void send(clearMigrationMsg(groupChoice)));
  let pinned = false;
  pinBtn.addEventListener('click', async () => {
    const r = pinned ? await send({ type: 'clear_view' }) : await send({ type: 'set_view', x: view.cam.x, y: view.cam.y, radius: 24 });
    if (r.ok) {
      pinned = !pinned;
      pinLabel.textContent = pinned ? 'Unpin view' : 'Pin view';
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
      const msgs = migrateMsgs(groupChoice, groupsOf(model.state), c.x, c.y);
      const rs = await Promise.all(msgs.map((m) => send(m)));
      // The server echoes the target it used: on land, possibly moved off water.
      const r = rs.find((x) => x.ok) ?? rs[0];
      const t: { x: number; y: number; note?: string } = (r?.data as { x: number; y: number; note?: string } | undefined) ?? c;
      const who = groupChoice === 'all' ? 'Adults' : `${groupName(groupChoice)} adults`;
      if (r?.ok) {
        model.toast('info', `${who} migrating toward (${t.x.toFixed(0)}, ${t.y.toFixed(0)})${t.note ? `: ${t.note}` : ''}`);
      } else if (r?.error) {
        model.toast('error', `Migrate: ${r.error.message}`);
      }
      return;
    }
    if (mode === 'rally') {
      setMode('select');
      const id = targetGroup(groupChoice, groupsOf(model.state));
      if (id === null) return;
      const r = await send(rallyMsg(id, c.x, c.y));
      if (r.ok) {
        const t = r.data as { x: number; y: number; radius: number };
        model.toast('info', `${groupName(id)} gathering at (${t.x.toFixed(0)}, ${t.y.toFixed(0)}), within ${t.radius.toFixed(1)} tiles`);
      } else if (r.error) {
        model.toast('error', `Rally: ${r.error.message}`);
      }
      return;
    }
    if (mode === 'territory') {
      const groups = groupsOf(model.state);
      const g = groups.find((x) => x.id === targetGroup(groupChoice, groups));
      if (!g) return setMode('select');
      const step = territoryTap(draft, g, c);
      setDraft(step.draft);
      updateHint();
      if (!step.claim) return;
      setMode('select');
      const r = await send(step.claim);
      if (r.ok) {
        const t = r.data as { x: number; y: number; radius: number };
        model.toast('info', `${groupName(g.id)} holds a territory of radius ${t.radius.toFixed(1)} at (${t.x.toFixed(0)}, ${t.y.toFixed(0)})`);
      } else if (r.error) {
        model.toast('error', `Territory: ${r.error.message}`);
      }
      return;
    }
    if (mode === 'attack') {
      const p = possessed();
      if (!p) return setMode('select');
      const own = (id: OrgId) => model.state?.organisms.some((o) => o.id === id);
      // Under a finger, the nearest animal that isn't yours.
      const target = [...(c.candidates ?? []), ...(c.organism !== null ? [c.organism] : [])].find((id) => !own(id));
      if (target === undefined) {
        if (c.organism === null) return setMode('select');
        model.toast('info', "That's one of yours: pick another player's or a wild animal.");
        return;
      }
      setMode('select');
      await attackWith(p, target);
      return;
    }
    hideChooser();
    const p = possessed();
    // Possessing: a click walks there, even onto other animals in a herd;
    // a modifier click (or a long press) selects, as does a click on the
    // possessed animal itself.
    if (p && !c.modifier && c.organism !== p.id) {
      view.setMoveTarget({ x: c.x, y: c.y });
      const r = await send({ type: 'order_move', organism: p.id, x: c.x, y: c.y });
      if (!r.ok) view.rejectMove();
      const used = r.data as { x: number; y: number; note?: string } | undefined;
      if (r.ok && used?.note) {
        // Out of reach: it walks as near as it can.
        view.setMoveTarget({ x: used.x, y: used.y });
        model.toast('info', `#${p.id}: ${used.note}`);
      }
      return;
    }
    if (c.organism !== null) {
      // A tap on overlapping animals picks the most relevant (yours, then the nearest) and offers the others.
      const pick = c.touch && c.candidates?.length ? c.candidates[0] : c.organism;
      model.select(pick);
      if (c.touch && (c.candidates?.length ?? 0) > 1) showChooser(c.candidates!, pick);
      // On phones a selection shows the inspector only if the sheet is up;
      // the floating button covers possess without hiding the map.
      if (!narrow() || sheetOpen) showTab('organism');
      return;
    }
    if (p) {
      // A modifier click on empty ground while possessing: just clear the selection.
      model.select(null);
    } else {
      model.select(null);
      if (narrow()) setSheet(false);
    }
  };
  // Long press: possess your own organism under the finger (or the cursor),
  // otherwise act like a tap.
  view.onLongPress = async (c) => {
    if (mode === 'select' && c.organism !== null && model.state?.organisms.some((o) => o.id === c.organism)) {
      model.select(c.organism);
      const own = model.state.organisms.find((o) => o.id === c.organism);
      if (own && !own.possessed) await possess(own.id);
      return;
    }
    // Held on another animal: select it (while possessing, a plain tap would walk).
    await view.onClick({ ...c, modifier: true });
  };

  // Tile map: the whole grid once, then the dynamic layers periodically.
  let terrain: TerrainGrid | null = null;
  function showTerrain(g: TerrainGrid): void {
    view.setTerrain(g);
    const t = terrainSummary(g);
    const chip = (color: Rgb, text: string) => h('span', { class: 'key-item' }, h('i', { class: 'key chip', style: `--c:${toCss(color)}` }), text);
    terrainLegend.replaceChildren(
      h('h4', {}, 'Terrain', h('span', { class: 'muted' }, ` · ${g.season}, ${String(g.hour).padStart(2, '0')}:00`)),
      chip(WATER.shallow, 'water'), chip(WATER.river, 'river'),
      chip(GROUND.straw, `drought ${pct(t.drought, 0)}`), chip(WATER.flood, `flooded ${pct(t.flood, 1)}`),
      chip(SNOW, `snow ${pct(t.snow, 0)}`), chip(ICE, `ice ${pct(t.ice, 0)}`),
      ...(view.foodOverlay ? [h('h4', {}, 'Food'), ...FOOD_BANDS.map((b) => chip(b.rgb, b.label))] : []));
  }
  function setFood(on: boolean, remember: boolean): void {
    view.setFoodOverlay(on);
    foodBtn.classList.toggle('active', on);
    foodBtn.setAttribute('aria-pressed', String(on));
    if (remember) {
      try {
        localStorage.setItem(FOOD_KEY, on ? 'on' : 'off');
      } catch {
        /* private mode */
      }
    }
    if (terrain) showTerrain(terrain);
  }
  foodBtn.addEventListener('click', () => setFood(!view.foodOverlay, true));
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
  // The calendar, for countdowns (offers expire when the season changes).
  let daysPerYear = 0;
  let ticksPerDay = DEFAULT_TICKS_PER_DAY;
  const msPerTick = () => view.tickClock.msPerTick;
  const span = (ticks: number) => gameAndReal(ticks, ticksPerDay, msPerTick());
  try {
    const w = await api.world();
    worldCap = w.lineage_cap;
    if (w.days_per_year) daysPerYear = w.days_per_year;
    if (w.ticks_per_day) ticksPerDay = w.ticks_per_day;
    view.setWorldSize(w.width, w.height);
    view.centerOn(w.width / 2, w.height / 2);
  } catch {
    /* defaults */
  }
  await loadTerrain();
  try {
    if (localStorage.getItem(FOOD_KEY) === 'on') setFood(true, false);
  } catch {
    /* private mode */
  }
  const terrainTimer = setInterval(() => void refreshTerrain(), TERRAIN_REFRESH_MS);
  cleanups.push(() => clearInterval(terrainTimer));

  // Genetics dashboard: fetched while its tab is open (and once now, for the feed).
  // Finished matings from the server (survive a reload; include matings
  // within the lineage and those that closed while the tab was shut).
  async function refreshMatingHistory(): Promise<void> {
    try {
      model.setMatingLog(await api.matingHistory(deps.token));
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) deps.onUnauthorized();
      // An older server without the endpoint: the history is this session's only.
    }
  }
  void refreshMatingHistory();

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

  // Ranked seasons: fetched while their tab is open.
  const ranked: SeasonsData = { current: null, list: null, mine: null, details: new Map() };
  let rankedShown = RANKED_PAGE;
  const rankedLoading = new Set<string>();
  const rankedHandlers = {
    open: (head: { world_id: string; season: number }) => {
      const key = seasonKey(head);
      if (rankedLoading.has(key)) return;
      rankedLoading.add(key);
      api.rankedSeason(head.world_id, head.season, model.player, 0, 10)
        .then((page) => { ranked.details.set(key, page); if (tab === 'seasons') drawRanked(true); })
        .catch(() => { /* next time it opens */ })
        .finally(() => rankedLoading.delete(key));
    },
    more: () => {
      rankedShown += RANKED_PAGE;
      void refreshRanked();
    },
  };
  // The whole panel is redrawn when the finished seasons change; the season
  // under way (pushed on every change) redraws only its own part.
  let rankedKey = '';
  let currentKey = '';
  function drawRanked(force = false): void {
    const key = JSON.stringify([ranked.current !== null, ranked.list, ranked.mine, ranked.details.size]);
    const cur = JSON.stringify(ranked.current);
    if (!force && key === rankedKey && tabs.seasons.childElementCount > 0) {
      if (cur !== currentKey && ranked.current && renderCurrentSeason(tabs.seasons, ranked.current, model.player)) currentKey = cur;
      return;
    }
    rankedKey = key;
    currentKey = cur;
    renderRanked(tabs.seasons, ranked, model.player, rankedHandlers);
  }
  /** Finished seasons, the player's history and (unless the socket pushes it) the season under way. */
  let rankedFetched = 0;
  async function refreshRanked(): Promise<void> {
    rankedFetched = Date.now();
    try {
      const pushed = watching && model.conn === 'open';
      const [current, list, mine] = await Promise.all([
        pushed ? Promise.resolve(ranked.current) : api.currentSeason(model.player, RANKED_ROWS),
        api.rankedSeasons(0, rankedShown), api.playerSeasons(model.player, 50),
      ]);
      Object.assign(ranked, { current: ranked.current && pushed ? ranked.current : current, list, mine: mine.seasons });
      if (tab === 'seasons') drawRanked();
    } catch {
      /* next time */
    }
  }
  // The standings under way come over the socket while the tab is open: one
  // shared copy on the server, pushed when it changes, instead of every open
  // tab polling for it.
  let watching = false;
  let rolloverTimer: ReturnType<typeof setTimeout> | undefined;
  function watchSeason(on: boolean): void {
    if (on === watching) return;
    watching = on;
    if (model.conn === 'open') void deps.send({ type: 'watch_season', on, limit: RANKED_ROWS }).catch(() => { /* resent on reconnect */ });
  }
  cleanups.push(
    // A new connection starts unwatched.
    model.on('welcome', () => {
      if (watching) void deps.send({ type: 'watch_season', on: true, limit: RANKED_ROWS }).catch(() => { /* next reconnect */ });
    }),
    model.on('season', () => {
      const before = ranked.current?.season;
      ranked.current = model.season;
      if (tab === 'seasons') drawRanked();
      // A rollover: the finished season and the player's places are new
      // (fetched at most every RANKED_ROLLOVER_MS, for very short seasons).
      if (before !== undefined && before !== model.season?.season && !rolloverTimer) {
        const wait = Math.max(0, rankedFetched + RANKED_ROLLOVER_MS - Date.now());
        rolloverTimer = setTimeout(() => { rolloverTimer = undefined; void refreshRanked(); }, wait);
      }
    }),
    () => clearTimeout(rolloverTimer),
  );
  const rankedTimer = setInterval(() => {
    if (tab === 'seasons') void refreshRanked();
  }, RANKED_REFRESH_MS);
  cleanups.push(() => clearInterval(rankedTimer));

  // Family tree: fetched while its tab is open, with its Share bar.
  let family: FamilyModel | null = null;
  function showFamily(m: FamilyModel): void {
    renderFamilyTree(tabs.family, m);
    const bar = familyShareBar(m, { world: deps.world ?? '', lineage: model.state?.lineage ?? model.welcomeLineage ?? null });
    if (bar) tabs.family.prepend(bar);
  }
  async function refreshFamily(): Promise<void> {
    try {
      family = buildFamilyTree(await api.familyTree(deps.token));
      exposeCards(family);
      if (tab === 'family') showFamily(family);
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) deps.onUnauthorized();
    }
  }
  const familyTimer = setInterval(() => {
    if (tab === 'family') void refreshFamily();
  }, FAMILY_REFRESH_MS);
  cleanups.push(() => clearInterval(familyTimer));

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
    // Its renderers are declared further down (see `ready`).
    if (!ready) return;
    if (now) {
      lastRender = performance.now();
      renderTopbar();
      if (tab === 'organism') renderInspector();
      if (tab === 'matings') renderMatings();
      if (tab === 'market') market.render();
      if (tab === 'lineage') renderLineage();
      if (tab === 'genetics') renderFeedPanel();
      if (tab === 'world' && !chronicle) renderWorld(tabs.world, null, model.player, 0);
      if (tab === 'family' && !family && !tabs.family.childElementCount) tabs.family.replaceChildren(h('p', { class: 'muted' }, 'Loading the family tree…'));
      if (tab === 'seasons') drawRanked();
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
    const hour = `${String(c.hour).padStart(2, '0')}:00`;
    const long = `Year ${c.year + 1} · day ${c.day + 1} · ${hour} · ${c.season} · ${c.daytime ? 'day' : 'night'}`;
    // Phones get the same facts in a form that fits the bar without an ellipsis.
    const short = `Y${c.year + 1} d${c.day + 1} ${hour} ${c.season} ${c.daytime ? '☀' : '☾'}`;
    if (clockEl.dataset.text !== long) {
      clockEl.dataset.text = long;
      clockEl.title = long;
      clockEl.replaceChildren(h('span', { class: 'clock-long' }, long), h('span', { class: 'clock-short', 'aria-hidden': 'true' }, short));
    }
    const w = s.weather;
    weatherEl.textContent = `wind ${pct(w.mean_wind, 0)}${w.storm ? ' · STORM' : ''} · drought ${pct(w.drought, 0)} · flood ${pct(w.flood, 0)}`;
    const banner = catastropheBanner(w.catastrophe, c.day);
    // The top bar keeps only a short pill on wide screens; the notice over the map carries the details.
    bannerEl.hidden = banner === null;
    bannerEl.textContent = banner === null ? '' : catastropheLabel(w.catastrophe!.kind);
    bannerEl.title = banner ?? '';
    const cat = w.catastrophe?.active ? w.catastrophe : null;
    catAlert.hidden = cat === null;
    if (cat) {
      // `end_day` is the first day after it: its last day is day `end_day` as the clock counts (from 1).
      const left = Math.max(0, cat.end_day - c.day);
      catTitle.textContent = `Catastrophe: ${catastropheLabel(cat.kind)}`;
      catLeft.textContent = left <= 1 ? '· last day' : `· ${left} days left (through day ${cat.end_day})`;
      catEffect.textContent = catastropheEffect(cat.kind);
      if (catSeen !== cat.id) {
        // Its start: a toast as well, unless world alerts already toast (the server's alert does).
        if (catSeen !== null || c.day - cat.start_day <= 1) {
          if (!model.prefs.toast.includes('world')) model.toast('info', `Catastrophe: ${catastropheLabel(cat.kind)} has begun. ${catastropheEffect(cat.kind)}`);
        }
        catSeen = cat.id;
        catAlert.classList.remove('folded');
        catFold.textContent = 'Less';
      }
    } else if (catSeen !== null && catSeen !== -1) {
      catSeen = -1;
    }
    // Toasts start below the notice, not under it.
    stageWrap.style.setProperty('--cat-h', cat ? `${catAlert.offsetHeight + 8}px` : '0px');
    const bal = balanceText(s.balance, s.escrow);
    if (balanceEl.title !== bal) {
      // Phones show the number alone (the CSS drops the word).
      balanceEl.replaceChildren(String(s.balance ?? 0), h('span', { class: 'pill-word' }, bal.slice(String(s.balance ?? 0).length)));
      balanceEl.title = bal;
      balanceEl.setAttribute('aria-label', bal);
    }
  }

  function renderConn(): void {
    const s = model.conn;
    connPill.className = `pill conn-${s}`;
    connPill.textContent = s === 'open' ? 'connected'
      : s === 'reconnecting' ? `reconnecting${model.retryInMs ? ` in ${(model.retryInMs / 1000).toFixed(1)} s` : '…'}`
      : s === 'connecting' ? 'connecting…' : 'offline';
    connPill.title = connPill.textContent;
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
  // Your organism's eyes: the numbers and the cones toggle (rebuilt only when they change).
  const inspVision = h('div', { id: 'inspector-vision' });
  const inspActions = h('div', { class: 'row wrap', id: 'inspector-actions' });
  append(tabs.organism, [inspDetails, inspVision, inspActions]);
  view.showVision = loadShowVision();

  function renderVision(own: OwnOrganism | undefined): void {
    const v = own?.vision;
    keyed(inspVision, own && v ? `${own.id}:${JSON.stringify(v)}:${view.showVision}` : 'none', () => {
      if (!own || !v) return [];
      const lines = visionLines(v);
      const toggle = h('input', { type: 'checkbox', id: 'vision-toggle' }) as HTMLInputElement;
      toggle.checked = view.showVision;
      toggle.addEventListener('change', () => {
        view.showVision = toggle.checked;
        saveShowVision(toggle.checked);
        renderVision(own);
      });
      const key = (cls: string, text: string) => h('span', { class: 'vision-key' }, h('i', { class: `vision-swatch ${cls}` }), text);
      return [
        h('h4', {}, 'Vision'),
        dl([
          ['View', lines.view], ['Depth', lines.depth], ['Blind behind', lines.blind],
          ...(lines.ahead ? [['Blind ahead', lines.ahead] as [string, string]] : []),
        ]),
        h('p', { class: 'muted small', id: 'vision-panel-summary' }, visionSummary(visionStats(v))),
        h('p', { class: 'muted small', id: 'vision-mechanic', 'data-directional': String(!!v.directional) }, lines.mechanic),
        h('label', { class: 'vision-toggle' }, toggle, h('span', {}, 'Show vision on the map')),
        view.showVision && h('p', { class: 'vision-legend small' },
          key('seen', 'seen by an eye'), key('depth', 'depth vision (two eyes)'), key('blind', 'blind spot')),
      ];
    });
  }

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
    renderVision(own);
    if (!own && !other) {
      keyed(inspActions, 'none', () => []);
      const tap = (narrow() || window.matchMedia('(pointer: coarse)').matches) ? 'Tap' : 'Click';
      const st = model.state;
      if (st && st.lineage !== null && st.population === 0) {
        // Kept, not rebuilt, on every state: its button must stay clickable.
        if (inspDetails.firstElementChild !== inspExtinct) inspDetails.replaceChildren(inspExtinct);
        return;
      }
      inspDetails.replaceChildren(h('p', { class: 'muted' }, model.selected === null
        ? `${tap} an organism on the map, or pick one from the Lineage tab.`
        : `#${model.selected} is no longer in view: it may have died or wandered out of sight. ${tap} another organism on the map, or pick one from the Lineage tab.`));
      return;
    }
    if (own) {
      const n = own.needs;
      inspDetails.replaceChildren(
        h('h3', {}, h('span', { class: 'swatch', style: `background:${cssColor(OWN_COLOR)}` }), `#${own.id} · yours`),
        dl([
          ['Action', actionText(own.action, own.needs)], ['Sex', own.sex], ['Age', `${own.age_days.toFixed(1)} days`],
          ['Stage', own.matured ? 'adult' : `juvenile (growth ${pct(own.growth, 0)})`],
          ['Health', meter(own.health, 100, 'good')], ['Lineage', `${own.lineage} · generation ${own.generation}`],
          ['Position', `${own.x.toFixed(1)}, ${own.y.toFixed(1)}`],
          ['Group', groupName(own.group ?? 0)],
          ['Body plan', own.look ? describeLook(own.look) : '—'],
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
          : h('button', { id: 'btn-possess', onclick: () => void possess(own.id) }, other ? `Possess (releases #${other.id})` : 'Possess'),
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
      dl([['Action', actionText(o.action)], ['Sex', o.sex], ['Stage', o.matured ? 'adult' : 'juvenile'], ['Lineage', String(o.lineage)],
        ['Position', `${o.x.toFixed(1)}, ${o.y.toFixed(1)}`], ['Body plan', o.look ? describeLook(o.look) : '—']]),
      h('p', { class: 'muted small' }, 'Only what can be seen: needs and health of other players\' organisms are hidden.'),
    );
    const poisonous = (model.state?.poisonous ?? []).includes(o.lineage);
    const hunter = possessed();
    // Orders reach 8 tiles (the server's `target_out_of_sight` beyond that).
    const reach = hunter ? Math.hypot(hunter.x - o.x, hunter.y - o.y) <= ATTACK_RANGE : false;
    keyed(inspActions, `other:${o.id}:${wild}:${hunter?.id ?? ''}:${reach}:${poisonous}`, () => [
      poisonous && h('p', { class: 'notice bad small', id: 'insp-poisonous' }, hunter
        ? 'This animal looks poisonous to you: your kind has been poisoned eating its kind.'
        : 'This animal looks poisonous: your kind has been poisoned eating its kind.'),
      !!hunter && h('button', {
        id: 'btn-attack', class: 'danger', disabled: !reach,
        title: reach ? '' : `#${hunter.id} is too far away: get within ${ATTACK_RANGE} tiles`,
        onclick: () => void attackWith(hunter, o.id),
      }, reach ? `Attack with #${hunter.id}` : `Attack with #${hunter.id} (too far)`),
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
    const mine = h('select', { id: 'mating-mine' }) as HTMLSelectElement;
    // The partner: picked from a list (yours and other players' visible
    // adults, nearest first); typing an id stays possible on a desktop.
    const partnerPick = h('select', { id: 'mating-partner-pick' }) as HTMLSelectElement;
    const partner = h('input', { id: 'mating-partner', type: 'number', min: 1, placeholder: 'or type an id' }) as HTMLInputElement;
    const price = h('input', { id: 'mating-price', type: 'number', min: 0, placeholder: 'optional' }) as HTMLInputElement;
    const submit = h('button', { id: 'mating-submit' }, 'Send request');
    const slots = h('p', { class: 'muted small' });
    // Compatibility of the chosen pair (server `preview_mating`), refreshed
    // when the pair changes and every few seconds (distance, condition).
    const compat = h('p', { id: 'mating-compat', class: 'small compat' });
    let picked = false;
    let previewSeq = 0;
    let previewAt = 0;
    let previewTimer: ReturnType<typeof setTimeout> | undefined;
    const ownById = (id: number) => model.state?.organisms.find((o) => o.id === id);
    const ownerName = (p: number) => model.state?.leaderboard.rows.find((r) => r.player === p)?.name ?? `player ${p}`;
    function pairContext(m: number, p: number): PairContext {
      const a = ownById(m);
      const b = ownById(p);
      const pregnant = a?.pregnant ? a.id : b?.pregnant ? b.id : null;
      return { ownPair: !!a && !!b, pregnant };
    }
    async function updateCompat(): Promise<void> {
      const m = Number(mine.value);
      const p = Number(partner.value);
      const seq = ++previewSeq;
      previewAt = Date.now();
      if (!m || !p) {
        compat.textContent = '';
        compat.dataset.level = '';
        compat.dataset.pair = '';
        return;
      }
      const r = await send({ type: 'preview_mating', mine: m, partner: p });
      if (seq !== previewSeq) return;
      compat.dataset.pair = `${m}:${p}`;
      if (r.ok) {
        const c = compatSummary(r.data as MatingPreviewView, pairContext(m, p));
        compat.textContent = c.text;
        compat.dataset.level = c.level;
      } else {
        compat.textContent = r.error ? `Can't preview: ${friendlyError(r.error)}` : '';
        compat.dataset.level = 'bad';
      }
    }
    const schedulePreview = (): void => {
      clearTimeout(previewTimer);
      previewTimer = setTimeout(() => void updateCompat(), 250);
    };
    mine.addEventListener('change', () => { picked = true; schedulePreview(); });
    partner.addEventListener('input', () => {
      partnerPick.value = [...partnerPick.options].some((o) => o.value === partner.value) ? partner.value : '';
      schedulePreview();
    });
    partnerPick.addEventListener('change', () => {
      if (partnerPick.value) partner.value = partnerPick.value;
      schedulePreview();
    });
    submit.addEventListener('click', async () => {
      const m = Number(mine.value);
      const p = Number(partner.value);
      if (!m || !p) return model.toast('error', 'Pick one of your adults and a partner.');
      const preg = pairContext(m, p).pregnant;
      if (preg) return model.toast('error', `#${preg} is already pregnant: pick another female, or wait until she gives birth.`);
      const msg: ClientMsg = { type: 'request_mating', mine: m, partner: p };
      if (price.value !== '') msg.price = Math.max(0, Math.floor(Number(price.value)));
      const r = await send(msg);
      if (!r.ok) {
        if (r.error) model.toast('error', friendlyError(r.error));
        return;
      }
      const d = r.data as { offer?: { id: number } } | MatingOutcome | undefined;
      if (d && 'offer' in d && d.offer) {
        model.toast('info', `Offer ${d.offer.id} sent; waiting for consent.`);
      } else {
        const outcome = d as MatingOutcome | undefined;
        model.addMating({ tick: model.state?.clock.tick ?? 0, mine: m, partner: p, outcome });
        model.toast('info', outcome?.conceived ? `#${m} × #${p}: conceived.` : `#${m} × #${p}: mated, but not conceived.`);
      }
    });
    function refresh(s: StateView): void {
      const adults = s.organisms.filter((o) => o.matured).sort((a, b) => a.id - b.id);
      // Options are updated in place, so an open list stays open and the choice stays put.
      reconcile(mine, adults, (o) => String(o.id), (o) => h('option', { value: o.id }) as HTMLOptionElement,
        (opt, o) => setText(opt, `#${o.id} ${o.sex}${o.pregnant ? ' (pregnant)' : ''} · gen ${o.generation}`));
      if (!picked) {
        // Nothing chosen yet: not a pregnant female (she can't conceive).
        const first = adults.find((o) => !o.pregnant) ?? adults[0];
        if (first && mine.value !== String(first.id)) {
          mine.value = String(first.id);
          schedulePreview();
        }
      }
      const me = ownById(Number(mine.value));
      const opposite = (sex: string) => !me || sex !== me.sex;
      const here = me ? { x: me.x, y: me.y } : view.cam;
      const dist = (o: { x: number; y: number }) => Math.hypot(o.x - here.x, o.y - here.y);
      // Every own adult of the other sex, wherever it is (the lineage is all
      // known); a pregnant female is listed but can't be picked. Other
      // players' animals: those the server sends (in or near the view).
      const ownPartners = adults.filter((o) => o.id !== me?.id && opposite(o.sex))
        .map((o) => ({ id: o.id, label: `#${o.id} ${o.sex} (yours)${o.pregnant ? ' · pregnant' : ''} · ${dist(o).toFixed(0)} tiles`, d: dist(o), off: o.pregnant }));
      const others = (s.nearby ?? []).filter((o) => o.matured && o.owner !== WILD && o.owner !== model.player && opposite(o.sex))
        .map((o) => ({ id: o.id, label: `#${o.id} ${o.sex} · ${ownerName(o.owner)} · ${dist(o).toFixed(0)} tiles`, d: dist(o), off: false }));
      const open = [...ownPartners, ...others].filter((c) => !c.off).length;
      const first = open > 0 ? 'Choose a partner…'
        : ownPartners.length > 0 ? 'No partner free now (pregnant)'
          : 'No partner: none of yours of the other sex, none of other players\' in view';
      const choices = [{ id: 0, label: first, d: -1, off: false },
        // Pickable first (nearest first), then the pregnant ones.
        ...[...ownPartners, ...others].sort((a, b) => Number(a.off) - Number(b.off) || a.d - b.d).slice(0, 40)];
      if (document.activeElement !== partnerPick) {
        reconcile(partnerPick, choices, (c) => String(c.id), (c) => h('option', { value: c.id || '' }) as HTMLOptionElement,
          (opt, c) => {
            setText(opt, c.label);
            opt.disabled = c.off;
          });
        partnerPick.value = [...partnerPick.options].some((o) => o.value === partner.value) ? partner.value : '';
      }
      if (Date.now() - previewAt > 3000 && partner.value !== '') void updateCompat();
      slots.textContent = `${s.slots_left} directed-mating slot${s.slots_left === 1 ? '' : 's'} left this season.`;
    }
    const el = h('div', { class: 'form' },
      h('label', {}, 'My adult', mine),
      h('label', {}, 'Partner', partnerPick),
      h('div', { class: 'row' }, h('label', { class: 'partner-id' }, 'Partner id', partner), h('label', {}, 'Price', price)),
      compat,
      h('div', { class: 'row' }, submit), slots,
      h('p', { class: 'muted small' }, `Pick a partner from the list, or on the map ("Request mating…"). Within your own lineage the pair mates at once when within ${MEETING_TILES} tiles; another player must agree, then the two must meet.`));
    return {
      el,
      refresh,
      setMine: (id: OrgId) => { picked = true; mine.value = String(id); schedulePreview(); },
      setPartner: (id: OrgId) => { partner.value = String(id); partnerPick.value = [...partnerPick.options].some((o) => o.value === String(id)) ? String(id) : ''; schedulePreview(); },
    };
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
    if (!r.ok) view.rejectMove();
    if (r.ok) model.toast('info', `#${mine} is walking to #${partner}.`);
  }

  /** When open offers lapse: at the next change of season (game and real time). */
  function offerTime(): string {
    const tick = model.state?.clock.tick;
    if (tick === undefined || !daysPerYear) return 'Open offers lapse when the season changes.';
    return `Lapses when the season changes, in ${span(ticksToNextSeason(tick, ticksPerDay, daysPerYear))}.`;
  }

  function renderMatings(): void {
    const s = model.state;
    if (s) matingForm.refresh(s);
    const all = [...model.offers.values()].sort((a, b) => b.offer.id - a.offer.id);
    const me = model.player;
    const incoming = all.filter((e) => e.offer.to_player === me && e.offer.status === 'pending');
    const open = all.filter((e) => (e.offer.from_player === me || (e.offer.to_player === me && e.offer.status === 'accepted')) && ['pending', 'accepted'].includes(e.offer.status));
    const closed = all.filter((e) => !['pending', 'accepted'].includes(e.offer.status));
    // Rows are kept by offer id and updated in place (oldest first, new ones
    // below), so Accept / Decline never move under a click.
    const none = (el: HTMLElement, text: string, empty: boolean) => {
      const ph = el.previousElementSibling as HTMLElement | null;
      if (ph?.classList.contains('offers-none')) ph.hidden = !empty;
      else el.before(h('p', { class: 'muted small offers-none', hidden: !empty }, text));
    };
    const inc = [...incoming].sort((a, b) => a.offer.id - b.offer.id);
    none(incomingEl, 'None.', inc.length === 0);
    reconcile(incomingEl, inc, (e) => String(e.offer.id), (e) => {
      const id = e.offer.id;
      return h('div', { class: 'offer', 'data-offer': id }, h('span', { class: 'offer-line' }), h('small', { class: 'muted offer-time' }),
        h('div', { class: 'row' },
          h('button', { class: 'accept', onclick: () => void send({ type: 'respond_mating', request: id, accept: true }) }, 'Accept'),
          h('button', { class: 'secondary decline', onclick: () => void send({ type: 'respond_mating', request: id, accept: false }) }, 'Decline')));
    }, (row, e) => {
      setText(row.querySelector('.offer-line')!, offerLine(e.offer));
      setText(row.querySelector('.offer-time')!, offerTime());
    });
    const opn = [...open].sort((a, b) => a.offer.id - b.offer.id);
    none(openEl, 'None.', opn.length === 0);
    reconcile(openEl, opn, (e) => `${e.offer.id}:${e.offer.status}`, (e) => {
      const mineId = e.offer.from_player === me ? e.offer.mine : e.offer.partner;
      const accepted = e.offer.status === 'accepted';
      return h('div', { class: 'offer', 'data-offer': e.offer.id }, h('span', { class: 'offer-line' }), h('small', { class: 'muted offer-time' }),
        accepted && h('small', { class: 'muted offer-note' }),
        h('div', { class: 'row' },
          accepted && h('button', { onclick: () => void send({ type: 'complete_mating', request: e.offer.id }) }, 'Complete now'),
          accepted && h('button', { class: 'secondary', title: 'Possess your organism and walk it to the partner (if the partner is in sight)', onclick: () => void escort(mineId, e.offer.from_player === me ? e.offer.partner : e.offer.mine) }, `Escort #${mineId}`),
          h('button', { class: 'secondary', onclick: () => void send({ type: 'cancel_mating', request: e.offer.id }) }, 'Cancel')));
    }, (row, e) => {
      setText(row.querySelector('.offer-line')!, offerLine(e.offer));
      setText(row.querySelector('.offer-time')!, offerTime());
      const note = row.querySelector<HTMLElement>('.offer-note');
      if (note) {
        const mineId = e.offer.from_player === me ? e.offer.mine : e.offer.partner;
        setText(note, `Agreed${e.error ? ` (${friendlyError(e.error)})` : ''}: escort #${mineId} to the partner; it completes when they meet.`);
      }
    });
    const done = historyRows(closed, model.matingLog);
    none(closedEl, 'None yet.', done.length === 0);
    reconcile(closedEl, done, (e) => e.key, (e) => h('div', { class: `offer closed status-${e.status}` },
      h('span', { class: 'offer-line' }), h('small', { class: 'offer-outcome' }), h('small', { class: 'error offer-error' })),
    (row, e) => {
      setText(row.querySelector('.offer-line')!, e.line);
      setText(row.querySelector('.offer-outcome')!, e.outcome ?? '');
      setText(row.querySelector('.offer-error')!, e.error ?? '');
    });
  }

  // --- Market: renown, the stud marketplace and the ledger
  const market = buildMarket({
    model,
    send,
    msPerTick,
    onOrganism: (id, x, y) => {
      model.select(id);
      view.setFollow(false);
      if (x !== undefined && y !== undefined) view.centerOn(x, y);
      showTab('organism');
    },
  });
  tabs.market.append(market.el);
  const marketTimer = setInterval(() => {
    if (tab === 'market') void market.refresh();
  }, MARKET_REFRESH_MS);
  cleanups.push(() => clearInterval(marketTimer));

  // --- Genetics dashboard, alerts feed and alert preferences
  const dashEl = h('div', { id: 'dashboard' });
  const feedEl = h('div', { id: 'alerts-feed', class: 'feed' });
  const prefsEl = h('div', { id: 'alert-prefs-body' });
  const prefsBox = h('details', { class: 'chart-card', id: 'alert-prefs' },
    h('summary', {}, 'Alert preferences'),
    h('p', { class: 'muted small' }, 'Saved to your account. Hidden kinds stay out of the feed and the badge; switch one back on to see its recent alerts.'),
    prefsEl);
  const push = pushSettings({ token: deps.token, player: model.player, onUnauthorized: deps.onUnauthorized, toast: (k, t) => model.toast(k, t) });
  cleanups.push(push.destroy);
  append(tabs.genetics, [dashEl, h('h3', {}, 'Alerts'), prefsBox, push.el, feedEl]);

  function renderPrefs(): void {
    prefsEl.replaceChildren(renderAlertPrefs(model.prefs, (p) => void savePrefs(p)));
  }

  async function savePrefs(p: AlertPrefs): Promise<void> {
    const before = model.prefs;
    model.setPrefs(p);
    renderPrefs();
    try {
      model.setPrefs(await api.setAlertPrefs(deps.token, model.prefs));
    } catch (e) {
      model.setPrefs(before);
      if (e instanceof ApiError && e.status === 401) return deps.onUnauthorized();
      model.toast('error', `Alert preferences not saved: ${(e as Error).message}`);
    }
    renderPrefs();
  }
  renderPrefs();
  void (async () => {
    try {
      model.setPrefs(await api.alertPrefs(deps.token));
      renderPrefs();
    } catch {
      /* keep the defaults */
    }
  })();

  function renderGenetics(): void {
    renderDashboard(dashEl, stats);
    renderFeedPanel();
  }

  function renderFeedPanel(): void {
    const feed = model.visibleFeed();
    seenFeed = feed.length;
    alertBadge.hidden = true;
    keyed(feedEl, `${feed.length}|${feed[feed.length - 1]?.text ?? ''}|${model.prefs.hidden.join()}`, () => [renderFeed(feed, (id) => {
      model.select(id);
      showTab('organism');
    })]);
  }

  function onAlerts(): void {
    if (tab === 'genetics') return renderFeedPanel();
    const unseen = model.visibleFeed().length - seenFeed;
    alertBadge.hidden = unseen <= 0;
    alertBadge.textContent = unseen > 9 ? '9+' : String(unseen);
  }

  // --- Lineage, leaderboards
  const statsEl = h('div', { id: 'lineage-stats' });
  const boardEl = h('div', { id: 'leaderboard' });
  const seasonsEl = h('div', { id: 'season-boards' });
  const listEl = h('div', { id: 'organism-list', class: 'org-list' });
  const listMore = h('p', { class: 'muted small', id: 'organism-list-more' });
  const groupsEl = h('div', { id: 'group-list' });
  const retireBtn = h('button', { type: 'button', id: 'retire-lineage', class: 'danger' }, 'Retire lineage');
  const retireEl = h('details', { id: 'lineage-retire', class: 'retire' },
    h('summary', {}, 'Retire this lineage'),
    h('p', { class: 'muted small' },
      'Start over: every animal and egg of your lineage leaves the world at once (they are removed, not killed and not left wild), ',
      'its orders end, and you can design and found a new lineage straight away. Your renown, records and titles stay. ',
      'A new lineage\'s legacy share counts toward the ranked season gradually over its first days.'),
    retireBtn);
  retireEl.hidden = true;
  retireBtn.addEventListener('click', async () => {
    const lineage = model.state?.lineage;
    if (lineage === null || lineage === undefined) return;
    if (!window.confirm(`Retire lineage #${lineage}? All ${model.state?.population ?? 0} of its animals leave the world. This can't be undone.`)) return;
    const r = await send({ type: 'retire_lineage', lineage });
    if (r.ok) {
      const n = (r.data as { removed?: number } | undefined)?.removed ?? 0;
      model.toast('info', `Lineage #${lineage} retired: ${n} animals left the world. You can found a new lineage.`);
      retireEl.open = false;
    } else if (r.error) {
      model.toast('error', `Retire: ${r.error.message}`);
    }
  });
  append(tabs.lineage, [lineageExtinct, statsEl,
    h('h3', { title: 'Split a herd or pack into groups with their own orders: migrate one, have another hold a territory' }, 'Groups'), groupsEl,
    h('h3', {}, 'Legacy leaderboard'), boardEl, h('h3', { title: 'Legacy standings per spring, summer, autumn and winter of the world\'s year (the Ranks tab has the ranked seasons, a competition of several weeks)' }, 'By time of year'), seasonsEl,
    h('h3', {}, 'My organisms'), listEl, listMore, retireEl]);

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
            h('td', {}, h('span', { class: 'swatch', style: `background:${cssColor(r.player === model.player ? OWN_COLOR : ownerColor(r.player))}` }), h('span', { class: 'who', title: r.name }, r.name)),
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
    const cap = capInfo(s.population, worldCap);
    // With level of detail on, the dashboard's counts include the cohorts far from players.
    const fresh = stats && stats.lineage === s.lineage && stats.alive === s.population;
    const adultsAll = fresh ? stats!.adults : adults;
    const youngAll = fresh ? stats!.juveniles : orgs.length - adults;
    const maxGen = orgs.reduce((m, o) => Math.max(m, o.generation), 0);
    statsEl.replaceChildren(dl([
      ['Lineage', s.lineage === null ? 'not founded' : `#${s.lineage}`],
      ['Alive', h('span', {}, h('strong', { id: 'lineage-size' }, cap.count),
        cap.status ? h('span', { class: `cap-status cap-${cap.level}`, id: 'cap-status' }, ` — ${cap.status}`) : null,
        s.population > orgs.length
          ? h('span', { class: 'muted small', title: 'Level of detail: members far from any player run as statistical cohorts and are not drawn' }, ` · ${orgs.length} on the map`)
          : null)],
      ['Adults / young', h('span', { id: 'adults-young', title: 'Young count toward the population cap too' }, `${grouped(adultsAll)} adults / ${grouped(youngAll)} young`)],
      ['Pregnant', String(orgs.filter((o) => o.pregnant).length)],
      ['Newest generation', String(maxGen)],
      ['Mating slots left', h('span', { id: 'slots-left' }, String(s.slots_left))],
      ['Migration', migrationText(s.migration, s.groups)],
    ]));
    if (cap.explain) statsEl.append(h('p', { class: `cap-note small${cap.level === 'ok' ? ' muted' : ''}`, id: 'cap-note' }, cap.explain));
    // A founded lineage with nobody left: say so plainly, and where to see why.
    // One element, shown or hidden (rebuilding it each state would eat clicks on its button).
    lineageExtinct.hidden = !(s.lineage !== null && s.population === 0);
    retireEl.hidden = !(s.lineage !== null && s.population > 0);
    renderGroups(s);
    const lb = s.leaderboard;
    keyed(boardEl, JSON.stringify(lb), () => lb.rows.length === 0
      ? [h('p', { class: 'muted small' }, lb.day === null ? 'The first legacy snapshot comes at the end of day 1.' : 'No breeding adults.')]
      : [h('p', { class: 'muted small' }, `Day ${(lb.day ?? 0) + 1} snapshot · ${lb.breeding_adults} breeding adults`),
        h('table', { class: 'board' },
          h('thead', {}, h('tr', {}, h('th', {}, 'Player'), h('th', {}, 'Share'), h('th', { title: 'Weighted by offspring raised to adulthood' }, 'Weighted'), h('th', {}, 'Alive'))),
          h('tbody', {}, ...lb.rows.map((r) => h('tr', { class: r.player === model.player ? 'me' : '' },
            h('td', {}, h('span', { class: 'swatch', style: `background:${cssColor(r.player === model.player ? OWN_COLOR : ownerColor(r.player))}` }), h('span', { class: 'who', title: r.name }, r.name)),
            h('td', {}, h('span', { class: 'share-bar', style: `--w:${(r.share * 100).toFixed(1)}%` }, pct(r.share))),
            h('td', {}, pct(r.weighted_share)), h('td', {}, String(r.alive))))))]);
    const shown = [...orgs].sort((a, b) => Number(b.possessed) - Number(a.possessed) || Number(b.matured) - Number(a.matured) || a.id - b.id).slice(0, 60);
    // One button per organism, kept and updated in place (no row moves under a click).
    reconcile(listEl, shown, (o) => String(o.id), (o) => {
      const id = o.id;
      return h('button', { class: 'org-item', 'data-org': id, onclick: () => {
        const cur = model.state?.organisms.find((x) => x.id === id);
        model.select(id);
        view.setFollow(false);
        if (cur) view.centerOn(cur.x, cur.y);
        showTab('organism');
      } });
    }, (btn, o) => {
      btn.classList.toggle('possessed', o.possessed);
      setText(btn, `#${o.id} ${o.sex === 'female' ? 'F' : 'M'}${o.matured ? '' : ' juv'}${o.pregnant ? ' preg' : ''} · ${actionText(o.action, o.needs)}`);
    });
    setText(listMore, orgs.length > shown.length ? `…and ${orgs.length - shown.length} more` : '');
  }

  function renderGroups(s: StateView): void {
    const groups = groupsOf(s);
    const tick = s.clock.tick;
    // The cards (and their buttons) are rebuilt only when the buttons
    // change; sizes and orders are updated in place, so a button isn't
    // replaced under the cursor at every birth.
    const key = JSON.stringify([groupChoice, groups.map((g) => [g.id, !!g.territory, !!g.rally, claimWait(g, tick) > 0, g.members === 0, canSplit(g, groups), !!g.home_range, g.protection_left_min])]);
    keyed(groupsEl, key, () => groups.map((g) => {
      const wait = claimWait(g, tick);
      const swatch = h('span', { class: 'swatch', style: `background:${cssColor(groupColor(g.id))}` });
      const home = homeRangeNote(g);
      return h('div', { class: `group-card${groupChoice === g.id ? ' chosen' : ''}`, 'data-group': g.id },
        h('div', { class: 'row wrap' }, swatch, h('strong', {}, groupName(g.id)),
          home && h('span', { class: 'home-badge', title: home }, '⌂ home range'),
          h('span', { class: 'muted small group-summary' })),
        home && h('p', { class: 'muted small home-range' }, home),
        h('div', { class: 'row wrap group-actions' },
          h('button', { class: 'secondary small', 'data-action': 'pick', onclick: () => setGroupChoice(g.id) }, groupChoice === g.id ? 'Picked' : 'Pick'),
          h('button', { class: 'secondary small', 'data-action': 'territory', title: wait > 0 ? 'Claimed recently: wait a little' : `Radius up to ${g.max_radius.toFixed(1)} tiles`,
            disabled: wait > 0 || g.members === 0, onclick: () => { setGroupChoice(g.id); setMode('territory'); if (narrow()) setSheet(false); } }, 'Claim territory…'),
          g.territory && h('button', { class: 'secondary small', 'data-action': 'abandon', onclick: () => void send({ type: 'abandon_territory', group: g.id }) }, 'Abandon territory'),
          h('button', { class: 'secondary small', 'data-action': 'rally', title: 'Gather the group at a point you tap; it holds there for a few days',
            disabled: g.members === 0, onclick: () => { setGroupChoice(g.id); setMode('rally'); if (narrow()) setSheet(false); } }, 'Rally…'),
          g.rally && h('button', { class: 'secondary small', 'data-action': 'cancel-rally', title: `Lapses on its own in ${span(rallyLeft(g, tick))}`,
            onclick: () => void send({ type: 'cancel_rally', group: g.id }) }, 'Cancel rally'),
          canSplit(g, groups) && h('button', { class: 'secondary small', 'data-action': 'split', onclick: () => void splitGroup(g.id) }, 'Split'),
          g.id !== 0 && h('button', { class: 'secondary small', 'data-action': 'merge', onclick: () => void send({ type: 'merge_groups', from: g.id, into: 0 }) }, 'Merge into A'),
          (g.territory || g.rally) && h('button', { class: 'secondary small', 'data-action': 'center', onclick: () => {
            const x = groupsOf(model.state).find((x) => x.id === g.id);
            const t = x?.territory ?? x?.rally;
            if (t) { view.setFollow(false); view.centerOn(t.x, t.y); }
          } }, 'Center')));
    }));
    for (const g of groups) {
      const el = groupsEl.querySelector(`.group-card[data-group="${g.id}"] .group-summary`);
      const heed = groupObedience(g);
      const lapse = g.rally ? ` (lapses in ${span(rallyLeft(g, tick))})` : '';
      const text = `${g.members} (${g.adults} adults) · ${groupOrders(g)}${lapse}${heed ? ` · ${heed}` : ''}`;
      if (el && el.textContent !== text) el.textContent = text;
    }
  }

  function renderToasts(): void {
    clear(toasts);
    for (const t of model.toasts.slice(-4)) append(toasts, [h('div', { class: `toast ${t.kind}` }, t.text, t.count ? h('span', { class: 'toast-more' }, ` (and ${t.count - 1} more like it)`) : null)]);
  }
  let toastTimer: ReturnType<typeof setTimeout> | null = null;

  // The map is drawn from the state; a drawing error must not stop the
  // panels (or the connection pill) from following it.
  function showState(): void {
    try {
      view.setState(model.state!);
    } catch (e) {
      console.error('[game] drawing the state failed:', e);
    }
  }

  cleanups.push(
    model.on('state', () => {
      showState();
      renderGroupMenu();
      updateHint();
      renderPanels();
    }),
    model.on('select', () => { view.selected = model.selected; renderFab(); renderPanels(); }),
    model.on('offers', () => renderPanels()),
    model.on('conn', renderConn),
    model.on('alerts', onAlerts),
    model.on('toast', () => {
      renderToasts();
      if (toastTimer) clearTimeout(toastTimer);
      toastTimer = setTimeout(() => { model.toasts = []; renderToasts(); }, 6000);
    }),
  );
  if (model.state) {
    showState();
    renderGroupMenu();
  }
  renderConn();
  updateHint();
  ready = true;
  el.classList.remove('starting');
  // The tab the player picked while the screen was loading, else Lineage.
  showTab(tab);

  // First-session onboarding: the guided tour (once per account on this
  // device), one-time hints and the Help dialog. `?tour=off` turns it off.
  const touchInput = () => narrow() || window.matchMedia('(pointer: coarse)').matches;
  const onboarding = createOnboarding({
    model, host: el, hintHost: stageWrap, helpBtn, narrow, touch: touchInput,
    openTab: (t, sheet) => {
      if (sheet && narrow()) setSheet(true);
      showTab(t);
    },
    selectOwn: () => {
      const orgs = model.state?.organisms ?? [];
      if (orgs.length === 0 || orgs.some((o) => o.id === model.selected)) return;
      model.select((orgs.find((o) => o.matured) ?? orgs[0]).id);
    },
    disabled: onboardingDisabled(window.location.search),
  });
  cleanups.push(
    onboarding.destroy,
    model.on('state', onboarding.check),
    model.on('alerts', onboarding.check),
    model.on('offers', onboarding.check),
  );

  return {
    el,
    view,
    showTab: (t: string) => {
      if (!(t in tabs)) return;
      if (narrow()) setSheet(true);
      showTab(t as Tab);
    },
    destroy: () => {
      for (const c of cleanups) c();
      if (toastTimer) clearTimeout(toastTimer);
      view.destroy();
    },
  };
}

const SHOW_VISION_KEY = 'evo.showVision';

/** Whether vision cones are shown (remembered per browser; on by default). */
function loadShowVision(): boolean {
  try {
    return localStorage.getItem(SHOW_VISION_KEY) !== '0';
  } catch {
    return true;
  }
}

function saveShowVision(on: boolean): void {
  try {
    localStorage.setItem(SHOW_VISION_KEY, on ? '1' : '0');
  } catch {
    /* private mode: not remembered */
  }
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

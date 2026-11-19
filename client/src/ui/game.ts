// The play screen: world view (tile map), top bar (clock, weather,
// connection) and a sidebar with the organism inspector, matings, the
// genetics dashboard with the alerts feed, the lineage/leaderboards tab, the
// ranked seasons (hall of fame) and the world chronicle (species, eras,
// catastrophes, achievements).

import { NeedBand, bandText } from '../needBand';
import { commandError } from '../errors';
import { DEFAULT_TICKS_PER_DAY, gameAndReal, ticksToNextSeason } from './duration';
import { reconcile, setText } from './reconcile';
import { friendlyError, MEETING_TILES } from './friendlyErrors';
import { historyRows, meetText, offerLine } from './matingHistory';
import { api, ApiError } from '../api';
import { alertEntry, type GameModel } from '../model';
import { buildFamilyTree, type FamilyModel } from '../familyTree';
import { actionText, capInfo, graceText, grouped, LEGACY_EXPLAIN, migrationText, rampText } from '../lineageInfo';
import { droughtNotice } from '../droughtInfo';
import { markRetired, wasRetired } from '../retired';
import { awaySummary, seenKey } from '../awaySummary';
import { renderFamilyTree } from './familyTree';
import { exposeCards, familyShareBar } from '../share/familyShare';
import type { AlertPrefs, ClientMsg, CompatibilityView, LineageStats, PlayerId, MatingOutcome, MatingPreviewView, NearbyOrganism, OfferView, OrgId, OwnOrganism, ResultMsg, SeasonBoard, SpeciesView, StateView } from '../protocol';
import { parseCompatibilityView, WILD } from '../protocol';
import { createPicker, type PickItem } from './picker';
import { confirmDialog, textDialog } from './confirm';
import { nameplatesKey, parseNameplates, plateBadge, setNameplates } from './nameplates';
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
  canSplit, claimWait, clearMigrationMsg, groupCenter, groupColor, groupName, groupObedience, groupOptions, groupOrders, groupsOf, migrateMsgs, nextViewGroup, parseChoice,
  homeRangeNote, MAX_GROUPS, rallyLeft, rallyMsg, targetGroup, territoryTap, validChoice, type GroupChoice, type TerritoryDraft,
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
  /** Logged in to a lineage they already had (not straight from founding): no first-session tour. */
  returning?: boolean;
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
    h('p', { class: 'extinct-text' }, h('strong', {}, 'Your lineage has died out.'), ' None of your organisms are alive. The Genes tab charts what killed them.'),
    onNewLineage ? h('p', {}, h('button', { type: 'button', class: 'new-lineage', id: `${id}-new`, onclick: onNewLineage }, 'Start a new lineage'),
      h('span', { class: 'muted small' }, ' Your renown, records and titles stay.')) : null);
}
/** The dynamic terrain layers change every simulated hour. */
const TERRAIN_REFRESH_MS = 4000;
/** The "Here:" chooser goes after this long, or at the next touch of the map (playtest 3: it stayed 8 s and caught the next press). */
const CHOOSER_MS = 5000;
const STATS_REFRESH_MS = 5000;
/** How often the list of protected newcomers (`/api/world`) is re-read. */
const PROTECTED_REFRESH_MS = 30000;
const SEASONS_REFRESH_MS = 15000;
const CHRONICLE_REFRESH_MS = 10000;
/** Bought nameplates change rarely. */
const NAMEPLATES_REFRESH_MS = 60000;
/**
 * The seasons tab's fallback refresh. The standings under way are pushed over the socket while the tab is open
 * (`watch_season`); finished seasons only change at a rollover, which a push shows.
 */
const RANKED_REFRESH_MS = 30000;
/** Least time between two fetches of the finished seasons after rollovers. */
const RANKED_ROLLOVER_MS = 5000;
/** Rows per category of the season under way. */
const RANKED_ROWS = 8;
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
  /** Players whose lineage is a protected newcomer, with the days left (`/api/world`). */
  const protectedPlayers = new Map<PlayerId, number>();
  const setProtected = (list: { player: PlayerId; days_left: number }[] | undefined) => {
    protectedPlayers.clear();
    for (const p of list ?? []) protectedPlayers.set(p.player, p.days_left);
  };
  /** The genetics dashboard's data (declared up here: the Lineage tab reads it too, possibly before its fetch is set up). */
  let stats: LineageStats | null = null;
  /** Open Help on a topic (set once onboarding is up). */
  let helpTopic: ((topic: string) => void) | null = null;
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
  const balanceEl = h('button', { class: 'pill balance-pill', id: 'balance', title: 'Renown: earned from your legacy share, spent in the Market (stud fees, reinforcements, slots): tap to open it', onclick: () => { setSheet(true); showTab('market'); } });
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
  const clearMigBtn = tool('clear-migration', 'clear', 'Clear', 'Cancel the orders: migration, territory and rally (the group picked in the group menu, or all)');
  // Group commands: pick a group, claim ground for it, split it.
  const groupSelect = h('select', { class: 'tool group-select', id: 'group-select', title: 'Which of your groups the group commands act on', 'aria-label': 'Group' });
  const territoryBtn = tool('territory', 'territory', 'Territory…', 'Claim a territory for the group: tap its center, then its edge');
  const rallyBtn = tool('rally', 'rally', 'Rally…', 'Gather the group at a point: tap where; it holds there until the rally lapses (at least 5 minutes; the group card shows when)');
  const splitBtn = tool('split-group', 'split', 'Split', 'Split the group in two (the far half across its widest spread)');
  const pinBtn = tool('pin-view', 'pin', 'Pin view', 'Pin the server view window (what you can see of others) to the camera center');
  const pinLabel = pinBtn.querySelector('.tool-label')!;
  // A second group out of the lineage's view window sees nothing around it:
  // this moves the window from group to group (and keeps it on the group as it moves).
  const viewGroupBtn = tool('view-group', 'eye', 'View group', 'Move the view window (what you can see of others) to each of your groups in turn, then back to the whole lineage');
  const viewGroupLabel = viewGroupBtn.querySelector('.tool-label')!;
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
    h('div', { class: 'tool-group', role: 'group', 'aria-label': 'View window' }, viewGroupBtn, pinBtn));
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
    item(swatch('pip', '--c:#7ee787'), 'pregnant / gravid', 'Carrying young: pregnant, or gravid (eggs coming) in an egg layer'),
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
  // Folded or not, remembered on this device (playtest 3: unfolded again at every load, 90 px of a phone's map).
  function foldable(alert: HTMLElement, fold: HTMLElement, key: string): (folded: boolean, remember: boolean) => void {
    const set = (folded: boolean, remember: boolean) => {
      alert.classList.toggle('folded', folded);
      fold.textContent = folded ? 'More' : 'Less';
      fold.setAttribute('aria-expanded', String(!folded));
      if (remember) {
        try { localStorage.setItem(key, folded ? 'folded' : 'open'); } catch { /* storage may be blocked */ }
      }
    };
    let stored: string | null = null;
    try { stored = localStorage.getItem(key); } catch { /* storage may be blocked */ }
    set(stored === 'folded', false);
    fold.addEventListener('click', () => set(!alert.classList.contains('folded'), true));
    return set;
  }
  foldable(catAlert, catFold, 'evo.notice.catastrophe');
  let catSeen: number | null = null;
  // A drought (much of the land dry): the same kind of notice, in amber, under it.
  const droughtTitle = h('strong', { class: 'cat-title' });
  const droughtAdvice = h('span', { class: 'cat-effect' });
  const droughtFold = h('button', { class: 'secondary cat-fold', type: 'button', 'aria-expanded': 'true', title: 'Fold or unfold the drought notice' }, 'Less');
  const droughtAlert = h('div', { class: 'catastrophe-alert drought-alert', id: 'drought-alert', role: 'status', hidden: true },
    h('span', { class: 'cat-icon', 'aria-hidden': 'true' }, '☀'), h('div', { class: 'cat-text' }, droughtTitle, droughtAdvice), droughtFold);
  const foldDrought = foldable(droughtAlert, droughtFold, 'evo.notice.drought');
  /** Both notices up: the drought one starts folded (two expanded covered a third of a phone's map, playtest 4). */
  let bothNotices = false;
  let droughtOn = false;
  // The lineage died out (or was retired): said over the map, whichever tab
  // is open (playtest 6: only the Lineage tab said so, seen after a fresh login).
  const extinctTitle = h('strong', { class: 'cat-title' });
  const extinctText = h('span', { class: 'cat-effect' });
  const extinctAlert = h('div', { class: 'catastrophe-alert extinct-alert', id: 'extinct-alert', role: 'alert', hidden: true },
    h('span', { class: 'cat-icon', 'aria-hidden': 'true' }, '⚠'), h('div', { class: 'cat-text' }, extinctTitle, extinctText),
    deps.onNewLineage ? h('button', { type: 'button', class: 'new-lineage', id: 'extinct-alert-new', onclick: deps.onNewLineage }, 'Start a new lineage') : null);
  function renderExtinct(): void {
    const st = model.state;
    const gone = !!st && st.lineage !== null && st.population === 0;
    extinctAlert.hidden = !gone;
    if (!gone) return;
    const retired = wasRetired(st!.lineage);
    const title = retired ? `You retired lineage #${st!.lineage}: start a new lineage` : 'Your lineage died out: start a new lineage';
    if (extinctTitle.textContent !== title) {
      extinctTitle.textContent = title;
      extinctText.textContent = retired
        ? 'Its animals have left the world. Your renown, records and titles stay.'
        : 'None of your animals are alive (the Genes tab charts what killed them). Your renown, records and titles stay.';
    }
  }
  const notices = h('div', { class: 'map-notices', id: 'map-notices' }, extinctAlert, catAlert, droughtAlert);
  // Phones: a tapped animal's compact card (who, what it does, how it fares)
  // over the map, without opening the panel (playtest 7: nothing showed until
  // the Animal tab was opened). Details opens the full inspector.
  const peekTitle = h('div', { class: 'peek-title' });
  const peekLine = h('div', { class: 'peek-line muted small' });
  const peekDetails = h('button', { class: 'secondary small', id: 'peek-details', type: 'button' }, 'Details');
  const peekClose = h('button', { class: 'secondary small peek-close', id: 'peek-close', type: 'button', 'aria-label': 'Close' }, '×');
  const peek = h('div', { class: 'peek', id: 'peek', hidden: true, role: 'status' },
    h('div', { class: 'peek-text' }, peekTitle, peekLine), peekDetails, peekClose);
  const stageWrap = h('div', { class: 'stage-wrap' }, stage, toolbar, notices, hint, legends, fab, peek, possessBar, chooser, toasts);
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
    // (Shown as "Ranks": its ids say so too, `tab-ranks` / `panel-ranks`.)
    seasons: h('div', { class: 'panel', id: 'panel-ranks' }),
    world: h('div', { class: 'panel', id: 'panel-world' }),
  };
  const tabBtns = (Object.keys(tabs) as Tab[]).map((t) =>
    h('button', { class: 'tab', 'data-tab': t, id: `tab-${t === 'seasons' ? 'ranks' : t}`, title: TAB_TITLES[t], onclick: () => tabClicked(t) }, TAB_LABELS[t]));
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
    renderPeek();
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
    if (t === 'genetics' || t === 'lineage') void refreshStats();
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
    // Split only where it can work (playtest 3: enabled at the four-group limit).
    const full = groups.filter((x) => x.members > 0).length >= MAX_GROUPS;
    const tg = targetGroup(groupChoice, groups);
    const g = tg === null ? undefined : groups.find((x) => x.id === tg);
    const can = !full && (g === undefined || canSplit(g, groups));
    if (splitBtn.disabled === can) {
      splitBtn.disabled = !can;
      splitBtn.title = full ? `You have ${MAX_GROUPS} groups, the most: merge two (Lineage tab) to split again`
        : can ? 'Split the group in two (the far half across its widest spread)' : 'Too few animals in this group to split';
    }
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
      : model.state?.view.pinned && viewGroup !== null ? `The view window is on ${groupName(viewGroup)}: other animals show only around it. View group (map tools) moves it on; Unpin view follows your whole lineage again.`
      : model.state?.view.pinned ? `The view is pinned at (${model.state.view.x.toFixed(0)}, ${model.state.view.y.toFixed(0)}): other animals show only there. Unpin view (map tools) to follow your lineage again.`
      : touch ? 'Tap an organism to inspect it, long-press yours to possess. Drag to pan, pinch to zoom.'
      : 'Click an organism to inspect it (hold to possess yours). Drag to pan, wheel to zoom.';
    // The possession bar says it all while possessing.
    hint.hidden = !!p && mode === 'select';
    renderFab();
    renderPeek();
    renderPossessBar();
    updateEdgePad();
  }

  // The animal last possessed: when it dies the bar says so for a while
  // instead of vanishing silently (playtest 2).
  let lastPossessed: OrgId | null = null;
  let endedTimer: ReturnType<typeof setTimeout> | undefined;
  function renderPossessBar(): void {
    const p = possessed();
    if (!p && lastPossessed !== null) {
      const gone = lastPossessed;
      lastPossessed = null;
      // Still among yours: released (by you, or a new possession). Gone: it died.
      if (model.state && !model.state.organisms.some((o) => o.id === gone)) {
        clearTimeout(endedTimer);
        possessBar.classList.add('ended');
        delete possessBar.dataset.org;
        setText(possessText, `#${gone} died while you were possessing it (the alert says how). Possess another to carry on.`);
        possessBar.hidden = false;
        endedTimer = setTimeout(() => {
          possessBar.classList.remove('ended');
          if (!possessed()) possessBar.hidden = true;
        }, 8000);
      }
    }
    if (p) {
      // Possessing from the chooser's pick: the chooser goes (it lay over the bar's buttons, playtest 5).
      if (lastPossessed !== p.id && !chooser.hidden) hideChooser();
      lastPossessed = p.id;
      clearTimeout(endedTimer);
      possessBar.classList.remove('ended');
    }
    if (!possessBar.classList.contains('ended')) possessBar.hidden = !p;
    // Hint cards move clear of the bar (onboarding.css).
    stageWrap.classList.toggle('possessing', !!p);
    if (p && possessBar.offsetHeight > 0) stageWrap.style.setProperty('--possess-h', `${possessBar.offsetHeight}px`);
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
    // A refusal is toasted by the model (every failed result is), once.
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
    stageWrap.classList.remove('choosing');
  }
  // Any touch of the map (a tap, a press, a drag, a pinch, the wheel) puts the chooser away.
  for (const ev of ['pointerdown', 'wheel'] as const) stage.addEventListener(ev, () => { if (!chooser.hidden) hideChooser(); }, { capture: true, passive: true });
  /** Several animals under a finger: chips to pick another than the one chosen. */
  function showChooser(ids: OrgId[], chosen: OrgId): void {
    const s = model.state;
    const label = (id: OrgId) => {
      const own = s?.organisms.find((o) => o.id === id);
      if (own) return `#${id} ${own.sex === 'female' ? '♀' : '♂'} yours`;
      const n = s?.nearby?.find((o) => o.id === id);
      return `#${id} ${n?.owner === WILD ? 'wild' : 'rival'}`;
    };
    // One compact row (it scrolls sideways if need be) that goes at the next touch of the map.
    chooser.replaceChildren(h('span', { class: 'muted small' }, 'Here:'),
      ...ids.slice(0, 6).map((id) => h('button', {
        type: 'button', class: `chip${id === chosen ? ' active' : ''}`, 'data-org': id,
        onclick: () => { model.select(id); hideChooser(); if (!narrow() || sheetOpen) showTab('organism'); },
      }, label(id))),
      h('button', { type: 'button', class: 'chip close', 'aria-label': 'Close', onclick: hideChooser }, '×'));
    chooser.hidden = false;
    stageWrap.classList.add('choosing');
    clearTimeout(chooserTimer);
    chooserTimer = setTimeout(hideChooser, CHOOSER_MS);
  }

  /**
   * The camera may go past the map's top edge by what floats over it (the
   * toolbar, and the notices under it): a herd at the north edge comes out
   * from under them and can be tapped (playtest 3).
   */
  function updateEdgePad(): void {
    const wrap = stageWrap.getBoundingClientRect();
    let bottom = toolbar.getBoundingClientRect().bottom;
    if (!notices.hidden && (!catAlert.hidden || !droughtAlert.hidden || !extinctAlert.hidden)) bottom = Math.max(bottom, notices.getBoundingClientRect().bottom);
    const top = Math.max(0, Math.round(bottom - wrap.top + 6));
    // And past the bottom edge by what floats there: the hint line, the
    // floating Possess button, the possession bar (playtest 4: a herd at the
    // bottom edge sat under them).
    let low = wrap.bottom;
    for (const el of [hint, fab, peek, possessBar]) {
      const r = el.getBoundingClientRect();
      if (r.height > 0 && r.top < wrap.bottom) low = Math.min(low, r.top);
    }
    const bottomPad = Math.max(0, Math.round(wrap.bottom - low + 6));
    if (view.edgePad.top !== top || view.edgePad.bottom !== bottomPad) view.edgePad = { ...view.edgePad, top, bottom: bottomPad };
  }
  window.addEventListener('resize', updateEdgePad);
  cleanups.push(() => window.removeEventListener('resize', updateEdgePad));

  /** The phone card for the selected animal (`peek`): shown on narrow screens while the sheet is down. */
  function renderPeek(): void {
    const s = model.state;
    const id = model.selected;
    const own = id !== null ? s?.organisms.find((o) => o.id === id) : undefined;
    const other = id !== null && !own ? s?.nearby.find((o) => o.id === id) : undefined;
    // A possessed animal has the possession bar.
    const show = narrow() && !sheetOpen && mode === 'select' && !!(own || other) && !own?.possessed;
    if (peek.hidden === show) {
      peek.hidden = !show;
      stageWrap.classList.toggle('peeking', show);
      queueMicrotask(updateEdgePad);
    }
    if (!show) return;
    const o = (own ?? other)!;
    const look = o.look ? describeLook(o.look).split(' · ')[0] : '';
    const whose = own ? 'yours' : other!.owner === WILD ? 'wild' : 'rival';
    const title = `#${o.id} ${o.sex === 'female' ? '♀' : '♂'} ${whose}${look ? ` · ${look}` : ''}`;
    const stage = o.matured ? 'adult' : 'young';
    const pct = (v: number) => String(Math.round(v));
    const line = own
      ? `${stage} · ${actionText(own.action, own.needs)} · health ${pct(own.health)} · hunger ${pct(own.needs.hunger)} · thirst ${pct(own.needs.thirst)}`
      : `${stage} · ${actionText(o.action)}${other!.protected ? ' · protected' : ''}`;
    if (peekTitle.textContent !== title) peekTitle.textContent = title;
    if (peekLine.textContent !== line) peekLine.textContent = line;
    peek.dataset.org = String(o.id);
    // The floating Possess button sits just above the card.
    stageWrap.style.setProperty('--peek-h', `${Math.ceil(peek.getBoundingClientRect().height)}px`);
  }
  peekDetails.addEventListener('click', () => { setSheet(true); showTab('organism'); });
  peekClose.addEventListener('click', () => model.select(null));

  /** The floating Possess / Release button for the selected own organism. */
  function renderFab(): void {
    const sel = model.selected !== null ? model.state?.organisms.find((o) => o.id === model.selected) : undefined;
    // A possessed animal's Release is on the possession bar: not a second one here (playtest 6).
    const own = sel && !sel.possessed ? sel : undefined;
    if (fab.hidden === !!own) queueMicrotask(updateEdgePad); // shown or hidden: the room at the bottom edge changes
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
      // (No toast: the possession bar says who and how to walk; playtest 3 found the two saying the same.)
      // But do say when it let another go: one at a time (playtest 4).
      const notice = (r.data as { notice?: string } | undefined)?.notice;
      if (notice) model.toast('info', notice);
    }
    return r.ok;
  }

  view.onFollowChange = (f) => followBtn.classList.toggle('active', f);
  followBtn.classList.add('active');
  followBtn.addEventListener('click', () => view.setFollow(!view.follow));
  migrateBtn.addEventListener('click', () => setMode(mode === 'migrate' ? 'select' : 'migrate'));
  clearMigBtn.addEventListener('click', async () => {
    // Every standing order of the picked group (or all): the migration,
    // a territory and a rally.
    const groups = groupsOf(model.state).filter((g) => groupChoice === 'all' || g.id === groupChoice);
    const done: string[] = [];
    if (groups.some((g) => g.migration)) done.push('migration');
    await send(clearMigrationMsg(groupChoice));
    for (const g of groups) {
      if (g.territory && (await send({ type: 'abandon_territory', group: g.id })).ok) done.push(`${groupName(g.id)}'s territory`);
      if (g.rally && (await send({ type: 'cancel_rally', group: g.id })).ok) done.push(`${groupName(g.id)}'s rally`);
    }
    model.toast('info', done.length > 0 ? `Cleared: ${done.join(', ')}` : 'No orders to clear');
  });
  let pinned = false;
  /** The lineage the screen shows (undefined before the first state). */
  let shownLineage: number | null | undefined;
  /** The button as the server has it (the state's `view.pinned`): a view pinned by an API call or before a reload shows too (playtest 6). */
  function setPinned(on: boolean): void {
    // Unpinned (by the button, a retire or another tab): no group is viewed.
    if (!on) setViewGroup(null);
    if (on === pinned && pinBtn.classList.contains('active') === on) return;
    pinned = on;
    pinLabel.textContent = pinned ? 'Unpin view' : 'Pin view';
    pinBtn.classList.toggle('active', pinned);
  }
  pinBtn.addEventListener('click', async () => {
    const r = pinned ? await send({ type: 'clear_view' }) : await send({ type: 'set_view', x: view.cam.x, y: view.cam.y, radius: 24 });
    if (r.ok) {
      setViewGroup(null);
      setPinned(!pinned);
    }
  });
  /** The group the view window is kept on (View group), or null. */
  let viewGroup: number | null = null;
  let viewGroupBusy = false;
  function setViewGroup(id: number | null): void {
    viewGroup = id;
    viewGroupLabel.textContent = id === null ? 'View group' : `Viewing ${groupName(id).replace(/^Group /, '').replace(' (main)', '')}`;
    viewGroupBtn.classList.toggle('active', id !== null);
  }
  function renderViewGroup(): void {
    const s = model.state;
    viewGroupBtn.disabled = viewGroup === null && nextViewGroup(s, null) === null;
    viewGroupBtn.title = viewGroupBtn.disabled
      ? 'View group: split your lineage into groups (Split) to look around each in turn'
      : 'Move the view window (what you can see of others) to each of your groups in turn, then back to the whole lineage';
  }
  /** Points the window at `id`'s middle; false if refused. */
  async function pinOnGroup(id: number): Promise<boolean> {
    const c = groupCenter(model.state, id);
    if (!c) return false;
    viewGroupBusy = true;
    try {
      const r = await send({ type: 'set_view', x: c.x, y: c.y, radius: 24 });
      return r.ok;
    } finally {
      viewGroupBusy = false;
    }
  }
  viewGroupBtn.addEventListener('click', async () => {
    const next = nextViewGroup(model.state, viewGroup);
    if (next === null) {
      if ((await send({ type: 'clear_view' })).ok) {
        setViewGroup(null);
        setPinned(false);
        model.toast('info', 'The view window follows your whole lineage again');
      }
      return;
    }
    if (await pinOnGroup(next)) {
      setViewGroup(next);
      setPinned(true);
      view.setFollow(true);
      model.toast('info', `Viewing ${groupName(next)}: the view window stays on it as it moves`);
    }
    updateHint();
  });
  /** Keeps the window on the viewed group as it moves (re-pinned once it drifts 4 tiles). */
  function trackViewGroup(): void {
    const s = model.state;
    if (viewGroup === null || viewGroupBusy || !s?.view.pinned) return;
    const c = groupCenter(s, viewGroup);
    if (!c) {
      setViewGroup(null);
      return;
    }
    if (Math.hypot(c.x - s.view.x, c.y - s.view.y) > 4) void pinOnGroup(viewGroup);
  }
  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'Escape') setMode('select');
  };
  window.addEventListener('keydown', onKey);
  cleanups.push(() => window.removeEventListener('keydown', onKey));

  // A group order's reply says what becomes of its brooding parents.
  const broodingToast = (data: unknown) => {
    const note = (data as { brooding_note?: string } | undefined)?.brooding_note;
    if (note) model.toast('info', note);
  };
  view.onClick = async (c) => {
    if (mode === 'migrate') {
      setMode('select');
      const msgs = migrateMsgs(groupChoice, groupsOf(model.state), c.x, c.y);
      const rs = await Promise.all(msgs.map((m) => send(m)));
      // The server echoes the target it used: on land, possibly moved off water.
      const r = rs.find((x) => x.ok) ?? rs[0];
      const t: { x: number; y: number; note?: string; warning?: string } = (r?.data as { x: number; y: number; note?: string; warning?: string } | undefined) ?? c;
      const who = groupChoice === 'all' ? 'Adults' : `${groupName(groupChoice)} adults`;
      if (r?.ok) {
        model.toast('info', `${who} migrating toward (${t.x.toFixed(0)}, ${t.y.toFixed(0)})${t.note ? `: ${t.note}` : ''}`);
        if (t.warning) model.toast('warn', `Migrating, but: ${t.warning}`);
        broodingToast(r.data);
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
        const t = r.data as { x: number; y: number; radius: number; warning?: string; note?: string };
        model.toast('info', `${groupName(id)} gathering at (${t.x.toFixed(0)}, ${t.y.toFixed(0)}), within ${t.radius.toFixed(1)} tiles${t.note ? ` (${t.note})` : ''}`);
        if (t.warning) model.toast('warn', `Rallying, but: ${t.warning}`);
        broodingToast(r.data);
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
        const t = r.data as { x: number; y: number; radius: number; warning?: string; note?: string; pending?: boolean };
        if (t.pending) model.toast('info', `${groupName(g.id)} is migrating to (${t.x.toFixed(0)}, ${t.y.toFixed(0)}): ${t.note ?? 'it claims the territory when it arrives'}`);
        else model.toast('info', `${groupName(g.id)} holds a territory of radius ${t.radius.toFixed(1)} at (${t.x.toFixed(0)}, ${t.y.toFixed(0)})${t.note ? ` (${t.note})` : ''}`);
        if (t.warning) model.toast('warn', `${t.pending ? 'Ordered' : 'Claimed'}, but: ${t.warning}`);
        broodingToast(r.data);
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
      // A tap on overlapping animals picks the one under the finger (the
      // nearest: a tap right on a wild animal in a herd of yours inspects it,
      // playtest 3) and offers the others, yours first.
      const pick = c.organism;
      model.select(pick);
      if (c.touch && (c.candidates?.length ?? 0) > 1) showChooser([pick, ...c.candidates!.filter((id) => id !== pick)], pick);
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
  // The selected animal's hunger over the last day (it swings with each meal).
  const hungerBand = new NeedBand();
  const msPerTick = () => view.tickClock.msPerTick;
  const span = (ticks: number) => gameAndReal(ticks, ticksPerDay, msPerTick());
  try {
    const w = await api.world();
    worldCap = w.lineage_cap;
    setProtected(w.protected);
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
  // Protected newcomers (players' lineages in their grace), for the inspector: re-read now and then.
  const protectedTimer = setInterval(() => {
    void api.world().then((w) => setProtected(w.protected)).catch(() => {});
  }, PROTECTED_REFRESH_MS);
  cleanups.push(() => clearInterval(protectedTimer));

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
      if (tab === 'lineage') renderLineage();
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) deps.onUnauthorized();
    }
  }
  void refreshStats();
  const statsTimer = setInterval(() => {
    if (tab === 'genetics' || tab === 'lineage') void refreshStats();
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
      if (tab === 'world') renderWorld(tabs.world, chronicle, model.player, model.state?.clock.day ?? species.day, nameSpecies);
    } catch {
      /* next time */
    }
  }
  /** Name a species of yours (`name_species`, a renown cosmetic). */
  async function nameSpecies(sp: SpeciesView): Promise<void> {
    // The price always shows, and what you have when it is more (playtest 7:
    // it was hidden when short); a refusal stays in the dialog to fix the name.
    const price = await market.shopPrice('species_name_price');
    const have = model.state?.balance;
    const short = price !== undefined && have !== undefined && have < price;
    const cost = price === undefined ? '' : `; costs ${price} renown${short ? ` (you have ${Math.floor(have!)})` : ''}`;
    let named = '';
    const name = await textDialog({
      id: 'species-name', title: `A name for "${sp.name}"`, value: sp.name, maxLength: 40, ok: price ? `Name it (${price} renown)` : 'Name it',
      body: `1 to 40 characters${cost}. A species is named once.`,
      submit: async (v) => {
        const t = v.trim();
        if (t === '') return 'Type a name (1 to 40 characters).';
        if (t === sp.name) return 'That is its name already: type a new one.';
        const r = await send({ type: 'name_species', species: sp.id, name: t });
        if (r.ok) {
          named = String(r.data ?? t);
          return null;
        }
        return r.error ? commandError('name_species', r.error).replace(/^[^:]*: /, '') : "That didn't work.";
      },
    });
    if (name !== null && named) {
      model.toast('info', `Species ${sp.id} is now called "${named}".`);
      void refreshChronicle();
    }
  }
  // Bought nameplates: shown next to names (Ranks, hall of fame, leaderboard, a rival's inspector).
  async function refreshNameplates(): Promise<void> {
    const before = nameplatesKey();
    const r = await send({ type: 'nameplates' });
    const m = r.ok ? parseNameplates(r.data) : null;
    if (!m) return;
    setNameplates(m);
    if (nameplatesKey() !== before) {
      if (tab === 'seasons') drawRanked(true);
      renderPanels(true);
    }
  }
  const platesTimer = setInterval(() => void refreshNameplates(), NAMEPLATES_REFRESH_MS);
  cleanups.push(() => clearInterval(platesTimer));
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
      renderExtinct();
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
        // (Folded stays folded, as the player chose: the one line still names it and the days left.)
      }
    } else if (catSeen !== null && catSeen !== -1) {
      catSeen = -1;
    }
    const dn = droughtNotice(w.drought, droughtOn, cat?.kind === 'megadrought', c.season);
    if (dn.on !== droughtOn) {
      droughtOn = dn.on;
      droughtAlert.hidden = !dn.on;
    }
    const both = !!cat && dn.on;
    if (both && !bothNotices) foldDrought(true, false);
    bothNotices = both;
    if (dn.on) {
      if (droughtTitle.textContent !== dn.title) droughtTitle.textContent = dn.title;
      if (droughtAdvice.textContent !== dn.advice) droughtAdvice.textContent = dn.advice;
    }
    // Toasts start below the notices, not under them.
    stageWrap.style.setProperty('--cat-h', cat || dn.on ? `${notices.offsetHeight + 8}px` : '0px');
    updateEdgePad();
    const bal = balanceText(s.balance, s.escrow);
    if (balanceEl.title !== bal) {
      // The number, then the word (a separate span, so the text can be styled apart).
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
      hungerBand.add(own.id, model.state?.clock.tick ?? 0, n.hunger, ticksPerDay);
      const band = bandText(hungerBand.band(own.id, ticksPerDay));
      inspDetails.replaceChildren(
        h('h3', {}, h('span', { class: 'swatch', style: `background:${cssColor(OWN_COLOR)}` }), `#${own.id} · yours`, model.player !== null ? plateBadge(model.player) : null),
        dl([
          ['Action', actionText(own.action, own.needs)], ['Sex', own.sex], ['Age', `${own.age_days.toFixed(1)} days`],
          ['Stage', own.matured ? 'adult' : `juvenile (growth ${pct(own.growth, 0)})`],
          ['Health', meter(own.health, 100, 'good')], ['Lineage', `${own.lineage} · generation ${own.generation}`],
          ['Position', `${own.x.toFixed(1)}, ${own.y.toFixed(1)}`],
          ['Group', groupName(own.group ?? 0)],
          ['Body plan', own.look ? describeLook(own.look) : '—'],
          ['Status', [own.possessed && 'possessed', own.pregnant && (laysEggs() ? 'gravid (about to lay)' : 'pregnant')].filter(Boolean).join(', ') || '—'],
        ]),
        h('h4', {}, 'Needs'),
        dl([['Hunger', band ? h('span', {}, meter(n.hunger, 100, 'bad'), h('span', { class: 'muted small', id: 'insp-hunger-band' }, ` ${band}`)) : meter(n.hunger, 100, 'bad')],
          ['Thirst', meter(n.thirst, 100, 'bad')], ['Energy', meter(n.energy, 100, 'good')],
          ['Threat', meter(n.safety, 100, 'bad')], ['Too cold', meter(n.cold, 20, 'bad', '°')], ['Too hot', meter(n.heat, 20, 'bad', '°')],
          ['Breeding urge', meter(n.breeding, 100, 'neutral')], ['Social', meter(n.social, 100, 'neutral')]]),
      );
      const other = possessed();
      keyed(inspActions, `own:${own.id}:${own.possessed}:${other?.id ?? ''}`, () => [
        own.possessed
          // One Release, on the possession bar (playtest 6: two of them while possessing).
          ? h('span', { class: 'muted small', id: 'btn-release' }, 'Possessed: Release is on the bar.')
          : h('button', { id: 'btn-possess', onclick: () => void possess(own.id) }, other ? `Possess (releases #${other.id})` : 'Possess'),
        h('button', { class: 'secondary', onclick: () => { view.setFollow(false); view.centerOn(own.x, own.y); } }, 'Center'),
        own.matured && h('button', { class: 'secondary', onclick: () => { matingForm.setMine(own.id); showTab('matings'); } }, 'Mate this one…'),
      ]);
      return;
    }
    const o = other!;
    const wild = o.owner === WILD;
    const ownerName = wild ? 'wild' : model.state?.leaderboard.rows.find((r) => r.player === o.owner)?.name ?? `player ${o.owner}`;
    // The neighbour's own flag (session B) or the world's list of protected newcomers.
    const flagged = o.protected === true;
    const graceDays = wild ? undefined : protectedPlayers.get(o.owner) ?? (flagged ? 0 : undefined);
    inspDetails.replaceChildren(
      h('h3', {}, h('span', { class: 'swatch', style: `background:${cssColor(ownerColor(o.owner))}` }), `#${o.id} · ${ownerName}`, wild ? null : plateBadge(o.owner),
        graceDays !== undefined ? h('span', { class: 'pill protected-pill', id: 'insp-protected', title: 'A newcomer\'s lineage: other players\' animals can\'t attack it yet, nor it theirs' }, 'protected newcomer') : null),
      dl([['Action', actionText(o.action)], ['Sex', o.sex], ['Stage', o.matured ? 'adult' : 'juvenile'], ['Lineage', String(o.lineage)],
        ['Position', `${o.x.toFixed(1)}, ${o.y.toFixed(1)}`], ['Body plan', o.look ? describeLook(o.look) : '—'],
        ...(graceDays !== undefined ? [['Protected', graceDays > 0 ? `newcomer grace, ${span(graceDays * ticksPerDay)} left: other players' animals can't attack it, nor it theirs` : 'newcomer grace: other players\' animals can\'t attack it, nor it theirs'] as [string, string]] : [])]),
      h('p', { class: 'muted small' }, 'Only what can be seen: needs and health of other players\' organisms are hidden.'),
    );
    const poisonous = (model.state?.poisonous ?? []).includes(o.lineage);
    const hunter = possessed();
    // Orders reach 8 tiles (the server's `target_out_of_sight` beyond that).
    const reach = hunter ? Math.hypot(hunter.x - o.x, hunter.y - o.y) <= ATTACK_RANGE : false;
    const guarded = graceDays !== undefined;
    const burrowed = o.burrowed === true;
    // A rival lineage known not to breed with yours (playtest 4: "Request mating…" was offered anyway).
    const sterile = !wild && matingForm.sterileWith(o.lineage);
    keyed(inspActions, `other:${o.id}:${wild}:${hunter?.id ?? ''}:${reach}:${poisonous}:${guarded}:${burrowed}:${sterile}`, () => [
      burrowed && h('p', { class: 'muted small', id: 'insp-burrowed' }, 'Down in its burrow: it can\'t be attacked on orders until it comes out, and hunters mostly can\'t reach it.'),
      poisonous && h('p', { class: 'notice bad small', id: 'insp-poisonous' }, hunter
        ? 'This animal looks poisonous to you: your kind has been poisoned eating its kind.'
        : 'This animal looks poisonous: your kind has been poisoned eating its kind.'),
      !!hunter && h('button', {
        id: 'btn-attack', class: 'danger', disabled: guarded || burrowed || !reach,
        title: guarded ? 'A protected newcomer: it can\'t be attacked on orders until its grace ends'
          : burrowed ? 'In its burrow: wait for it to come out'
          : reach ? '' : `#${hunter.id} is too far away: get within ${ATTACK_RANGE} tiles`,
        onclick: () => void attackWith(hunter, o.id),
      }, burrowed ? `Attack with #${hunter.id} (in its burrow)` : guarded || reach ? `Attack with #${hunter.id}` : `Attack with #${hunter.id} (too far)`),
      wild ? h('p', { class: 'muted small' }, 'Wild organisms have no owner to consent to a mating.')
        : sterile ? h('p', { class: 'muted small', id: 'insp-sterile' }, 'Its lineage can\'t breed with yours (body plans too far apart): no mating to ask for.')
        : h('button', { id: 'btn-ask-mating', onclick: () => { matingForm.setPartner(o.id); showTab('matings'); } }, 'Request mating…'),
      h('button', { class: 'secondary', onclick: () => { view.setFollow(false); view.centerOn(o.x, o.y); } }, 'Center'),
    ]);
  }

  // --- Matings
  const matingForm = buildMatingForm();
  const incomingEl = h('div', { id: 'incoming' });
  const openEl = h('div', { id: 'open-offers' });
  const closedEl = h('div', { id: 'closed-offers' });
  // Who may send me requests (server `mating_prefs`): decline all, or block players.
  let matePrefs: { blocked: number[]; auto_decline: boolean } | null = null;
  const autoDecline = h('input', { type: 'checkbox', id: 'mating-auto-decline' }) as HTMLInputElement;
  const blockedEl = h('div', { id: 'mating-blocked', class: 'blocked-list' });
  const matePrefsEl = h('details', { id: 'mating-prefs', class: 'mating-prefs' },
    h('summary', {}, 'Who may send me requests'),
    h('label', { class: 'check' }, autoDecline, 'Decline every request automatically'),
    h('p', { class: 'muted small' }, 'Listings and auctions are not affected: a listing is your consent. Blocked players\' requests are declined at once; they are not told why.'),
    h('h4', {}, 'Blocked players'), blockedEl);
  async function sendPrefs(msg: { block?: number; unblock?: number; auto_decline?: boolean }): Promise<void> {
    const r = await send({ type: 'mating_prefs', ...msg });
    if (r.ok) {
      const d = r.data as { blocked?: number[]; auto_decline?: boolean } | undefined;
      if (d && Array.isArray(d.blocked)) matePrefs = { blocked: d.blocked, auto_decline: !!d.auto_decline };
    } else if (r.error) {
      model.toast('error', `Request settings: ${r.error.message}`);
    }
    renderMatePrefs();
  }
  autoDecline.addEventListener('change', () => void sendPrefs({ auto_decline: autoDecline.checked }));
  function blockPlayer(p: number): void {
    const name = model.state?.leaderboard.rows.find((r) => r.player === p)?.name ?? `player ${p}`;
    void confirmDialog({ id: 'block-confirm', title: `Block ${name}?`, ok: 'Block', danger: true,
      body: 'Their mating requests will be declined automatically. You can unblock them under "Who may send me requests".' }).then(async (yes) => {
      if (!yes) return;
      await sendPrefs({ block: p });
      model.toast('info', `${name} blocked: their requests are declined automatically.`);
    });
  }
  function renderMatePrefs(): void {
    if (!matePrefs) {
      setText(blockedEl, 'Loading…');
      return;
    }
    autoDecline.checked = matePrefs.auto_decline;
    const names = (p: number) => model.state?.leaderboard.rows.find((r) => r.player === p)?.name ?? `player ${p}`;
    if (matePrefs.blocked.length === 0) {
      blockedEl.replaceChildren(h('p', { class: 'muted small' }, 'Nobody. Block a player from one of their requests.'));
      return;
    }
    if (blockedEl.querySelector('p.muted')) blockedEl.replaceChildren();
    reconcile(blockedEl, matePrefs.blocked, (p) => String(p), (p) => h('div', { class: 'row blocked-row', 'data-player': p },
      h('span', { class: 'who' }), h('button', { class: 'secondary small unblock', onclick: () => void sendPrefs({ unblock: p }) }, 'Unblock')),
    (row, p) => setText(row.querySelector('.who')!, names(p)));
  }
  matePrefsEl.addEventListener('toggle', () => { if (matePrefsEl.open && !matePrefs) void sendPrefs({}); });

  append(tabs.matings, [
    h('h3', {}, 'Request a directed mating'), matingForm.el,
    h('h3', {}, 'Incoming requests'), incomingEl, matePrefsEl,
    h('h3', {}, 'My open offers'), openEl,
    h('h3', {}, 'Recent outcomes'), closedEl,
  ]);

  function buildMatingForm() {
    // Searchable pickers, not <select>s: a lineage of thousands made "My
    // adult" unusable (above all on a phone). Nearest first, filters for sex,
    // free and not pregnant, search by id.
    const mineP = createPicker({ id: 'mating-mine', label: 'My adult', placeholder: 'Search your adults in view: id or words', filters: ['sex', 'free', 'notPregnant'], noneLabel: 'Choose one of your adults…' });
    const mine = mineP.el;
    // The partner: picked from a list (yours and other players' visible
    // adults, nearest first); typing an id stays possible.
    const partnerP = createPicker({ id: 'mating-partner-pick', label: 'Partner', placeholder: 'Search partners: id, owner or words', filters: ['free', 'notPregnant'], noneLabel: 'Choose a partner…' });
    const partnerPick = partnerP.el;
    const partner = h('input', { id: 'mating-partner', type: 'number', min: 1, placeholder: 'or type an id' }) as HTMLInputElement;
    const price = h('input', { id: 'mating-price', type: 'number', min: 0, placeholder: 'optional' }) as HTMLInputElement;
    const submit = h('button', { id: 'mating-submit' }, 'Send request');
    // The pair must meet: possess "My adult" and walk it to the partner (the
    // control the "too far apart" message points to).
    const walk = h('button', { id: 'mating-walk', class: 'secondary', type: 'button', title: 'Possess your adult and walk it to the partner\'s last seen position', disabled: true }, 'Walk to the partner') as HTMLButtonElement;
    walk.addEventListener('click', () => {
      const m = Number(mine.value);
      const p = Number(partner.value);
      if (m && p) void escort(m, p);
    });
    const slots = h('p', { class: 'muted small' });
    const partnerNote = h('p', { class: 'muted small', id: 'mating-partner-note' });
    // Compatibility of the chosen pair (server `preview_mating`), refreshed
    // when the pair changes and every few seconds (distance, condition).
    const compat = h('p', { id: 'mating-compat', class: 'small compat' });
    let picked = false;
    let previewSeq = 0;
    let previewAt = 0;
    let previewTimer: ReturnType<typeof setTimeout> | undefined;
    // Which lineages can breed with yours (server `compatibility`): others'
    // animals of a sterile lineage are listed but marked and not pickable.
    let breeds: CompatibilityView | null = null;
    let breedsAt = 0;
    let breedsLoading = false;
    async function loadBreeds(): Promise<void> {
      if (breedsLoading) return;
      breedsLoading = true;
      breedsAt = Date.now();
      try {
        const r = await send({ type: 'compatibility' });
        if (r.ok) breeds = parseCompatibilityView(r.data) ?? breeds;
      } finally {
        breedsLoading = false;
      }
      if (model.state) refresh(model.state);
    }
    const sterileWith = (lineage: number): boolean => {
      if (lineage === model.state?.lineage) return false;
      const row = breeds?.rows.find((r) => r.lineage === lineage);
      return !!row && !row.compatible;
    };
    const ownById = (id: number) => model.state?.organisms.find((o) => o.id === id);
    const ownerName = (p: number) => model.state?.leaderboard.rows.find((r) => r.player === p)?.name ?? `player ${p}`;
    /** Own organisms promised in an open or agreed offer: not free now. */
    const promised = (): Set<number> => {
      const ids = new Set<number>();
      for (const e of model.offers.values()) {
        if (e.offer.status !== 'pending' && e.offer.status !== 'accepted') continue;
        ids.add(e.offer.mine);
        ids.add(e.offer.partner);
      }
      return ids;
    };
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
    // A new "My adult": the partner list (other sex, distances from it) is
    // rebuilt at once, not at the next state.
    mine.addEventListener('change', () => {
      picked = true;
      if (model.state) refresh(model.state);
      schedulePreview();
    });
    partner.addEventListener('input', () => {
      partnerP.set(partner.value === '' ? null : Number(partner.value));
      schedulePreview();
    });
    partnerPick.addEventListener('change', () => {
      partner.value = partnerPick.value;
      walk.disabled = !mine.value || partner.value === '';
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
      if (s.lineage !== null && !breedsLoading && Date.now() - breedsAt > 60_000) void loadBreeds();
      const adults = s.organisms.filter((o) => o.matured);
      const busy = promised();
      const center = view.cam;
      const camDist = (o: { x: number; y: number }) => Math.hypot(o.x - center.x, o.y - center.y);
      mineP.setItems(adults.map((o): PickItem => ({
        id: o.id,
        label: `#${o.id} ${o.sex} · gen ${o.generation}`,
        detail: [o.pregnant && 'pregnant', busy.has(o.id) && 'promised', o.possessed && 'possessed'].filter(Boolean).join(', ') || undefined,
        sex: o.sex, pregnant: o.pregnant, free: !busy.has(o.id) && !o.pregnant, distance: camDist(o),
      })));
      if (!picked) {
        // Nothing chosen yet: the nearest free adult (a pregnant female can't conceive).
        const first = [...adults].sort((a, b) => Number(a.pregnant) - Number(b.pregnant) || camDist(a) - camDist(b))[0];
        if (first && mine.value !== String(first.id)) {
          mineP.set(first.id);
          schedulePreview();
        }
      }
      const me = ownById(Number(mine.value));
      const opposite = (sex: string) => !me || sex !== me.sex;
      const here = me ? { x: me.x, y: me.y } : view.cam;
      const dist = (o: { x: number; y: number }) => Math.hypot(o.x - here.x, o.y - here.y);
      // Every own adult of the other sex, wherever it is (the lineage is all
      // known); a pregnant female is listed but can't be picked. Other
      // players' animals: those the server sends (in or near the view);
      // those of a lineage that can't breed with yours are marked.
      const ownPartners = adults.filter((o) => o.id !== me?.id && opposite(o.sex)).map((o): PickItem => ({
        id: o.id, label: `#${o.id} ${o.sex} (yours)`,
        detail: [o.pregnant && 'pregnant', busy.has(o.id) && 'promised', `${dist(o).toFixed(0)} tiles`].filter(Boolean).join(' · '),
        sex: o.sex, pregnant: o.pregnant, free: !busy.has(o.id) && !o.pregnant, disabled: o.pregnant, distance: dist(o),
      }));
      const others = (s.nearby ?? []).filter((o) => o.matured && o.owner !== WILD && o.owner !== model.player && opposite(o.sex)).map((o): PickItem => {
        const sterile = sterileWith(o.lineage);
        return {
          id: o.id, label: `#${o.id} ${o.sex} · ${ownerName(o.owner)}`,
          detail: [sterile && 'sterile with yours', `${dist(o).toFixed(0)} tiles`].filter(Boolean).join(' · '),
          sex: o.sex, free: !sterile, disabled: sterile, distance: dist(o),
        };
      });
      const all = [...ownPartners, ...others];
      partnerP.setItems(all);
      const open = all.filter((c) => !c.disabled).length;
      const sterile = others.filter((c) => c.disabled).length;
      setText(partnerNote, open > 0
        ? `${open} possible partner${open === 1 ? '' : 's'}${sterile ? ` · ${sterile} of other players' can't breed with yours` : ''}.`
        : ownPartners.length > 0 ? 'No partner free now: yours of the other sex are pregnant.'
          : sterile > 0 ? 'No partner: the other players\' animals in view can\'t breed with yours.'
            : 'No partner: none of yours of the other sex, none of other players\' in view.');
      walk.disabled = !me || partner.value === '' || Number(partner.value) === me.id;
      if (Date.now() - previewAt > 3000 && partner.value !== '') void updateCompat();
      slots.textContent = `${s.slots_left} directed-mating slot${s.slots_left === 1 ? '' : 's'} left this season.`;
    }
    const el = h('div', { class: 'form' },
      h('div', { class: 'field' }, h('span', { class: 'field-label' }, 'My adult'), mine),
      h('div', { class: 'field' }, h('span', { class: 'field-label' }, 'Partner'), partnerPick),
      partnerNote,
      h('div', { class: 'row' }, h('label', { class: 'partner-id' }, 'Partner id', partner), h('label', {}, 'Price', price)),
      compat,
      h('div', { class: 'row' }, submit, walk), slots,
      h('p', { class: 'muted small' }, `Pick a partner from the list, or on the map ("Request mating…"). Within your own lineage the pair mates at once when within ${MEETING_TILES} tiles; another player must agree, then the two must meet.`));
    return {
      el,
      refresh,
      setMine: (id: OrgId) => { picked = true; mineP.set(id); if (model.state) refresh(model.state); schedulePreview(); },
      setPartner: (id: OrgId) => { partner.value = String(id); partnerP.set(id); schedulePreview(); },
      /** Whether that lineage is known not to breed with yours (false while unknown). */
      sterileWith,
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
    if (!p) return model.toast('info', `Possessing #${mine}. The partner #${partner} is out of sight: ${touchInput() ? 'tap' : 'click'} the map to walk.`);
    view.setMoveTarget({ x: p.x, y: p.y });
    const r = await send({ type: 'order_move', organism: mine, x: p.x, y: p.y });
    if (!r.ok) view.rejectMove();
    if (r.ok) model.toast('info', `#${mine} is walking to #${partner}.`);
  }

  /** When an open offer lapses: its own deadline (pending: unanswered; accepted: the two never met), or on older
   * servers at the next change of season (game and real time). */
  function offerTime(o?: OfferView): string {
    const tick = model.state?.clock.tick;
    if (tick !== undefined && o?.expires_tick !== undefined) {
      const left = Math.max(0, o.expires_tick - tick);
      return o.status === 'accepted' ? `Lapses if they haven't met in ${span(left)}.` : `Expires unanswered in ${span(left)}.`;
    }
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
    // (Offers come with the first state: until then "None." would be wrong, playtest 3.)
    const none = (el: HTMLElement, text: string, empty: boolean) => {
      const say = s ? text : 'Loading…';
      const ph = el.previousElementSibling as HTMLElement | null;
      if (ph?.classList.contains('offers-none')) {
        ph.hidden = !empty;
        if (ph.textContent !== say) ph.textContent = say;
      } else el.before(h('p', { class: 'muted small offers-none', hidden: !empty }, say));
    };
    const inc = [...incoming].sort((a, b) => a.offer.id - b.offer.id);
    none(incomingEl, 'None.', inc.length === 0);
    reconcile(incomingEl, inc, (e) => String(e.offer.id), (e) => {
      const id = e.offer.id;
      // An offer to your lineage: you pick which of your animals takes it
      // (default: the server picks the nearest free one).
      const pick = h('select', { class: 'pick-own', hidden: true, title: 'Which of your animals takes this mating' }) as HTMLSelectElement;
      const accept = () => {
        const partner = Number(pick.value) || undefined;
        void send({ type: 'respond_mating', request: id, accept: true, ...(partner ? { partner } : {}) });
      };
      return h('div', { class: 'offer', 'data-offer': id }, h('span', { class: 'offer-line' }), h('small', { class: 'muted offer-time' }),
        h('div', { class: 'row' },
          pick,
          h('button', { class: 'accept', onclick: accept }, 'Accept'),
          h('button', { class: 'secondary decline', onclick: () => void send({ type: 'respond_mating', request: id, accept: false }) }, 'Decline'),
          h('button', { class: 'secondary block', title: 'Decline this and every later request from this player', onclick: () => {
            const from = model.offers.get(id)?.offer.from_player;
            if (from === undefined) return;
            void send({ type: 'respond_mating', request: id, accept: false });
            blockPlayer(from);
          } }, 'Block player')));
    }, (row, e) => {
      setText(row.querySelector('.offer-line')!, offerLine(e.offer));
      setText(row.querySelector('.offer-time')!, [offerTime(e.offer), meetText(e.offer, model.player)].filter(Boolean).join(' '));
      const pick = row.querySelector<HTMLSelectElement>('.pick-own');
      if (pick) {
        const lineageOffer = e.offer.partner === 0 && e.offer.lineage !== undefined;
        pick.hidden = !lineageOffer;
        if (lineageOffer && document.activeElement !== pick) {
          const adults = (s?.organisms ?? []).filter((o) => o.matured && !o.pregnant).slice(0, 200);
          const key = adults.map((o) => o.id).join(',');
          if (pick.dataset.key !== key) {
            const keep = pick.value;
            pick.dataset.key = key;
            pick.replaceChildren(h('option', { value: '' }, 'nearest free one'),
              ...adults.map((o) => h('option', { value: o.id }, `#${o.id} ${o.sex} · gen ${o.generation}`)));
            if ([...pick.options].some((x) => x.value === keep)) pick.value = keep;
          }
        }
      }
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
          accepted && e.offer.from_player === me && h('select', { class: 'sub-pick', hidden: true, 'aria-label': 'Send another animal' }),
          accepted && e.offer.from_player === me && h('button', { class: 'sub-send', hidden: true, onclick: async () => {
            const pick = document.querySelector<HTMLSelectElement>(`[data-offer="${e.offer.id}"] .sub-pick`);
            const mine = Number(pick?.value);
            if (!mine) return;
            const r = await send({ type: 'substitute_mating', request: e.offer.id, mine });
            if (r.error && !LOCAL_CODES.has(r.error.code)) model.toast('error', friendlyError(r.error));
          } }, 'Send this one instead'),
          h('button', { class: 'secondary', onclick: () => void send({ type: 'cancel_mating', request: e.offer.id }) }, 'Cancel')));
    }, (row, e) => {
      setText(row.querySelector('.offer-line')!, offerLine(e.offer));
      setText(row.querySelector('.offer-time')!, [offerTime(e.offer), meetText(e.offer, model.player)].filter(Boolean).join(' '));
      const note = row.querySelector<HTMLElement>('.offer-note');
      if (note) {
        const mineId = e.offer.from_player === me ? e.offer.mine : e.offer.partner;
        const lost = e.offer.lost;
        setText(note, lost && e.offer.from_player === me
          ? `Your #${e.offer.mine} ${lost}: send another adult before the deadline, or cancel.`
          : lost ? `Their #${e.offer.mine} ${lost}: they may send another before the deadline.`
            : `Agreed${e.error ? ` (${friendlyError(e.error)})` : ''}: escort #${mineId} to the partner; it completes when they meet.`);
      }
      const subPick = row.querySelector<HTMLSelectElement>('.sub-pick');
      const subSend = row.querySelector<HTMLElement>('.sub-send');
      if (subPick && subSend) {
        const show = !!e.offer.lost;
        subPick.hidden = !show;
        subSend.hidden = !show;
        if (show && document.activeElement !== subPick) {
          // Only the sex the partner needs (playtest 6 listed females to stand in for a dam's mate): the
          // server says which (`substitute_sex`); else the partner's opposite when it is in sight.
          const partnerSex = s?.nearby.find((o) => o.id === e.offer.partner)?.sex;
          const want = e.offer.substitute_sex ?? (partnerSex ? (partnerSex === 'male' ? 'female' : 'male') : undefined);
          const adults = (s?.organisms ?? []).filter((o) => o.matured && !o.pregnant && (!want || o.sex === want)).slice(0, 200);
          const key = adults.map((o) => o.id).join(',');
          if (subPick.dataset.key !== key) {
            const keep = subPick.value;
            subPick.dataset.key = key;
            subPick.replaceChildren(...adults.map((o) => h('option', { value: o.id }, `#${o.id} ${o.sex} · gen ${o.generation}`)));
            if ([...subPick.options].some((x) => x.value === keep)) subPick.value = keep;
          }
        }
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
    cam: () => view.cam,
    onNameplate: () => void refreshNameplates(),
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
    renderDashboard(dashEl, stats, () => helpTopic?.('Why are my animals dying?'));
    renderFeedPanel();
  }

  // "While you were away" (playtest 7): the alerts since the last tick this
  // device saw, once, when a returning player's feed arrives.
  const awayKey = seenKey(deps.world ?? '', model.player);
  let lastSeen = 0;
  try { lastSeen = Number(localStorage.getItem(awayKey) ?? 0) || 0; } catch { /* storage off */ }
  let awaySaid = !deps.returning || lastSeen === 0;
  function sayAway(): void {
    if (awaySaid || model.feed.length === 0) return;
    awaySaid = true;
    const text = awaySummary(model.feed, lastSeen);
    if (text) model.toast('info', text);
  }
  let seenSaved = 0;
  function rememberSeen(): void {
    const tick = model.state?.clock.tick ?? 0;
    const now = Date.now();
    if (tick <= 0 || now - seenSaved < 10_000) return;
    seenSaved = now;
    try { localStorage.setItem(awayKey, String(tick)); } catch { /* storage off */ }
  }
  /** Alerts from before the current lineage's founding show only on request. */
  let showEarlierFeed = false;
  function renderFeedPanel(): void {
    const all = model.visibleFeed();
    seenFeed = all.length;
    alertBadge.hidden = true;
    // Alerts from before this lineage was founded (a retired or extinct one's)
    // are folded away (playtest 7: the feed kept the retired lineage's attacks).
    const since = stats && stats.lineage === model.state?.lineage ? stats.ramp?.founded_day : undefined;
    const earlier = since === undefined ? 0 : all.filter((e) => e.day !== undefined && e.day < since).length;
    const feed = earlier > 0 && !showEarlierFeed ? all.filter((e) => e.day === undefined || e.day >= since!) : all;
    keyed(feedEl, `${all.length}|${all[all.length - 1]?.text ?? ''}|${model.prefs.hidden.join()}|${earlier}|${showEarlierFeed}`, () => [
      earlier > 0 ? h('p', { class: 'muted small', id: 'feed-earlier' },
        showEarlierFeed ? 'Showing alerts from before this lineage too. ' : `${earlier} alert${earlier === 1 ? '' : 's'} from before this lineage (day ${since! + 1}) hidden. `,
        h('button', { type: 'button', class: 'secondary', onclick: () => { showEarlierFeed = !showEarlierFeed; renderFeedPanel(); } }, showEarlierFeed ? 'Hide them' : 'Show them')) : null,
      renderFeed(feed, (id) => {
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
  const retireRule = h('p', { class: 'small', id: 'lineage-retire-rule' }, h('strong', {}, 'Not a free re-roll: '), 'founding again soon after retiring founds fewer founders and costs renown. Founding again within that window also continues your last lineage\'s grace (even one that has run out); it never starts a new one.');
  const retireEl = h('details', { id: 'lineage-retire', class: 'retire' },
    h('summary', {}, 'Retire this lineage'),
    h('p', { class: 'muted small' },
      'Start over: every animal and egg of your lineage leaves the world at once (they are removed, not killed and not left wild), ',
      'its orders end, and you can design and found a new lineage straight away. Your renown, records and titles stay. ',
      'A new lineage\'s legacy share counts toward the ranked season gradually over its first days.'),
    retireRule,
    retireBtn);
  retireEl.hidden = true;
  // The re-founding rule on the panel itself, not only in the confirm (playtest 7: it read as a free re-roll).
  retireEl.addEventListener('toggle', () => {
    if (!retireEl.open) return;
    void api.world().then((w) => {
      retireRule.replaceChildren(h('strong', {}, 'Not a free re-roll: '), w?.refound_rule ? `after retiring, ${w.refound_rule}.` : 'founding again soon after retiring founds fewer founders and costs renown.',
        ' Founding again within that window also continues your last lineage\'s grace (even one that has run out); it never starts a new one.');
    }).catch(() => undefined);
  });
  retireBtn.addEventListener('click', async () => {
    const lineage = model.state?.lineage;
    if (lineage === null || lineage === undefined) return;
    const alive = model.state?.population ?? 0;
    // Animals carrying a quarter or more of another player's genes join that player's lineage instead (server RETIRE_TRANSFER_SHARE).
    // The window the server enforces, fetched now (game days, real minutes).
    const w = await api.world().catch(() => null);
    // The server's own words for the re-founding rule (its pricing changes: playtest 5).
    const rule = w?.refound_rule ? `After retiring, ${w.refound_rule}.` : 'Founding again soon after retiring founds fewer founders and costs renown.';
    const yes = await confirmDialog({
      id: 'retire-confirm', title: `Retire lineage #${lineage}?`, ok: 'Retire lineage', danger: true,
      body: [
        `Its ${animals(alive)} and their eggs leave the world (any carrying at least 25% of another player's genes join that player's lineage instead). This can't be undone.`,
        rule,
      ],
    });
    if (!yes) return;
    const r = await send({ type: 'retire_lineage', lineage });
    if (r.ok) {
      const d = r.data as { removed?: number; transferred?: number } | undefined;
      const n = d?.removed ?? 0;
      const moved = d?.transferred ?? 0;
      markRetired(lineage);
      model.toast('info', `Lineage #${lineage} retired: ${animals(n)} left the world${moved > 0 ? ` and ${animals(moved)} joined other players' lineages` : ''}. You can found a new lineage.`);
      retireEl.open = false;
    }
  });
  // End the newcomer grace early (playtest 3: predators never got to hunt). One element, shown while graced.
  const endGraceBtn = h('button', { type: 'button', id: 'end-grace', class: 'danger' }, 'End my grace now');
  const endGraceEl = h('details', { id: 'lineage-end-grace', class: 'retire' },
    h('summary', {}, 'End the newcomer grace'),
    h('p', { class: 'muted small' },
      'While the grace lasts, other players\' animals can\'t attack yours, and yours can\'t attack theirs, by order or on instinct. ',
      'A hunter needs prey: end it to hunt other players\' animals now. They can hunt yours from then on, and founding again soon after won\'t bring it back.'),
    endGraceBtn);
  endGraceEl.hidden = true;
  endGraceBtn.addEventListener('click', async () => {
    if (!await confirmDialog({ id: 'end-grace-confirm', title: 'End your newcomer grace?', ok: 'End grace', danger: true,
      body: 'Other players\' animals can attack yours from now on. This can\'t be undone.' })) return;
    const r = await send({ type: 'end_grace' });
    // The server's alert says it (and toasts): no second toast here.
    if (r.ok) endGraceEl.open = false;
  });
  append(tabs.lineage, [lineageExtinct, retireEl, endGraceEl, statsEl,
    h('h3', { title: 'Split a herd or pack into groups with their own orders: migrate one, have another hold a territory' }, 'Groups'), groupsEl,
    h('h3', {}, 'Legacy leaderboard'), h('p', { class: 'muted small', id: 'legacy-explain' }, LEGACY_EXPLAIN), boardEl, h('h3', { title: 'Legacy standings per spring, summer, autumn and winter of the world\'s year (the Ranks tab has the ranked seasons, a competition of several weeks)' }, 'By time of year'), seasonsEl,
    h('h3', {}, 'My organisms'), listEl, listMore]);

  function renderSeasons(): void {
    const list = seasons;
    const clock = model.state?.clock;
    // "(now)" from the clock (the server's flag lagged a season: playtest 2).
    const isNow = (b: SeasonBoard) => (clock ? b.year === clock.year && b.season === clock.season : b.current);
    keyed(seasonsEl, JSON.stringify([list, clock?.year, clock?.season, clock?.day]), () => {
      if (!list) return [h('p', { class: 'muted small' }, 'Loading…')];
      if (list.length === 0) return [h('p', { class: 'muted small' }, 'The first season standings come with the first daily record.')];
      return list.map((b) => {
        const now = isNow(b);
        const last = now && clock ? Math.max(b.last_day, clock.day) : b.last_day;
        // Players are ranked among players: the wild row has no number.
        let rank = 0;
        return h('details', { class: 'season', open: now || undefined },
          h('summary', {}, `Year ${b.year + 1} ${b.season}${now ? ' (now)' : ''} · days ${b.first_day + 1}–${last + 1}${now ? ' so far' : ''}`),
          h('div', { class: 'table-scroll' }, h('table', { class: 'board' },
            h('thead', {}, h('tr', {}, h('th', { title: 'Rank among players' }, '#'), h('th', {}, 'Player'), h('th', { title: 'Mean legacy share over the recorded days' }, 'Mean'),
              h('th', {}, 'Final'), h('th', {}, 'Peak'), h('th', { title: 'Most organisms alive on a recorded day' }, 'Peak alive'))),
            h('tbody', {}, ...b.rows.slice(0, 8).map((r) => h('tr', { class: r.player === model.player ? 'me' : '' },
              h('td', {}, r.player === WILD ? '—' : String(++rank)),
              h('td', {}, h('span', { class: 'swatch', style: `background:${cssColor(r.player === model.player ? OWN_COLOR : ownerColor(r.player))}` }), h('span', { class: 'who', title: r.name }, r.name)),
              h('td', {}, pct(r.mean_share)), h('td', {}, pct(r.final_share)), h('td', {}, pct(r.peak_share)), h('td', {}, String(r.peak_alive))))))));
      });
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
    const mine = stats && stats.lineage === s.lineage ? stats : null;
    const crowding = mine?.crowding;
    // The state's grace (session B: days, and real time at the server's clock), else the statistics'.
    const grace = s.grace
      ? graceText(s.grace.days_left, ticksPerDay, span, s.grace.real_seconds_left)
      : graceText(mine?.grace_days_left, ticksPerDay, span);
    const ramp = rampText(mine?.ramp);
    const cap = capInfo(s.population, worldCap, crowding);
    // With level of detail on, the dashboard's counts include the cohorts far from players.
    const fresh = stats && stats.lineage === s.lineage && stats.alive === s.population;
    const adultsAll = fresh ? stats!.adults : adults;
    const youngAll = fresh ? stats!.juveniles : orgs.length - adults;
    const maxGen = orgs.reduce((m, o) => Math.max(m, o.generation), 0);
    const gone = s.lineage !== null && s.population === 0;
    if (gone) {
      // Nobody left (died out or retired): none of the living lineage's rows,
      // crowding or group orders apply (they lingered from the last statistics).
      statsEl.replaceChildren(dl([
        ['Lineage', `#${s.lineage}`],
        ['Alive', h('strong', { id: 'lineage-size' }, '0')],
      ]));
      keyed(groupsEl, 'no-lineage', () => [h('p', { class: 'muted small' }, 'No groups: your lineage has no living animals.')]);
    }
    if (!gone) statsEl.replaceChildren(dl([
      ['Lineage', s.lineage === null ? 'not founded' : `#${s.lineage}`],
      ['Alive', h('span', {}, crowding ? h('span', { id: 'alive-count' }, `${grouped(s.population)} · crowding `) : null, h('strong', { id: 'lineage-size', title: crowding ? 'Crowding count (adults, young weighted) / soft cap' : 'Living organisms / cap' }, cap.count),
        cap.status ? h('span', { class: `cap-status cap-${cap.level}`, id: 'cap-status' }, ` — ${cap.status}`) : null,
        s.population > orgs.length
          ? h('span', { class: 'muted small', title: 'Level of detail: members far from any player run as statistical cohorts and are not drawn' }, ` · ${orgs.length} on the map`)
          : null)],
      ['Adults / young', h('span', { id: 'adults-young', title: 'Young count toward the population cap too' }, `${grouped(adultsAll)} adults / ${grouped(youngAll)} young`)],
      eggStatus(orgs),
      ['Newest generation', String(maxGen)],
      ['Mating slots left', h('span', { id: 'slots-left' }, String(s.slots_left))],
      ['Migration', migrationText(s.migration, s.groups)],
      ...(grace ? [['Newcomer grace', h('span', { id: 'grace-left' }, grace)] as [string, string | Node]] : []),
      ...(ramp ? [['Season weight', h('span', { id: 'ramp-weight' }, ramp)] as [string, string | Node]] : []),
    ]));
    if (cap.explain && !gone) statsEl.append(h('p', { class: `cap-note small${cap.level === 'ok' ? ' muted' : ''}`, id: 'cap-note' }, cap.explain));
    // Lineage-wide warnings (hunger crash, near the hard cap, starving territories), as the alerts say them.
    const warnings = gone ? [] : mine?.warnings ?? [];
    if (warnings.length) {
      statsEl.append(h('ul', { class: 'lineage-warnings', id: 'lineage-warnings', role: 'status' },
        ...warnings.map((w) => h('li', { class: `lineage-warning warn-${w.kind}`, 'data-kind': w.kind }, '⚠ ', w.text))));
    }
    // A founded lineage with nobody left: say so plainly, and where to see why.
    // One element, shown or hidden (rebuilding it each state would eat clicks on its button).
    lineageExtinct.hidden = !(s.lineage !== null && s.population === 0);
    if (!lineageExtinct.hidden && wasRetired(s.lineage) && !lineageExtinct.dataset.retired) {
      // Retired, not dead: say so (and no "what killed them").
      lineageExtinct.dataset.retired = '1';
      lineageExtinct.querySelector('.extinct-text')?.replaceChildren(h('strong', {}, `You retired lineage #${s.lineage}.`), ' Its animals have left the world.');
    }
    retireEl.hidden = !(s.lineage !== null && s.population > 0);
    // Nothing to end without a living lineage (playtest 7: it stayed after a retire).
    endGraceEl.hidden = !s.grace || gone;
    if (!gone) renderGroups(s);
    const lb = s.leaderboard;
    keyed(boardEl, JSON.stringify(lb) + nameplatesKey(), () => lb.rows.length === 0
      ? [h('p', { class: 'muted small' }, lb.day === null ? 'The first legacy snapshot comes at the end of day 1.' : 'No breeding adults.')]
      : [h('p', { class: 'muted small' }, `Day ${(lb.day ?? 0) + 1} snapshot · ${lb.breeding_adults} breeding adults`),
        h('table', { class: 'board' },
          h('thead', {}, h('tr', {}, h('th', {}, 'Player'), h('th', {}, 'Share'), h('th', { title: 'Weighted by offspring raised to adulthood' }, 'Weighted'), h('th', {}, 'Alive'))),
          h('tbody', {}, ...lb.rows.map((r) => h('tr', { class: r.player === model.player ? 'me' : '' },
            h('td', {}, h('span', { class: 'swatch', style: `background:${cssColor(r.player === model.player ? OWN_COLOR : ownerColor(r.player))}` }), h('span', { class: 'who', title: r.name }, r.name), plateBadge(r.player)),
            h('td', {}, h('span', { class: 'share-bar', style: `--w:${(r.share * 100).toFixed(1)}%` }, pct(r.share))),
            h('td', {}, pct(r.weighted_share)), h('td', {}, String(r.alive))))))]);
    // Possessed first, then pregnant or gravid ones (so the Eggs line's "about to lay" are all listed), then adults.
    const shown = [...orgs].sort((a, b) => Number(b.possessed) - Number(a.possessed) || Number(b.pregnant) - Number(a.pregnant)
      || Number(b.matured) - Number(a.matured) || a.id - b.id).slice(0, 60);
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
      setText(btn, `#${o.id} ${o.sex === 'female' ? 'F' : 'M'}${o.matured ? '' : ' juv'}${o.pregnant ? (laysEggs() ? ' gravid' : ' preg') : ''} · ${actionText(o.action, o.needs)}`);
    });
    setText(listMore, orgs.length > shown.length ? `…and ${orgs.length - shown.length} more` : '');
  }

  /** "12 lost since founding (12 chilled)" for the eggs a lineage lost, or null for none. */
  function eggsLostText(lost: Record<string, number> | undefined): string | null {
    const entries = Object.entries(lost ?? {}).filter(([, n]) => n > 0);
    const total = entries.reduce((sum, [, n]) => sum + n, 0);
    // A lifetime count, beside the clutches now: said so (it read as more lost than there are eggs).
    return total > 0 ? `${total} lost since founding (${entries.map(([c, n]) => `${n} ${c}`).join(', ')})` : null;
  }

  /** The lineage lays eggs (its stats count clutches): a carrying female is "gravid", not "pregnant" (playtest 3). */
  function laysEggs(): boolean {
    if (!stats || stats.lineage !== model.state?.lineage) return false;
    // By the lineage's birth mode (the mean of its members, as the family
    // tree reads it: live birth from 2/3), not by whether any egg was laid:
    // one hybrid clutch made a live-bearing lineage "gravid" (playtest 5).
    const mode = stats.genes.find((g) => g.gene === 'birth_mode')?.mean;
    return mode !== undefined ? mode < 2 / 3 : !!stats.eggs;
  }

  /** "Pregnant N" for live bearers; clutches and brooding for egg layers (a gravid female is about to lay). */
  function eggStatus(orgs: StateView['organisms']): [string, string | Node] {
    const pregnant = orgs.filter((o) => o.pregnant).length;
    const e = laysEggs() ? stats?.eggs : undefined;
    if (!e) return ['Pregnant', String(pregnant)];
    const brooding = orgs.filter((o) => o.action === 'guard').length;
    const parts = [
      // The same counts as the Genes tiles (playtest 7: "Clutches 0 / Eggs 1" there, "no clutch now" here).
      e.clutches === 0 && e.eggs === 0 ? 'no clutch now'
        : `now ${e.clutches} clutch${e.clutches === 1 ? '' : 'es'} (${e.eggs} egg${e.eggs === 1 ? '' : 's'})`,
      e.clutches > 0 ? `${e.brooded} brooded` : null,
      brooding > 0 ? `${brooding} brooding now` : null,
      pregnant > 0 ? `${pregnant} about to lay` : null,
      eggsLostText(e.lost),
    ].filter(Boolean);
    return ['Eggs', h('span', { id: 'egg-status', title: 'Now: the clutches being incubated. Lost: every egg since founding. Eggs stay warm while a parent broods them or stays near; a clutch whose parent wanders far or dies chills, and an unguarded one can be raided' }, parts.join(' · '))];
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
          h('button', { class: 'secondary small', 'data-action': 'territory', title: wait > 0 ? `Claimed recently: it can claim again in ${span(wait)}` : `Radius up to ${g.max_radius.toFixed(1)} tiles`,
            disabled: wait > 0 || g.members === 0, onclick: () => { setGroupChoice(g.id); setMode('territory'); if (narrow()) setSheet(false); } }, 'Claim territory…'),
          g.territory && h('button', { class: 'secondary small', 'data-action': 'abandon', onclick: async () => {
            const r = await send({ type: 'abandon_territory', group: g.id });
            if (r.ok) model.toast('info', `${groupName(g.id)} gave up its territory (it can claim again once the claim wait is over)`);
            else if (r.error) model.toast('error', `Abandon: ${r.error.message}`);
          } }, 'Abandon territory'),
          h('button', { class: 'secondary small', 'data-action': 'rally', title: 'Gather the group at a point you tap; it holds there until the rally lapses (at least 5 minutes; the group card shows when)',
            disabled: g.members === 0, onclick: () => { setGroupChoice(g.id); setMode('rally'); if (narrow()) setSheet(false); } }, 'Rally…'),
          g.rally && h('button', { class: 'secondary small', 'data-action': 'cancel-rally', title: `Lapses on its own in ${span(rallyLeft(g, tick))}`,
            onclick: async () => {
              const r = await send({ type: 'cancel_rally', group: g.id });
              const note = (r.data as { note?: string } | undefined)?.note;
              if (r.ok) model.toast('info', `${groupName(g.id)}'s rally cancelled${note ? `: ${note}` : ''}`);
            } }, 'Cancel rally'),
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
      const outcome = g.last_outcome ? ` · last order: ${g.last_outcome}` : '';
      const text = `${g.members} (${g.adults} adults) · ${groupOrders(g)}${lapse}${heed ? ` · ${heed}` : ''}${outcome}`;
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
      setPinned(!!model.state?.view.pinned);
      trackViewGroup();
      renderViewGroup();
      // A new lineage (after a retire): nothing of the old one stays on
      // screen, neither its animal in the inspector nor its group cards'
      // last orders (playtest 6).
      const lin = model.state?.lineage ?? null;
      if (lin !== shownLineage) {
        if (shownLineage !== undefined) {
          if (model.selected !== null && !model.state?.organisms.some((o) => o.id === model.selected) && !model.state?.nearby.some((o) => o.id === model.selected)) model.select(null);
          keys.delete(groupsEl);
          groupMenuKey = '';
        }
        shownLineage = lin;
      }
      showState();
      renderGroupMenu();
      updateHint();
      renderPanels();
    }),
    model.on('select', () => { view.selected = model.selected; renderFab(); renderPeek(); renderPanels(); }),
    model.on('offers', () => renderPanels()),
    model.on('conn', renderConn),
    model.on('alerts', onAlerts),
    model.on('alerts', sayAway),
    model.on('state', rememberSeen),
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
  sayAway();
  renderConn();
  updateHint();
  ready = true;
  void refreshNameplates();
  el.classList.remove('starting');
  // The tab the player picked while the screen was loading, else Lineage.
  showTab(tab);

  // First-session onboarding: the guided tour (once per account on this
  // device), one-time hints and the Help dialog. `?tour=off` turns it off.
  const touchInput = () => narrow() || window.matchMedia('(pointer: coarse)').matches;
  // (Declared above its first use: the dashboard links into Help.)
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
    returning: deps.returning,
  });
  helpTopic = (t) => onboarding.openHelp(t);
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

/** "1 animal", "2,100 animals". */
function animals(n: number): string {
  return `${n.toLocaleString('en-US')} animal${n === 1 ? '' : 's'}`;
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

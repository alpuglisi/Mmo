// Wire types of the evo-server HTTP API and WebSocket protocol.
//
// Written by hand from crates/server/src/protocol.rs (WebSocket, state) and
// crates/server/src/design.rs + http.rs (HTTP bodies). The Rust types are the
// source of truth: KEEP THIS FILE IN SYNC when they change.

export type PlayerId = number;
export type OrgId = number;

/** Player 0 owns the wild ecosystem. */
export const WILD: PlayerId = 0;

// ---------------------------------------------------------------- client → server

export type ClientMsg =
  | { type: 'possess'; organism: OrgId }
  | { type: 'order_move'; organism: OrgId; x: number; y: number }
  | { type: 'release'; organism: OrgId }
  | { type: 'request_mating'; mine: OrgId; partner: OrgId; price?: number }
  /** What a mating of `mine` with `partner` would give; data: `MatingPreviewView`. */
  | { type: 'preview_mating'; mine: OrgId; partner: OrgId }
  | { type: 'respond_mating'; request: number; accept: boolean }
  | { type: 'complete_mating'; request: number }
  | { type: 'cancel_mating'; request: number }
  | { type: 'migrate'; x: number; y: number }
  | { type: 'clear_migration' }
  | { type: 'set_view'; x: number; y: number; radius: number }
  | { type: 'clear_view' }
  /** Put one of your adults on the stud marketplace; data: `{ listing: ListingView }`. */
  | { type: 'create_listing'; organism: OrgId; fee: number; expires_days?: number }
  | { type: 'withdraw_listing'; listing: number }
  /** Buy one mating with a listed organism for your adult `mine`; data: `{ offer: OfferView }`. */
  | { type: 'buy_listing'; listing: number; mine: OrgId }
  /** Open listings (with compatibility against `mine`); data: `MarketView`. */
  | { type: 'browse_market'; mine?: OrgId }
  /** Balance and recent ledger entries; data: `LedgerView`. */
  | { type: 'ledger'; limit?: number };

/** `ClientEnvelope`: a command plus an optional id echoed in its `result`. */
export type ClientEnvelope = ClientMsg & { id?: number };

// ---------------------------------------------------------------- server → client

export interface ErrorInfo {
  code: string;
  message: string;
  /** Specifics for some codes: `incompatible_body_plans` has `{ genes: BodyPlanGap[] }`. */
  detail?: unknown;
}

/** A body-plan gene in which two organisms differ enough to lower fertility. */
export interface BodyPlanGap {
  /** Gene key as in /api/genes. */
  gene: string;
  /** Difference in expressed value. */
  gap: number;
}

/** Reply to `preview_mating`. */
export interface MatingPreviewView {
  mine: OrgId;
  partner: OrgId;
  /** Whether the pair can conceive at all (fertility > 0). */
  compatible: boolean;
  fertility: number;
  /** 0 for different body plans (thermoregulation, birth mode, body size, diet). */
  body_plan_fertility: number;
  /** Outbreeding depression over the whole genome. */
  distance_fertility: number;
  body_plan_gaps: BodyPlanGap[];
  mismatch: number;
  cost: number;
  failure_chance: number;
  /** Tiles apart; they must meet within `meeting_distance`. */
  distance: number;
  meeting_distance: number;
  /** Why a request would be refused: `incompatible_body_plans` or `genetically_incompatible`. */
  problem?: ErrorInfo;
}

export interface MatingOutcome {
  mismatch: number;
  cost: number;
  failure_chance: number;
  fertility: number;
  conceived: boolean;
}

export type OfferStatus = 'pending' | 'accepted' | 'completed' | 'declined' | 'cancelled' | 'failed' | 'expired';

export const OPEN_STATUSES: readonly OfferStatus[] = ['pending', 'accepted'];

export function isOpen(s: OfferStatus): boolean {
  return OPEN_STATUSES.includes(s);
}

export interface OfferView {
  id: number;
  from_player: PlayerId;
  from_name: string;
  to_player: PlayerId;
  to_name: string;
  /** The requester's organism. */
  mine: OrgId;
  /** The partner owner's organism. */
  partner: OrgId;
  price?: number;
  status: OfferStatus;
  created_tick: number;
  /** The marketplace listing it was bought from. */
  listing?: number;
  /** Renown held in escrow (the requester's price) while open. */
  escrow?: number;
}

export interface WeatherView {
  /** Map-wide mean wind, 0 calm to 1 gale. */
  mean_wind: number;
  storm: boolean;
  /** Share of land in drought, 0..1. */
  drought: number;
  /** Share of river tiles in flood, 0..1. */
  flood: number;
  /** The catastrophe under way, if any. */
  catastrophe?: CatastropheView;
}

export interface ClockView {
  tick: number;
  day: number;
  year: number;
  hour: number;
  season: string;
  daytime: boolean;
  /** Current era (0-based); eras last `era_days` days. */
  era: number;
  era_days: number;
}

export interface ViewWindow {
  x: number;
  y: number;
  /** Half the side of the square window, in tiles. */
  radius: number;
}

export interface NeedsView {
  hunger: number;
  thirst: number;
  energy: number;
  cold: number;
  heat: number;
  safety: number;
  breeding: number;
  social: number;
}

export type Sex = 'female' | 'male';

export type ActionName =
  | 'idle' | 'graze' | 'hunt' | 'scavenge' | 'drink' | 'rest' | 'bask' | 'flee' | 'seek_mate'
  | 'group' | 'wander' | 'migrate' | 'follow_parent' | 'guard' | 'ordered' | 'dormant' | 'cache' | 'sentry';

/** One of the player's own organisms: full detail. */
export interface OwnOrganism {
  id: OrgId;
  lineage: number;
  x: number;
  y: number;
  action: ActionName | string;
  sex: Sex;
  age_days: number;
  growth: number;
  matured: boolean;
  health: number;
  needs: NeedsView;
  possessed: boolean;
  pregnant: boolean;
  generation: number;
  /** Visible body plan, for the sprite (absent from older servers). */
  look?: Appearance;
  /** Possessed only: the move order's target `[x, y]` (tiles) while it has one. */
  order?: [number, number];
  /** Possessed only: tiles per tick it walks toward an order on open ground (before the terrain's factor). */
  speed?: number;
}

/** Someone else's organism inside the view window: only what can be seen. */
export interface NearbyOrganism {
  id: OrgId;
  owner: PlayerId;
  lineage: number;
  x: number;
  y: number;
  action: ActionName | string;
  sex: Sex;
  matured: boolean;
  /** Visible body plan, for the sprite (absent from older servers). */
  look?: Appearance;
}

/** `appearance::Appearance`: coarse visible body-plan classes of an adult. */
export interface Appearance {
  /** Adult size class from mass: 0 tiny (< 1 kg), 1 small (< 8 kg), 2 medium (< 40 kg), 3 large. */
  size: 0 | 1 | 2 | 3;
  diet: 'herbivore' | 'omnivore' | 'carnivore';
  /** Warm-blooded. */
  warm: boolean;
  locomotion: 'walk' | 'swim' | 'glide' | 'fly';
  cover: 'skin' | 'fur' | 'scales' | 'armor';
  /** Drab browns, camouflage mottling, bright display colors, or warning colors (bright and toxic). */
  coloring: 'drab' | 'camo' | 'bright' | 'warning';
}

export interface LeaderRow {
  player: PlayerId;
  name: string;
  /** Share of alleles among living breeding adults. */
  share: number;
  /** Share weighted by offspring raised to adulthood. */
  weighted_share: number;
  /** Living organisms the player owns. */
  alive: number;
}

export interface Leaderboard {
  /** Day of the legacy snapshot (null before the first simulated day ends). */
  day: number | null;
  breeding_adults: number;
  rows: LeaderRow[];
}

export interface StateView {
  clock: ClockView;
  weather: WeatherView;
  player: PlayerId;
  lineage: number | null;
  slots_left: number;
  /** Living members of the lineage, cohort members of abstract (level of detail) regions included; `organisms` lists only agents. */
  population: number;
  migration: [number, number] | null;
  view: ViewWindow;
  organisms: OwnOrganism[];
  nearby: NearbyOrganism[];
  leaderboard: Leaderboard;
  offers: OfferView[];
  /** Renown available to spend (absent from servers before the economy). */
  balance?: number;
  /** Renown of yours held in escrow on open agreements. */
  escrow?: number;
}

export interface WelcomeMsg {
  type: 'welcome';
  player: PlayerId;
  name: string;
  lineage: number | null;
}

/** `ServerMsg::State` is a newtype variant: the state's fields sit beside `type`. */
export type StateMsg = { type: 'state' } & StateView;

export interface ResultMsg {
  type: 'result';
  id?: number;
  command: string;
  ok: boolean;
  error?: ErrorInfo;
  data?: unknown;
}

export interface MatingRequestMsg {
  type: 'mating_request';
  offer: OfferView;
}

export interface MatingUpdateMsg {
  type: 'mating_update';
  offer: OfferView;
  outcome?: MatingOutcome;
  error?: ErrorInfo;
}

export type AlertKind = 'daily_summary' | 'birth' | 'death' | 'milestone' | 'need' | 'world' | 'achievement' | 'market';

/** Every alert kind, in the server's order. */
export const ALERT_KINDS: readonly AlertKind[] = ['daily_summary', 'birth', 'death', 'milestone', 'need', 'world', 'achievement', 'market'];

/**
 * `GET` / `POST /api/alerts/prefs`: which alert kinds show in the feed and
 * which also pop up as a toast. Stored per player on the server; applied
 * by the client.
 */
export interface AlertPrefs {
  /** Left out of the feed and the unread badge (never toasted). */
  hidden: AlertKind[];
  /** Also toasted when they arrive. */
  toast: AlertKind[];
}

/** What the server answers before a player saves any preferences. */
export const DEFAULT_ALERT_PREFS: AlertPrefs = { hidden: [], toast: ['death', 'need', 'market'] };

export interface AlertView {
  tick: number;
  day: number;
  kind: AlertKind;
  text: string;
  organism?: OrgId;
}

/** Something the player should notice (deaths, milestones, needs, a day's vitals). */
export interface AlertMsg {
  type: 'alert';
  alert: AlertView;
}

/** The session expired or was logged out; the socket closes next. */
export interface SessionEndedMsg {
  type: 'session_ended';
  reason: string;
}

export type ServerMsg = WelcomeMsg | StateMsg | ResultMsg | MatingRequestMsg | MatingUpdateMsg | AlertMsg | SessionEndedMsg;

// ---------------------------------------------------------------- economy and marketplace
// Data of the `ledger`, `browse_market` and `create_listing` commands.

export type LedgerKind = 'grant' | 'income' | 'achievement' | 'escrow' | 'sale' | 'refund';

/** One change of a player's renown balance. */
export interface LedgerEntry {
  seq: number;
  player: PlayerId;
  tick: number;
  day: number;
  delta: number;
  /** Balance after the change. */
  balance: number;
  kind: LedgerKind | string;
  text: string;
  offer?: number;
  listing?: number;
}

export interface LedgerView {
  balance: number;
  escrow: number;
  /** Newest first. */
  entries: LedgerEntry[];
}

export interface TraitView {
  /** Gene key as in /api/genes. */
  gene: string;
  value: number;
}

/** A stud (or dam) on the marketplace. */
export interface ListingView {
  id: number;
  owner: PlayerId;
  owner_name: string;
  organism: OrgId;
  role: 'stud' | 'dam' | string;
  sex: Sex;
  fee: number;
  created_tick: number;
  expires_tick?: number;
  sales: number;
  /** A dam already promised in an open agreement. */
  busy: boolean;
  pregnant: boolean;
  lineage: number;
  lineage_name: string;
  generation: number;
  mother?: OrgId;
  father?: OrgId;
  offspring_raised: number;
  age_days: number;
  health: number;
  x: number;
  y: number;
  traits: TraitView[];
  /** Compatibility with the organism passed to `browse_market`. */
  preview?: MatingPreviewView;
}

export interface MarketView {
  balance: number;
  escrow: number;
  slots_left: number;
  /** Newest first. */
  listings: ListingView[];
}

// ---------------------------------------------------------------- HTTP bodies

export interface AuthResponse {
  token: string;
  player: PlayerId;
  name: string;
}

export type ThermoSpec = 'warm' | 'cold';
export type BirthSpec = 'live' | 'incubated_eggs' | 'abandoned_eggs';

/** `DesignSpec`: the body of `POST /api/lineage`. */
export interface DesignSpec {
  name: string;
  thermo: ThermoSpec;
  birth: BirthSpec;
  litter_size: number;
  /** Trait values in [0, 1] that differ from the defaults. */
  traits: Record<string, number>;
  founders: number;
}

export interface FoundResponse {
  lineage: number;
  founders: OrgId[];
  cost: number;
}

export interface GeneInfo {
  name: string;
  default: number;
  /** Points per unit above the default. */
  cost: number;
}

/** `GET /api/genes`. */
export interface GeneCatalog {
  budget: number;
  base_founders: number;
  cost_per_extra_founder: number;
  cost_per_extra_offspring: number;
  max_litter: number;
  genes: GeneInfo[];
}

export interface WorldSummary {
  clock: ClockView;
  weather: WeatherView;
  width: number;
  height: number;
  /** Every living organism, cohort members of abstract regions included. */
  organisms: number;
  /** Organisms simulated as agents. */
  agents: number;
  lineages: number;
}

/**
 * `GET /api/terrain`: the tile grid. Every layer is one byte per tile,
 * row-major (`y * width + x`), base64-encoded.
 */
export interface TerrainView {
  width: number;
  height: number;
  terrain_version: number;
  /** 0 deep water, 1 shallow water, 2 marsh, 3 grassland, 4 forest, 5 desert, 6 tundra, 7 mountain. */
  biome: string;
  /** 0..255 for the map's lowest..highest. */
  elevation: string;
  /** River channel strength, 0 none .. 255 main stream. */
  river: string;
  /** Water depth at normal wetness, centimeters (capped at 255). */
  base_water: string;
  dynamic: TerrainDynamic;
}

/** `GET /api/terrain/dynamic`: the layers that change with weather and seasons. */
export interface TerrainDynamic {
  tick: number;
  day: number;
  hour: number;
  season: string;
  /** Current water depth, centimeters (capped at 255). */
  water: string;
  /** Ice cover, 0..255 for 0..1. */
  ice: string;
  /** Snow, 0..255 for 0..0.5 m. */
  snow: string;
  /** Plant food as a share of the tile's maximum, 0..255. */
  veg: string;
  /** Regional soil wetness, 0..255 for 0..1 (0.5 normal, below 0.3 drought). */
  wetness: string;
}

export interface AgeBand {
  from_days: number;
  to_days: number;
  female: number;
  male: number;
}

export interface DayVitals {
  day: number;
  births: number;
  /** Deaths by cause: starved, thirst, exposure, eaten, toxin, drowned, old_age, attrition, recessive, disease, journey. */
  deaths: Record<string, number>;
}

export interface GeneStat {
  gene: string;
  mean: number;
  sd: number;
  founding_mean: number;
  /** Daily means aligned with `LineageStats.history_days` (null: not recorded). */
  history: (number | null)[];
  /** Living members per equal-width bin of the expressed value over 0..1 (`GENE_BINS` bins). */
  bins?: number[];
}

/** Bins of `GeneStat.bins`. */
export const GENE_BINS = 10;

/** Whose alleles a lineage's living members carry (`LineageStats.origins`). */
export interface AlleleOrigin {
  /** The originating player (0: the wild ecosystem). */
  player: PlayerId;
  name: string;
  /** That player's lineage, if any. */
  lineage?: string;
  alleles: number;
  /** 0..1 of all alleles the members carry. */
  share: number;
}

export interface LegacyPoint {
  day: number;
  share: number;
  weighted_share: number;
  alive: number;
}

/** `GET /api/lineage/stats`: the genetics dashboard of the caller's lineage. */
export interface LineageStats {
  player: PlayerId;
  lineage: number | null;
  name: string | null;
  day: number;
  /** Cohort members of abstract (level of detail) regions included. */
  alive: number;
  /** Members simulated as agents (visible on the map). */
  agents: number;
  adults: number;
  juveniles: number;
  pregnant: number;
  max_generation: number;
  age_bands: AgeBand[];
  /** Oldest first; the last entry is the current day so far. */
  vitals: DayVitals[];
  history_days: number[];
  /** Largest drift from the founding mean first. */
  genes: GeneStat[];
  /** Whose alleles the members carry, largest share first. */
  origins?: AlleleOrigin[];
  legacy: LegacyPoint[];
  alerts: AlertView[];
}

export interface SeasonRow {
  player: PlayerId;
  name: string;
  mean_share: number;
  final_share: number;
  peak_share: number;
  mean_weighted_share: number;
  peak_alive: number;
  days: number;
}

/** `GET /api/leaderboard/seasons`: newest first. */
export interface SeasonBoard {
  year: number;
  season: string;
  first_day: number;
  last_day: number;
  days: number;
  current: boolean;
  rows: SeasonRow[];
}

// ---------------------------------------------------------------- chronicle
// Species, eras, catastrophes and achievements (`GET /api/species`,
// `/api/eras`, `/api/catastrophes`, `/api/achievements`).

export type CatastropheKind = 'severe_winter' | 'megadrought' | 'epidemic' | 'flood_year';

export interface CatastropheView {
  id: number;
  kind: CatastropheKind | string;
  severity: number;
  start_day: number;
  /** Planned first day after it while active; the day it ended once over. */
  end_day: number;
  active: boolean;
  population_before: number;
  population_after?: number;
  players_before: PlayerId[];
  survivors: PlayerId[];
}

export interface CatastropheList {
  enabled: boolean;
  active?: CatastropheView;
  /** Newest first. */
  history: CatastropheView[];
}

export interface SpeciesLineage {
  lineage: number;
  name: string;
  owner: PlayerId;
  adults: number;
}

export interface SpeciesView {
  id: number;
  name: string;
  origin: 'initial' | 'founding' | 'split' | string;
  parent?: number;
  founded_day: number;
  extinct_day?: number;
  merged_into?: number;
  /** Player credited with the speciation (splits only). */
  founder?: PlayerId;
  founder_name?: string;
  dominant: PlayerId;
  dominant_name: string;
  dominant_share: number;
  adults: number;
  peak_adults: number;
  lineages: SpeciesLineage[];
  carnivory: number;
  body_size: number;
}

export interface FoundedCount {
  player: PlayerId;
  name: string;
  count: number;
}

export interface SpeciesList {
  day: number;
  detected_day?: number;
  samples: number;
  alive: number;
  species: SpeciesView[];
  founded: FoundedCount[];
}

export interface EraRow {
  player: PlayerId;
  name: string;
  share: number;
  weighted_share: number;
}

export interface NicheView {
  niche: string;
  player: PlayerId;
  name: string;
  share: number;
  adults: number;
}

export interface EraBoard {
  era: number;
  first_day: number;
  last_day: number;
  breeding_adults: number;
  species_alive: number;
  catastrophes: number[];
  rows: EraRow[];
  niches: NicheView[];
}

export interface EraList {
  era: number;
  era_days: number;
  first_day: number;
  last_day: number;
  days_left: number;
  /** Newest first. */
  eras: EraBoard[];
}

export interface AchievementView {
  day: number;
  player: PlayerId;
  name: string;
  kind: 'species_founded' | 'lineage_longevity' | 'niche_dominance' | 'survived_catastrophe' | string;
  text: string;
}

export interface AchievementTally {
  player: PlayerId;
  name: string;
  species_founded: number;
  longevity_days: number;
  niche_dominance: number;
  catastrophes_survived: number;
}

export interface AchievementList {
  players: AchievementTally[];
  /** Newest first. */
  recent: AchievementView[];
}

/** HTTP error body: `{code, message, ...detail}`. */
export interface ApiErrorBody {
  code: string;
  message: string;
  [detail: string]: unknown;
}

// ---------------------------------------------------------------- parsing

function isObj(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

const isNum = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v);
const isStr = (v: unknown): v is string => typeof v === 'string';

function isOffer(v: unknown): v is OfferView {
  return isObj(v) && isNum(v.id) && isNum(v.mine) && isNum(v.partner) && isStr(v.status)
    && isNum(v.from_player) && isNum(v.to_player);
}

function isState(v: Record<string, unknown>): boolean {
  return isObj(v.clock) && isObj(v.weather) && isObj(v.view) && Array.isArray(v.organisms)
    && Array.isArray(v.nearby) && Array.isArray(v.offers) && isObj(v.leaderboard) && isNum(v.slots_left);
}

function isListing(v: unknown): v is ListingView {
  return isObj(v) && isNum(v.id) && isNum(v.organism) && isNum(v.owner) && isNum(v.fee) && isStr(v.role)
    && isNum(v.x) && isNum(v.y) && Array.isArray(v.traits);
}

function isLedgerEntry(v: unknown): v is LedgerEntry {
  return isObj(v) && isNum(v.seq) && isNum(v.delta) && isNum(v.balance) && isStr(v.kind) && isStr(v.text);
}

/** The data of a `browse_market` result; malformed listings are dropped, a malformed reply is null. */
export function parseMarketView(data: unknown): MarketView | null {
  if (!isObj(data) || !isNum(data.balance) || !Array.isArray(data.listings)) return null;
  return {
    balance: data.balance,
    escrow: isNum(data.escrow) ? data.escrow : 0,
    slots_left: isNum(data.slots_left) ? data.slots_left : 0,
    listings: data.listings.filter(isListing),
  };
}

/** The data of a `ledger` result. */
export function parseLedgerView(data: unknown): LedgerView | null {
  if (!isObj(data) || !isNum(data.balance) || !Array.isArray(data.entries)) return null;
  return { balance: data.balance, escrow: isNum(data.escrow) ? data.escrow : 0, entries: data.entries.filter(isLedgerEntry) };
}

/**
 * Parse one WebSocket text frame. Returns null for anything that isn't a
 * well-formed server message (bad JSON, unknown type, missing fields), so a
 * newer server can add message types without breaking an older client.
 * Checks the shape only as deep as the client relies on it.
 */
export function parseServerMsg(text: string): ServerMsg | null {
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch {
    return null;
  }
  if (!isObj(v) || !isStr(v.type)) return null;
  switch (v.type) {
    case 'welcome':
      return isNum(v.player) && isStr(v.name) && (v.lineage === null || v.lineage === undefined || isNum(v.lineage))
        ? ({ type: 'welcome', player: v.player, name: v.name, lineage: (v.lineage as number | undefined) ?? null })
        : null;
    case 'state':
      return isState(v) ? (v as unknown as StateMsg) : null;
    case 'result':
      if (!isStr(v.command) || typeof v.ok !== 'boolean') return null;
      if (v.error !== undefined && !(isObj(v.error) && isStr(v.error.code))) return null;
      return v as unknown as ResultMsg;
    case 'mating_request':
      return isOffer(v.offer) ? (v as unknown as MatingRequestMsg) : null;
    case 'mating_update':
      return isOffer(v.offer) ? (v as unknown as MatingUpdateMsg) : null;
    case 'alert':
      return isObj(v.alert) && isStr(v.alert.text) && isStr(v.alert.kind) && isNum(v.alert.day)
        ? (v as unknown as AlertMsg) : null;
    case 'session_ended':
      return isStr(v.reason) ? { type: 'session_ended', reason: v.reason } : null;
    default:
      return null;
  }
}

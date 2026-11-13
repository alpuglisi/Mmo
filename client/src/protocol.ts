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
  | { type: 'respond_mating'; request: number; accept: boolean }
  | { type: 'complete_mating'; request: number }
  | { type: 'cancel_mating'; request: number }
  | { type: 'migrate'; x: number; y: number }
  | { type: 'clear_migration' }
  | { type: 'set_view'; x: number; y: number; radius: number }
  | { type: 'clear_view' };

/** `ClientEnvelope`: a command plus an optional id echoed in its `result`. */
export type ClientEnvelope = ClientMsg & { id?: number };

// ---------------------------------------------------------------- server → client

export interface ErrorInfo {
  code: string;
  message: string;
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
}

export interface WeatherView {
  /** Map-wide mean wind, 0 calm to 1 gale. */
  mean_wind: number;
  storm: boolean;
  /** Share of land in drought, 0..1. */
  drought: number;
  /** Share of river tiles in flood, 0..1. */
  flood: number;
}

export interface ClockView {
  tick: number;
  day: number;
  year: number;
  hour: number;
  season: string;
  daytime: boolean;
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
  migration: [number, number] | null;
  view: ViewWindow;
  organisms: OwnOrganism[];
  nearby: NearbyOrganism[];
  leaderboard: Leaderboard;
  offers: OfferView[];
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

export type AlertKind = 'daily_summary' | 'birth' | 'death' | 'milestone' | 'need';

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
  organisms: number;
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
  alive: number;
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

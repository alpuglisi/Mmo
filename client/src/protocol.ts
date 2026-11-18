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
  /**
   * Order the possessed organism `organism` to attack `target` (another player's or a wild organism within sight):
   * it pursues and strikes by the usual strike rules, and its pack-mates nearby join. Refused: `own_target`,
   * `target_out_of_sight`, `too_big` (detail `{mass, limit}`), `sheltered`, `newcomer_grace` (detail `{days_left}`).
   * The target's owner gets an `attack` alert; a move order or a release ends it. Data: `{organism, target}`.
   */
  | { type: 'possess_attack'; organism: OrgId; target: OrgId }
  | { type: 'request_mating'; mine: OrgId; partner: OrgId; price?: number }
  /** What a mating of `mine` with `partner` would give; data: `MatingPreviewView`. */
  | { type: 'preview_mating'; mine: OrgId; partner: OrgId }
  | { type: 'respond_mating'; request: number; accept: boolean; partner?: OrgId }
  | { type: 'complete_mating'; request: number }
  | { type: 'cancel_mating'; request: number }
  /**
   * Group command: group `group` (default 0, the main group) migrates; replaces its territory or rally. Off-map
   * targets are refused (`off_map`); one on deep water or a cliff moves to the nearest land (`moved`, `note`) or is
   * refused (`unreachable`). The order ends once most of the group has arrived (a `group` alert). Data:
   * `{x, y, group, route?, moved?, note?}`.
   */
  | { type: 'migrate'; x: number; y: number; group?: number }
  | { type: 'clear_migration'; group?: number }
  /** Group `group` (default 0) holds a circle (radius kept within its `max_radius`); data: `TerritoryView`. */
  | { type: 'claim_territory'; group?: number; x: number; y: number; radius: number }
  | { type: 'abandon_territory'; group?: number }
  /** Group `group` (default 0) gathers at a point and holds near it until the rally lapses, at least 5 real minutes (replaces its migration or territory); data: `RallyView`. */
  | { type: 'rally'; group?: number; x: number; y: number }
  | { type: 'cancel_rally'; group?: number }
  /** Divide a group in two (the named organisms, else the far half across its widest spread); data: `{group, moved, kept}`. */
  | { type: 'split_group'; group?: number; organisms?: OrgId[] }
  /** Every member of `from` joins `into` (default 0); data: `{moved}`. */
  | { type: 'merge_groups'; from: number; into?: number }
  /**
   * Retire your living lineage `lineage` to start over: its animals, cohorts and eggs leave the world (removed, not
   * killed or turned wild) and you may found again at once. Data: `{removed}`.
   */
  | { type: 'retire_lineage'; lineage: number }
  /** End your lineage's newcomer grace now (`no_grace` when there is none). */
  | { type: 'end_grace' }
  | { type: 'set_view'; x: number; y: number; radius: number }
  | { type: 'clear_view' }
  /** Put one of your adults on the stud marketplace; data: `{ listing: ListingView }`. */
  | { type: 'create_listing'; organism: OrgId; fee: number; expires_days?: number }
  | { type: 'withdraw_listing'; listing: number }
  /** Buy one mating with a listed organism for your adult `mine`; data: `{ offer: OfferView }`. */
  | { type: 'buy_listing'; listing: number; mine: OrgId }
  /** Auction one mating with one of your adults (closes after `ticks`, else `days`, default 1); data: `{ listing }`. */
  | { type: 'create_auction'; organism: OrgId; reserve: number; days?: number; ticks?: number }
  /** Bid on an auction for your adult `mine` (held in escrow, refunded when outbid); data: `{ listing }`. */
  | { type: 'bid'; listing: number; amount: number; mine: OrgId }
  /** One page of the open listings (with compatibility against `mine`); data: `MarketView`. */
  | ({ type: 'browse_market' } & MarketQuery)
  /** Balance and recent ledger entries; data: `LedgerView`. */
  | { type: 'ledger'; limit?: number }
  /** Block or unblock a player's mating requests, or decline them all; no fields reads the settings. Data: `{blocked, auto_decline}`. */
  | { type: 'mating_prefs'; block?: PlayerId; unblock?: PlayerId; auto_decline?: boolean; blocked?: PlayerId[] }
  /** The renown shop's prices; data: `ShopView`. */
  | { type: 'renown_shop' }
  /** Buy an extra directed-mating slot for the rest of the weather season; data: `ShopView`. */
  | { type: 'buy_mating_slot' }
  /** Reinforcements for a small lineage (adults copied from its own); data: `{ added: OrgId[], shop: ShopView }`. */
  | { type: 'reinforce' }
  | { type: 'provision' }
  | { type: 'substitute_mating'; request: number; mine: OrgId }
  /** Which lineages yours can breed with; data: `CompatibilityView`. */
  | { type: 'compatibility' }
  /** Offer a mating of your `mine` with any suitable animal of another player's lineage (its owner picks); data: `{ offer }`. */
  | { type: 'offer_to_lineage'; mine: OrgId; lineage: number; price?: number }
  /** Buy or change your nameplate (cosmetic); data: `{ nameplate, shop }`. */
  | { type: 'set_nameplate'; color: string; title: string }
  /** Name a species your lineage founded (cosmetic); data: `{ species, name }`. */
  | { type: 'name_species'; species: number; name: string }
  /** Every player's nameplate; data: `{ nameplates: { [player]: { color, title } } }`. */
  | { type: 'nameplates' }
  /** Protocol 2: the delta states stopped decoding; the next state is a keyframe (no reply). */
  | { type: 'resync' }
  /**
   * Get the ranked season under way as `season` messages (rows `0..limit` per category, default 5, plus your
   * own row): one at once, then one per change (at most about once a second). `on: false` stops them; a new
   * connection starts without.
   */
  | { type: 'watch_season'; on: boolean; limit?: number };

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

/** One finished mating of `GET /api/matings/history`, newest first (an offer, or a mating within the player's lineage). */
export interface MatingRecord {
  tick: number;
  offer?: OfferView;
  mine: OrgId;
  partner: OrgId;
  outcome?: MatingOutcome;
  error?: ErrorInfo;
}

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
  /** While open: the tick a pending request expires unanswered, or an accepted agreement lapses (servers before
   * wall-clock lifetimes: absent, offers lapse at the season change). */
  expires_tick?: number;
  /** An offer to any animal of this lineage: `partner` is 0 until accepted. */
  lineage?: number;
  /** An accepted agreement's escort: where its two animals are (8-tile cells) and how far apart; they walk to meet
   * on their own. */
  meet?: {
    mine_x: number; mine_y: number; partner_x: number; partner_y: number; distance: number;
    /** The meeting point (dry land near the midpoint, fixed at acceptance). */
    meet_x?: number; meet_y?: number;
    /** Both asleep: the meeting (and its deadline) waits until they wake. */
    asleep?: boolean;
    /** One is possessed: the other only walks to the meeting point and waits. */
    possessed?: boolean;
  };
  /** The requester's animal died after acceptance (how): they may send another (`substitute_mating`) before `expires_tick`. */
  lost?: string;
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
  /** Pinned with `set_view` (stays there until `clear_view`); absent when it follows the lineage. */
  pinned?: boolean;
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
  | 'group' | 'wander' | 'migrate' | 'follow_parent' | 'guard' | 'ordered' | 'dormant' | 'cache' | 'sentry' | 'patrol' | 'defend' | 'rally' | 'home';

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
  /** The group it belongs to (absent: 0, the main group). */
  group?: number;
  /**
   * Facing, one of 32 directions: `k · 11.25°` from +x (east) toward +y
   * (south). Own organisms only (protocol 3 or JSON states; absent from older servers).
   */
  heading?: number;
  /** The eyes' fixed geometry, for the vision cones. Own organisms only (absent from older servers). */
  vision?: VisionView;
}

/**
 * `vision::VisionView`: one organism's eyes, fixed at birth. Angles are whole
 * degrees on the head, 0 = straight ahead, +90 = right. Only ever sent for
 * the player's own organisms.
 */
export interface VisionView {
  /** 1–6; an odd count puts one eye on the midline, pair k sits at ±min(170°, azimuth + 50°·k). */
  eyes: number;
  /** The first pair's azimuth, 0–180. */
  azimuth: number;
  /** Each eye's field width (140 + 60 × eye size). */
  field: number;
  /** Degrees of the binocular field a long snout hides (absent: 0). */
  occlusion?: number;
  /** Seen by at least one eye, whole percent. */
  coverage: number;
  /** Depth field: degrees within ±60° of ahead seen by two or more eyes, less the snout's occlusion. */
  binocular: number;
  /** The unseen arc straight behind. */
  blind: number;
  /** The unseen arc straight ahead (absent: 0). */
  front_gap?: number;
  /** Directional vision is on in this world: blind spots shorten detection (absent: off, the cones only describe the eyes). */
  directional?: boolean;
}

/** A territory a group holds. */
export interface TerritoryView {
  x: number;
  y: number;
  radius: number;
  claimed_tick: number;
}

/** A rally a group is answering. */
export interface RallyView {
  x: number;
  y: number;
  /** Members farther than this walk to the point; inside it they hold. */
  radius: number;
  /** Tick at which the order lapses. */
  expires_tick: number;
}

/** One of the player's groups. */
export interface GroupView {
  /** 0 is the main group; `split_group` makes the others (up to 4). */
  id: number;
  members: number;
  adults: number;
  migration?: [number, number];
  /** The way the migration should go (around water and thin ice), from the group's middle to the target (absent: none planned). */
  route?: [number, number][];
  /** Tick at which the migration ends if the group hasn't gathered at the target (absent: no migration). */
  migration_deadline_tick?: number;
  /** The home ground the group keeps where its last migration arrived, until its next migration: members walk back into it and forage near it; a rally or territory holds the group elsewhere meanwhile, then it goes back home. */
  home?: { x: number; y: number; radius: number };
  /** How the group's last order ended on its own ("reached (40, 46)", "migration to (60, 4) ended: never got going"…), until its next order. */
  last_outcome?: string;
  territory?: TerritoryView;
  rally?: RallyView;
  /** Largest territory radius the group could claim now. */
  max_radius: number;
  /** First tick at which it may claim again (absent: now). */
  claim_ready_tick?: number;
  /** Mean obedience gene of its members (0–1; absent from older servers): how strongly it heeds orders. */
  obedience?: number;
  /** Its territory is the player's home range, shielded while they are offline (absent: no). */
  home_range?: boolean;
  /** On the home range: minutes of offline protection left (at most 12 hours in any 24, however often you reconnect; absent from older servers). */
  protection_left_min?: number;
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
  /** Its lineage is a newcomer under its grace: no other player's animal may attack it, nor it theirs. */
  protected?: true;
  /** Down in its burrow: attack orders are refused and hunters mostly can't reach it. */
  burrowed?: true;
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
  // Morphology (docs/morphology-contract.md), quantized; absent from older
  // servers (the sprite then draws today's defaults, `creature.ts` `morphOf`).
  /** Eye count, 1–6. */
  eyes?: number;
  /**
   * Eye placement bucket: 0 forward (under 35°), 1 the default (absent), 2 on
   * the sides (85° and more). Sent only when not the default; drawn
   * by `world/creature.ts` (forward near the snout, side high and back).
   */
  eye_azimuth?: SizeBucket;
  /** Eye size bucket: 0 small, 1 medium, 2 large. */
  eye_size?: SizeBucket;
  /** Ear count: 0, 2 or 4. */
  ears?: number;
  ear_shape?: EarShape;
  /** Ear size bucket. */
  ear_size?: SizeBucket;
  /** Leg count: 0, 2, 4, 6 or 8. */
  legs?: number;
  tail?: TailLook;
  /** Tail length bucket: 0 short, 1 medium, 2 long. */
  tail_size?: SizeBucket;
  /** Snout length bucket. */
  snout?: SizeBucket;
  /** Fangs: 0 none, 1 small, 2 large. */
  fangs?: SizeBucket;
  /** Horns (Phase 3): 0 none (absent), 1 small, 2 large. */
  horns?: SizeBucket;
  /** Horn form; absent for curved (the default) or no horns. */
  horn_form?: HornForm;
  /** Wing shape of a glider or flier: 0 broad, 1 default (absent), 2 narrow. */
  wing_aspect?: SizeBucket;
  /** Coat pattern; absent for plain. */
  pattern?: CoatPattern;
}

export type SizeBucket = 0 | 1 | 2;
export type HornForm = 'curved' | 'straight' | 'branched';
export type CoatPattern = 'plain' | 'spots' | 'stripes' | 'mottled';
export type EarShape = 'round' | 'pointed' | 'long' | 'fan';
export type TailLook = 'none' | 'thin' | 'bushy' | 'fin' | 'club' | 'prehensile';

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
  /** The main group's migration order (each group's is in `groups`). */
  migration: [number, number] | null;
  /** The player's groups by id (absent from older servers). */
  groups?: GroupView[];
  view: ViewWindow;
  organisms: OwnOrganism[];
  nearby: NearbyOrganism[];
  leaderboard: Leaderboard;
  offers: OfferView[];
  /** Renown available to spend (absent from servers before the economy). */
  balance?: number;
  /** Renown of yours held in escrow on open agreements. */
  escrow?: number;
  /** Your lineage's newcomer grace while it lasts (absent once over): other players can't attack it, and it can't attack theirs. */
  grace?: GraceView | null;
  /** Lineages your animals have learned are poisonous: theirs "look poisonous to you". */
  poisonous?: number[];
}

/** The newcomer grace left: game days, and at least some real minutes, whichever is longer. */
export interface GraceView {
  /** Game days left, to a tenth. */
  days_left: number;
  /** The tick it ends on. */
  until_tick: number;
  /** Real seconds left at the server's clock, rounded up to tens. */
  real_seconds_left?: number;
}

export interface WelcomeMsg {
  type: 'welcome';
  player: PlayerId;
  name: string;
  lineage: number | null;
  /** The protocol the connection speaks: 2 for binary delta states (wire.ts); absent for whole JSON states. */
  proto?: number;
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

export type AlertKind = 'daily_summary' | 'birth' | 'death' | 'milestone' | 'need' | 'world' | 'achievement' | 'market' | 'group' | 'attack';

/** Every alert kind, in the server's order. */
export const ALERT_KINDS: readonly AlertKind[] = ['daily_summary', 'birth', 'death', 'milestone', 'need', 'world', 'achievement', 'market', 'group', 'attack'];

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
export const DEFAULT_ALERT_PREFS: AlertPrefs = { hidden: [], toast: ['death', 'need', 'world', 'market', 'group', 'attack'] };

/** What a push notification is about (crates/server/src/protocol.rs `PushKind`). */
export type PushKind = 'mating' | 'death' | 'market' | 'species' | 'season' | 'catastrophe';

/** Every push kind, in the server's order (most important first). */
export const PUSH_KINDS: readonly PushKind[] = ['mating', 'death', 'market', 'species', 'season', 'catastrophe'];

/** `GET` / `POST /api/push/prefs`: the push kinds switched off (every kind is on by default). */
export interface PushPrefs {
  off: PushKind[];
}

/** The push service of a native app's device token (crates/server/src/protocol.rs `NativeService`): FCM for Android, APNs for iOS. */
export type NativeService = 'fcm' | 'apns';

/** `GET /api/push/key`: whether the server sends push notifications, the VAPID key to subscribe with, and the native services the apps can register with. */
export interface PushKeyView {
  enabled: boolean;
  public_key?: string;
  kinds: PushKind[];
  /** Left out when no native push service is configured. */
  native?: NativeService[];
}

/** One app device (`GET /api/push/subscriptions` `devices`, `POST /api/push/native/subscribe`); the token stays on the server. */
export interface PushDeviceView {
  service: NativeService;
  created_unix: number;
  label?: string;
}

/** One subscribed device (`GET /api/push/subscriptions`, `POST /api/push/subscribe`). */
export interface PushSubscriptionView {
  endpoint: string;
  created_unix: number;
  label?: string;
}

/** What the service worker receives in a push (crates/server/src/webpush/service.rs `Payload`). */
export interface PushPayload {
  title: string;
  body: string;
  world: string;
  tab: string;
  kind: PushKind;
  count: number;
  tag: string;
  organism?: number;
}

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

/** The ranked season under way, to a connection that sent `watch_season` (the body of `GET /api/seasons/current`). */
export interface SeasonMsg extends CurrentSeason {
  type: 'season';
}

export type ServerMsg = WelcomeMsg | StateMsg | ResultMsg | MatingRequestMsg | MatingUpdateMsg | AlertMsg | SessionEndedMsg | SeasonMsg;

// ---------------------------------------------------------------- economy and marketplace
// Data of the `ledger`, `browse_market` and `create_listing` commands.

export type LedgerKind = 'grant' | 'income' | 'achievement' | 'escrow' | 'sale' | 'refund' | 'royalty' | 'purchase';

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

/** Order of a `browse_market` page. */
export type MarketSort = 'newest' | 'price_asc' | 'price_desc' | 'ending' | 'fertility';

/** The fields of `browse_market` (all optional). */
export interface MarketQuery {
  mine?: OrgId;
  /** Listings per page (server default 20, at most 50). */
  limit?: number;
  /** The previous page's `next_cursor`. */
  cursor?: string;
  sort?: MarketSort;
  kind?: 'fixed' | 'auction';
  role?: 'stud' | 'dam';
  /** Highest price: the fee, or an auction's current bid (its reserve before any). */
  max_price?: number;
  lineage?: number;
  species?: number;
  /** With `mine`: only listings it can conceive with. */
  compatible?: boolean;
  hide_own?: boolean;
  /** Also return all your own listings as `own`. */
  with_own?: boolean;
}

/** What a bidder sees of an auction. */
export interface AuctionView {
  reserve: number;
  deadline_tick: number;
  high_bid?: number;
  high_bidder?: PlayerId;
  bids: number;
  /** The smallest bid accepted now. */
  min_bid: number;
}

/** A stud (or dam) on the marketplace: a fixed fee, or an auction. */
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
  /** Whether your lineage can breed with it at all (body plans 0.30 apart are sterile); absent on older servers. */
  compatible?: boolean;
  /** Best body-plan fertility of a sample of your adults with it, 0..1. */
  body_plan_fertility?: number;
  /** `fixed` when absent (servers before auctions). */
  kind: 'fixed' | 'auction';
  auction?: AuctionView;
}

/** One page of `browse_market`. */
export interface MarketView {
  balance: number;
  escrow: number;
  slots_left: number;
  /** In the requested order. */
  listings: ListingView[];
  /** Listings matching the filters on every page (the page's length from servers before paging). */
  total: number;
  /** Pass back as `cursor` for the next page; absent on the last page. */
  next_cursor?: string;
  /** All your own listings (`with_own`). */
  own?: ListingView[];
}

// ---------------------------------------------------------------- HTTP bodies

export interface AuthResponse {
  token: string;
  player: PlayerId;
  name: string;
}

export type ThermoSpec = 'warm' | 'cold';
export type BirthSpec = 'live' | 'incubated_eggs' | 'abandoned_eggs';

/** One world of `GET /api/worlds`. */
export interface WorldListing {
  /** What `?world=` takes. */
  id: string;
  name: string;
  width: number;
  height: number;
  day: number;
  year: number;
  season: string;
  population: number;
  lineages: number;
  players_online: number;
}

/** `GET /api/worlds`: the worlds the server hosts, the default first. */
export interface WorldList {
  default: string;
  worlds: WorldListing[];
}

/** `DesignSpec`: the body of `POST /api/lineage`. */
export interface DesignSpec {
  name: string;
  thermo: ThermoSpec;
  birth: BirthSpec;
  litter_size: number;
  /** Trait values in [0, 1] that differ from the defaults. */
  traits: Record<string, number>;
  founders: number;
  /**
   * The body plan's new parameters (docs/morphology.md, section 7). Optional:
   * missing fields take their defaults. Only sent to a server whose
   * `/api/genes` lists a `morph` section (`designMorph.ts`).
   */
  morph?: MorphSpec;
}

/** `DesignSpec.morph`: counts, shapes and axes not carried by `traits`. */
export interface MorphSpec {
  eye_count?: number;
  eye_azimuth?: number;
  ear_count?: number;
  ear_shape?: string;
  leg_count?: number;
  tail_shape?: string;
  snout_length?: number;
  /** 0–1; horns show from 0.1 (priced per unit). */
  horn_size?: number;
  horn_form?: string;
  /** 0 broad to 1 narrow. */
  wing_aspect?: number;
  pattern?: string;
}

/** One row of `GET /api/genes`' `morph` section. */
export interface MorphInfo {
  /** Wire name (`"eye_count"`, `"ear_shape"`, …). */
  name: string;
  /** `count`, `enum` or `axis`. */
  kind: string;
  /** Allowed values (counts or shape names); absent for an axis. */
  values?: (number | string)[];
  min?: number;
  max?: number;
  default: number | string;
  /**
   * For a count: points per step above the default (one eye; a pair of legs
   * or ears), nothing below it. Shapes and axes are free (0).
   */
  cost: number;
}

export interface FoundResponse {
  lineage: number;
  founders: OrgId[];
  cost: number;
  /** Founders the design asked for (more than `founders` on a re-founding soon after a lineage ended). */
  requested?: number;
  /** Why fewer were founded. */
  reduced?: string;
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
  /** Founders up to which each extra costs `cost_per_extra_founder`; past it
   * each costs `founder_cost_step` more than the last (absent on older servers:
   * a flat price). */
  flat_founders?: number;
  founder_cost_step?: number;
  cost_per_extra_offspring: number;
  /** Points for a cold-blooded design (absent on older servers: 0). */
  cost_cold_blooded?: number;
  /** Points per unit of `body_size` below its default (absent on older servers: 0). */
  cost_per_smaller_body?: number;
  /** Body size up to which a bigger body is free (absent on older servers: no price). */
  free_body_size?: number;
  /** Points per unit of `body_size` above `free_body_size` (absent on older servers: 0). */
  cost_per_larger_body?: number;
  max_litter: number;
  genes: GeneInfo[];
  /** The body plan's parameters, on servers that accept `DesignSpec.morph`. */
  morph?: MorphInfo[];
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
  /** Whole days until winter, 0 during winter (absent from older servers). */
  days_to_winter?: number;
  /** A lineage founded now starts fed with full fat reserves (winter provision). */
  winter_provision?: boolean;
  /** Re-founding within this many game days of a lineage's end (about `refound_window_minutes` real minutes) founds half the founders. */
  refound_window_days?: number;
  refound_window_minutes?: number;
  /** The re-founding rule in words (shown verbatim). */
  refound_rule?: string;
  /** Most living organisms, young included, one player may have; at it births pause (absent from older servers). */
  lineage_cap?: number;
  /** No births past this many living members (with crowding, a multiple of `lineage_cap`, which is then the soft cap). */
  lineage_hard_cap?: number;
  /** Player lineages in their newcomer grace (others can't order attacks on them). */
  protected?: { player: PlayerId; lineage: number; days_left: number }[];
  /** Days in a year (four equal seasons); absent from older servers. */
  days_per_year?: number;
  /** Ticks in a game day; absent from older servers. */
  ticks_per_day?: number;
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
  /** Most plant food each tile holds, 0..255 for none..the map's richest tile (absent from older servers). */
  veg_max?: string;
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
  /** Ripe fruit, seeds and nuts, 0..255 for 0..8 food units; absent while no tile bears any. */
  fruit?: string;
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
  /** Deaths by cause: starved, thirst, exposure, eaten, toxin, drowned, old_age, attrition, recessive, disease, journey, winter_sleep. */
  deaths: Record<string, number>;
  /** Eggs lost during the day by cause: chilled, overheated, raided (absent: none). */
  eggs_lost?: Record<string, number>;
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
  /** How the legacy share comes about: genes, not heads (absent on older servers). */
  score?: ScoreBreakdown;
  /** The soft lineage cap and how crowded the lineage is (absent on older servers). */
  crowding?: CrowdingView;
  /** The founding ramp: how much of the share counts toward the ranked season now (absent from older servers). */
  ramp?: { founded_day: number; weight: number; days: number };
  /** Days left of the newcomer grace (others can't order attacks on yours); absent: none. */
  grace_days_left?: number;
  /** Lineage-wide warnings now: `starving`, `near_cap`, `territory_starving` (absent from older servers). */
  warnings?: { kind: string; text: string }[];
  /** Egg-laying lineages: their clutches on the ground (absent for live bearers and older servers). */
  eggs?: {
    clutches: number;
    eggs: number;
    brooded: number;
    /** Eggs lost since founding by cause: chilled (no parent warming them), overheated, raided (absent: none). */
    lost?: Record<string, number>;
  };
  /** How the lineage ended once it has (absent while it lives, and on older servers). */
  ended?: 'retired' | 'died_out';
  legacy: LegacyPoint[];
  alerts: AlertView[];
}

/**
 * `lineage_stats.score`. Every share is of all alleles in the world's breeding adults, as the legacy
 * share is. `share = home_share + exported_share`; `head_share = home_share + foreign_share`.
 */
export interface ScoreBreakdown {
  day: number;
  /** The legacy share: your alleles wherever they live. */
  share: number;
  /** Your lineage's share of all alleles, whoever's: its head count among breeding adults. */
  head_share: number;
  /** Your alleles in your own lineage. */
  home_share: number;
  /** Your alleles in rivals' animals and the wild. */
  exported_share: number;
  /** The part in other players' lineages: what the royalty pays for (absent on older servers). */
  traded_share?: number;
  /** Others' alleles your lineage carries: what hybridising cost. */
  foreign_share: number;
  /** Whose alleles your breeding adults carry; `share` of your lineage's alleles. */
  carried: { player: PlayerId; name: string; share: number }[];
  /** Where your alleles live; `share` of your alleles. */
  placed: { lineage: number; lineage_name: string; owner: PlayerId; owner_name: string; share: number }[];
  /** Renown a day the royalty pays now. */
  royalty: number;
}

/** `lineage_stats.crowding`: breeding slows toward `soft_cap` (half pace at it) and stops at `hard_cap`. */
export interface CrowdingView {
  soft_cap: number;
  hard_cap: number;
  /** Adults plus juveniles at `juvenile_weight`. */
  count: number;
  juvenile_weight: number;
  /** Litter multiplier (1: uncrowded). */
  litter: number;
  /** Hunger multiplier (1 below the soft cap). */
  hunger: number;
}

/** `renown_shop`: what renown buys. */
export interface ShopView {
  balance: number;
  slots_left: number;
  slots_bought: number;
  /** Price of the next extra slot (absent at the limit). */
  slot_price?: number;
  reinforce_size: number;
  reinforce_price: number;
  reinforce_max_alive: number;
  /** Why reinforcements aren't possible now. */
  reinforce_blocked?: string;
  market_fee_percent: number;
  royalty_per_point: number;
  reinforcements_bought?: number;
  reinforcements_max?: number;
  nameplate_price?: number;
  nameplate_colors?: string[];
  nameplate?: { color: string; title: string };
  species_name_price?: number;
  /** A provision (a sink for small lineages): its price, provisions within the rolling window and the most allowed, and why not now. */
  provision_price?: number;
  /** Adults a provision would feed now (the price is per adult). */
  provision_size?: number;
  provisions_bought?: number;
  provisions_max?: number;
  provision_blocked?: string;
  /** When the next reinforcement is possible if the window is full: the day and about how many real minutes from now. */
  reinforce_next_day?: number;
  reinforce_next_minutes?: number;
}

/** One lineage in a compatibility check (`compatibility`, `POST /api/design/compatibility`). */
export interface CompatibilityRow {
  lineage: number;
  lineage_name: string;
  owner: PlayerId;
  owner_name: string;
  /** The wild ecosystem's lineage (owner 0); a player's lineage never is. Absent on older servers. */
  wild?: boolean;
  /** Mean fertility over every pair of sampled individuals (the best pair's on older servers). */
  fertility: number;
  /** 10th and 90th percentile pair (absent on older servers). */
  fertility_low?: number;
  fertility_high?: number;
  best_fertility?: number;
  /** Share of sampled pairs with any fertility. */
  fertile_share?: number;
  /** At least half the pairs fertile (any fertile pair on older servers). */
  compatible: boolean;
  /** `none`, `low` or `high`: how likely drift is to make the two (more) fertile. */
  drift_risk?: 'none' | 'low' | 'high';
  /** `carnivory`, `endothermy`, `birth_mode`, `body_size` or `morphology`. */
  limiting: string;
  gap: number;
}

/** `compatibility`: which lineages yours can breed with. */
export interface CompatibilityView {
  lineage?: number;
  rows: CompatibilityRow[];
}

/** `POST /api/design/compatibility`: a design's founders against each living lineage. */
export interface DesignCompatView {
  day: number;
  sample: number;
  fertile_with: number;
  at_risk: number;
  rows: CompatibilityRow[];
  /** Share of today's shores the founders can stand on and drink from (steep banks stop non-climbers); absent on older servers. */
  shore_access?: number;
  /** Set when the founders can reach less than half the shores. */
  water_warning?: string;
  /** Set for a warm-blooded design that abandons its eggs (they chill below 24 °C and almost never hatch): what to do instead. */
  egg_warning?: string;
  /** Incubated eggs on a meat eater (carnivory ≥ 0.5): a roaming hunter leaves its clutch (shown verbatim). */
  carnivore_egg_warning?: string;
  /** Balance's drought-founding warning, when the server sends one (shown verbatim). */
  drought_warning?: string;
  /** Founding now is a re-founding soon after your last lineage ended: `founders` of `requested` (founders cost double points: as many as fit, at least half), costing
   * `cost` points, until `until_day` (about `minutes_left` real minutes). */
  refound?: { founders: number; requested: number; cost: number; until_day: number; minutes_left: number; reason: string };
  /** Why founding would refuse the design's name (it looks like another player's); absent when fine. */
  name_warning?: string;
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

// ---------------------------------------------------------------- ranked seasons
// Fixed-length ranked seasons (`GET /api/seasons`, `/api/seasons/current`,
// `/api/seasons/:n`, `/api/seasons/player/:player`), not the weather seasons
// above.

export type SeasonCategory = 'legacy' | 'lineage' | 'species' | 'market' | 'survival';

export interface SeasonPlacement {
  player: PlayerId;
  name: string;
  /** Competition rank: equal values share a place. */
  rank: number;
  /** Legacy: mean share (0–1); lineage: peak members; species: founded; market: renown; survival: catastrophes. */
  value: number;
}

export interface SeasonWinner {
  category: SeasonCategory;
  player: PlayerId;
  name: string;
  value: number;
}

export interface RankedSeasonHead {
  world_id: string;
  season: number;
  first_day: number;
  last_day: number;
  length: number;
  days: number;
  ended_tick: number;
  ended_unix: number;
  players: number;
  winners: SeasonWinner[];
  summary: string;
}

/** `GET /api/seasons`: newest first. */
export interface RankedSeasonList {
  total: number;
  offset: number;
  seasons: RankedSeasonHead[];
  /** Titles and crowns per player over every finished season (absent from older servers: count from the page). */
  hall_of_fame?: { player: PlayerId; name: string; crowns: number; titles: number }[];
}

export interface StandingPage {
  category: SeasonCategory;
  /** Players ranked in the category. */
  total: number;
  /** The asked-for page, plus the asked-for player's row if outside it. */
  rows: SeasonPlacement[];
}

/** `GET /api/seasons/:n`. */
export interface RankedSeasonPage extends RankedSeasonHead {
  offset: number;
  limit: number;
  standings: StandingPage[];
}

/** `GET /api/seasons/current`. */
export interface CurrentSeason {
  world_id: string;
  season: number;
  first_day: number;
  last_day: number;
  length: number;
  days: number;
  days_left: number;
  offset: number;
  limit: number;
  standings: StandingPage[];
}

export interface PlayerPlacement {
  category: SeasonCategory;
  rank: number;
  value: number;
  /** Players ranked in the category. */
  of: number;
  /** Won the category's title (one of the season's winners; a tie can be first without one). Absent from older servers. */
  title?: boolean;
}

export interface PlayerSeason {
  world_id: string;
  season: number;
  first_day: number;
  last_day: number;
  ended_unix: number;
  placements: PlayerPlacement[];
}

/** `GET /api/seasons/player/:player`: newest first. */
export interface PlayerSeasonHistory {
  player: PlayerId;
  seasons: PlayerSeason[];
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
  /** The owner's player name (absent from older servers). */
  owner_name?: string;
  adults: number;
}

export interface SpeciesView {
  id: number;
  name: string;
  /** Who may name it (`name_species`; absent on older servers). */
  namer?: PlayerId;
  /** A player has named it: not offered for naming again. */
  named?: boolean;
  origin: 'initial' | 'founding' | 'split' | string;
  parent?: number;
  founded_day: number;
  extinct_day?: number;
  merged_into?: number;
  /** The name of the species it merged into. */
  merged_into_name?: string;
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
  /** Mean thermoregulation and birth mode genes, usual leg and eye counts (absent from older servers). */
  endothermy?: number;
  birth_mode?: number;
  leg_count?: number;
  eye_count?: number;
}

/** A group's typical body plan: counts and shapes by majority, axes by mean (server `BodyPlanMode`). */
export interface BodyPlanMode {
  eye_count: number;
  eye_azimuth: number;
  ear_count: number;
  ear_shape: string;
  leg_count: number;
  tail_shape: string;
  snout_length: number;
  horn_size: number;
  horn_form: string;
  wing_aspect: number;
  pattern: string;
}

/** The caller's lineage on one recorded day (or today). */
export interface FamilySample {
  day: number;
  alive: number;
  /** Mean expressed value of every gene, by wire name. */
  genes: Record<string, number>;
  /** Absent in records from before the server kept it. */
  morph?: BodyPlanMode;
}

/** A species the caller's lineage belongs or belonged to (or an ancestor of one). */
export interface FamilySpecies {
  id: number;
  name: string;
  origin: 'initial' | 'founding' | 'split' | string;
  parent?: number;
  founded_day: number;
  extinct_day?: number;
  merged_into?: number;
  adults: number;
  /** Of them, the caller's lineage's (0: an ancestor only). */
  mine: number;
  genes: Record<string, number>;
  morph: BodyPlanMode;
}

/** `GET /api/lineage/tree`: the family tree view's data. */
export interface FamilyTree {
  player: PlayerId;
  lineage: number | null;
  name: string | null;
  day: number;
  /** Living members now (0 with a lineage: it died out). */
  alive: number;
  /** Oldest first, today last (while alive). */
  timeline: FamilySample[];
  /** Oldest first. */
  species: FamilySpecies[];
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
  /** Share at the era's end (and weighted). */
  share: number;
  weighted_share: number;
  /** The era's mean day score, which ranks it (absent for older eras, ranked by the final share). */
  mean_share?: number;
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

function isAuction(v: unknown): v is AuctionView {
  return isObj(v) && isNum(v.reserve) && isNum(v.deadline_tick) && isNum(v.min_bid) && isNum(v.bids);
}

function isListing(v: unknown): v is ListingView {
  return isObj(v) && isNum(v.id) && isNum(v.organism) && isNum(v.owner) && isNum(v.fee) && isStr(v.role)
    && isNum(v.x) && isNum(v.y) && Array.isArray(v.traits);
}

/** A listing as the client keeps it: the kind filled in, a malformed auction dropped (with the listing). */
function toListing(v: ListingView): ListingView | null {
  const raw = v as unknown as Record<string, unknown>;
  if (raw.kind === 'auction' || raw.auction !== undefined) {
    return isAuction(raw.auction) ? { ...v, kind: 'auction' } : null;
  }
  return { ...v, kind: 'fixed' };
}

function listings(v: unknown[]): ListingView[] {
  return v.filter(isListing).map(toListing).filter((l): l is ListingView => l !== null);
}

function isLedgerEntry(v: unknown): v is LedgerEntry {
  return isObj(v) && isNum(v.seq) && isNum(v.delta) && isNum(v.balance) && isStr(v.kind) && isStr(v.text);
}

/** The data of a `browse_market` result; malformed listings are dropped, a malformed reply is null. */
export function parseMarketView(data: unknown): MarketView | null {
  if (!isObj(data) || !isNum(data.balance) || !Array.isArray(data.listings)) return null;
  const page = listings(data.listings);
  const view: MarketView = {
    balance: data.balance,
    escrow: isNum(data.escrow) ? data.escrow : 0,
    slots_left: isNum(data.slots_left) ? data.slots_left : 0,
    listings: page,
    total: isNum(data.total) ? data.total : page.length,
  };
  if (isStr(data.next_cursor)) view.next_cursor = data.next_cursor;
  if (Array.isArray(data.own)) view.own = listings(data.own);
  return view;
}

/** The data of a `renown_shop` result (or `buy_mating_slot`, or `reinforce`'s `shop`). */
export function parseShopView(data: unknown): ShopView | null {
  if (!isObj(data) || !isNum(data.balance) || !isNum(data.reinforce_price) || !isNum(data.slots_left)) return null;
  return data as unknown as ShopView;
}

/** The data of a `compatibility` result. */
export function parseCompatibilityView(data: unknown): CompatibilityView | null {
  if (!isObj(data) || !Array.isArray(data.rows)) return null;
  const rows = data.rows.filter((r): r is CompatibilityView['rows'][number] =>
    isObj(r) && isNum(r.lineage) && isNum(r.fertility) && isStr(r.lineage_name) && isStr(r.owner_name));
  return { ...(isNum(data.lineage) ? { lineage: data.lineage } : {}), rows };
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
        ? ({
          type: 'welcome', player: v.player, name: v.name, lineage: (v.lineage as number | undefined) ?? null,
          ...(isNum(v.proto) ? { proto: v.proto } : {}),
        })
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
    case 'season':
      return isNum(v.season) && isNum(v.days_left) && Array.isArray(v.standings) ? (v as unknown as SeasonMsg) : null;
    default:
      return null;
  }
}

// Spectator view (`GET /api/spectate`, server `game::spectate`): a world as a
// logged-out watcher sees it, at most once a simulated day: per-lineage head
// counts on coarse cells, the most common look, public names and shares.

export interface SpectateLineage {
  lineage: number;
  name: string;
  /** The owner's public name, or "wild". */
  owner: string;
  wild: boolean;
  alive: number;
  adults: number;
  /** The owner's allele share (player lineages only). */
  share?: number;
  /** The most common look of its adults; absent once extinct. */
  look?: Appearance;
  /** `[cell x, cell y, count]` on `Spectate.cell`-tile cells (wild) or `player_cell` (players'). */
  cells: [number, number, number][];
  /** Under its newcomer grace: counted, but not placed on the map (no cells). */
  hidden?: boolean;
}

export interface Spectate {
  clock: ClockView;
  width: number;
  height: number;
  /** Cell side in tiles of the wild lineages. */
  cell: number;
  /** Cell side in tiles of the players' lineages (coarser: as a market listing's position). */
  player_cell?: number;
  /** Catastrophes under way. */
  catastrophes: CatastropheView[];
  lineages: SpectateLineage[];
}

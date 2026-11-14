// HTTP API calls. Paths are relative, so the client works when evo-server
// serves it (`--static client/dist`) and behind the Vite dev proxy. A
// Capacitor build sets the server origin with `setApiBase`.
//
// A server may host several worlds: calls about one world (and the
// WebSocket) carry `world=<id>` for the world chosen with `setWorld`; with
// none chosen they go to the server's default world.

import type {
  AchievementList, AlertPrefs, ApiErrorBody, AuthResponse, CatastropheList, CurrentSeason, DesignSpec, EraList, FoundResponse, GeneCatalog, Leaderboard,
  LineageStats, NativeService, PlayerId, PlayerSeasonHistory, PushDeviceView, PushKeyView, PushPrefs, PushSubscriptionView, RankedSeasonList, RankedSeasonPage, SeasonBoard, SpeciesList, TerrainDynamic, TerrainView, WorldList,
  WorldSummary,
} from './protocol';
import { PROTO_DELTA } from './wire';

let base = '';

export function setApiBase(origin: string): void {
  base = origin.replace(/\/$/, '');
}

/** The server origin in use ('' when it is the page's own). */
export function apiBase(): string {
  return base;
}

export function apiUrl(path: string): string {
  return base + path;
}

let world = '';

/** Choose the world that per-world calls and the socket use ('' for the server's default). */
export function setWorld(id: string): void {
  world = id;
}

/** The chosen world ('' when the server's default). */
export function currentWorld(): string {
  return world;
}

/** `path` with `world=<id>` added when a world is chosen. */
export function withWorld(path: string, id: string = world): string {
  if (!id) return path;
  return `${path}${path.includes('?') ? '&' : '?'}world=${encodeURIComponent(id)}`;
}

/**
 * WebSocket URL for a session token, on the same origin as the API. Asks
 * for protocol 2 (binary delta states); a server without it ignores the
 * parameter and sends JSON states, which the socket handles too.
 */
export function wsUrl(token: string): string {
  const origin = base || window.location.origin;
  return withWorld(`${origin.replace(/^http/, 'ws')}/ws?token=${encodeURIComponent(token)}&proto=${PROTO_DELTA}`);
}

export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string, public body: ApiErrorBody | null) {
    super(message);
  }
}

async function request<T>(method: string, path: string, body?: unknown, token?: string): Promise<T> {
  const headers: Record<string, string> = {};
  if (body !== undefined) headers['content-type'] = 'application/json';
  if (token) headers.authorization = `Bearer ${token}`;
  let res: Response;
  try {
    res = await fetch(apiUrl(path), { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  } catch (e) {
    throw new ApiError(0, 'network', `cannot reach the server (${(e as Error).message})`, null);
  }
  const text = await res.text();
  let json: unknown = null;
  try {
    json = text ? JSON.parse(text) : null;
  } catch {
    /* not JSON (e.g. an axum extractor rejection in plain text) */
  }
  if (!res.ok) {
    const b = json as ApiErrorBody | null;
    if (b && typeof b.code === 'string') throw new ApiError(res.status, b.code, b.message ?? b.code, b);
    throw new ApiError(res.status, `http_${res.status}`, text || res.statusText, null);
  }
  return json as T;
}

// Global: accounts, alert and push preferences, push subscriptions, the gene catalog, the world list.
// Everything else is about the chosen world (`withWorld`).
export const api = {
  register: (name: string, password: string) => request<AuthResponse>('POST', '/api/register', { name, password }),
  login: (name: string, password: string) => request<AuthResponse>('POST', '/api/login', { name, password }),
  logout: (token: string) => request<null>('POST', '/api/logout', undefined, token),
  worlds: () => request<WorldList>('GET', '/api/worlds'),
  found: (token: string, design: DesignSpec) => request<FoundResponse>('POST', withWorld('/api/lineage'), design, token),
  genes: () => request<GeneCatalog>('GET', '/api/genes'),
  world: () => request<WorldSummary>('GET', withWorld('/api/world')),
  leaderboard: () => request<Leaderboard>('GET', withWorld('/api/leaderboard')),
  seasons: (limit = 8) => request<SeasonBoard[]>('GET', withWorld(`/api/leaderboard/seasons?limit=${limit}`)),
  terrain: () => request<TerrainView>('GET', withWorld('/api/terrain')),
  terrainDynamic: () => request<TerrainDynamic>('GET', withWorld('/api/terrain/dynamic')),
  lineageStats: (token: string) => request<LineageStats>('GET', withWorld('/api/lineage/stats'), undefined, token),
  alertPrefs: (token: string) => request<AlertPrefs>('GET', '/api/alerts/prefs', undefined, token),
  setAlertPrefs: (token: string, prefs: AlertPrefs) => request<AlertPrefs>('POST', '/api/alerts/prefs', prefs, token),
  // Push notifications: global (one account, every world; a notification names its world).
  pushKey: () => request<PushKeyView>('GET', '/api/push/key'),
  pushSubscriptions: (token: string) =>
    request<{ subscriptions: PushSubscriptionView[]; devices?: PushDeviceView[] }>('GET', '/api/push/subscriptions', undefined, token),
  pushSubscribe: (token: string, sub: PushSubscriptionJSON, label?: string) =>
    request<PushSubscriptionView>('POST', '/api/push/subscribe', { ...sub, label }, token),
  pushUnsubscribe: (token: string, endpoint: string) => request<{ removed: boolean }>('POST', '/api/push/unsubscribe', { endpoint }, token),
  // Native push in the apps: a device token for FCM (Android) or APNs (iOS).
  pushNativeSubscribe: (token: string, service: NativeService, deviceToken: string, label?: string) =>
    request<PushDeviceView>('POST', '/api/push/native/subscribe', { service, token: deviceToken, label }, token),
  pushNativeUnsubscribe: (token: string, service: NativeService, deviceToken: string) =>
    request<{ removed: boolean }>('POST', '/api/push/native/unsubscribe', { service, token: deviceToken }, token),
  pushPrefs: (token: string) => request<PushPrefs>('GET', '/api/push/prefs', undefined, token),
  setPushPrefs: (token: string, prefs: PushPrefs) => request<PushPrefs>('POST', '/api/push/prefs', prefs, token),
  species: () => request<SpeciesList>('GET', withWorld('/api/species')),
  eras: () => request<EraList>('GET', withWorld('/api/eras')),
  catastrophes: () => request<CatastropheList>('GET', withWorld('/api/catastrophes')),
  achievements: () => request<AchievementList>('GET', withWorld('/api/achievements')),
  // Ranked seasons of the chosen world; `map` is the generated map a finished season belongs to (its `world_id`).
  rankedSeasons: (offset = 0, limit = 10) => request<RankedSeasonList>('GET', withWorld(`/api/seasons?offset=${offset}&limit=${limit}`)),
  currentSeason: (player: PlayerId, limit = 5) => request<CurrentSeason>('GET', withWorld(`/api/seasons/current?limit=${limit}&player=${player}`)),
  rankedSeason: (map: string, season: number, player: PlayerId, offset = 0, limit = 10) =>
    request<RankedSeasonPage>('GET', withWorld(`/api/seasons/${season}?map=${encodeURIComponent(map)}&offset=${offset}&limit=${limit}&player=${player}`)),
  playerSeasons: (player: PlayerId, limit = 50) => request<PlayerSeasonHistory>('GET', withWorld(`/api/seasons/player/${player}?limit=${limit}`)),
};

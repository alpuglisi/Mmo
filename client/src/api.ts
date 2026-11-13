// HTTP API calls. Paths are relative, so the client works when evo-server
// serves it (`--static client/dist`) and behind the Vite dev proxy. A
// Capacitor build sets the server origin with `setApiBase`.

import type {
  AchievementList, AlertPrefs, ApiErrorBody, AuthResponse, CatastropheList, DesignSpec, EraList, FoundResponse, GeneCatalog, Leaderboard,
  LineageStats, SeasonBoard, SpeciesList, TerrainDynamic, TerrainView, WorldSummary,
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

/**
 * WebSocket URL for a session token, on the same origin as the API. Asks
 * for protocol 2 (binary delta states); a server without it ignores the
 * parameter and sends JSON states, which the socket handles too.
 */
export function wsUrl(token: string): string {
  const origin = base || window.location.origin;
  return `${origin.replace(/^http/, 'ws')}/ws?token=${encodeURIComponent(token)}&proto=${PROTO_DELTA}`;
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

export const api = {
  register: (name: string, password: string) => request<AuthResponse>('POST', '/api/register', { name, password }),
  login: (name: string, password: string) => request<AuthResponse>('POST', '/api/login', { name, password }),
  logout: (token: string) => request<null>('POST', '/api/logout', undefined, token),
  found: (token: string, design: DesignSpec) => request<FoundResponse>('POST', '/api/lineage', design, token),
  genes: () => request<GeneCatalog>('GET', '/api/genes'),
  world: () => request<WorldSummary>('GET', '/api/world'),
  leaderboard: () => request<Leaderboard>('GET', '/api/leaderboard'),
  seasons: (limit = 8) => request<SeasonBoard[]>('GET', `/api/leaderboard/seasons?limit=${limit}`),
  terrain: () => request<TerrainView>('GET', '/api/terrain'),
  terrainDynamic: () => request<TerrainDynamic>('GET', '/api/terrain/dynamic'),
  lineageStats: (token: string) => request<LineageStats>('GET', '/api/lineage/stats', undefined, token),
  alertPrefs: (token: string) => request<AlertPrefs>('GET', '/api/alerts/prefs', undefined, token),
  setAlertPrefs: (token: string, prefs: AlertPrefs) => request<AlertPrefs>('POST', '/api/alerts/prefs', prefs, token),
  species: () => request<SpeciesList>('GET', '/api/species'),
  eras: () => request<EraList>('GET', '/api/eras'),
  catastrophes: () => request<CatastropheList>('GET', '/api/catastrophes'),
  achievements: () => request<AchievementList>('GET', '/api/achievements'),
};

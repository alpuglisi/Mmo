import fs from 'node:fs';
import path from 'node:path';

// Configuration comes from (later wins):
//   1. built-in defaults below
//   2. marketing.config.json (gitignored) or the file named by EVO_MARKETING_CONFIG
//   3. environment variables for every secret (preferred for credentials)
// Nothing here is ever written back to disk.

export const DEFAULTS = {
  dataDir: 'data',
  outDir: 'out',
  timezone: 'UTC',
  site: { url: 'https://example.com', campaign: 'launch' },
  claude: { model: 'claude-opus-5-5', effort: 'medium' },
  calendar: {
    // Posts per ISO week per channel and the local windows they may go out in.
    defaults: { postsPerWeek: 3, maxPerDay: 1, minGapHours: 20, windows: [{ days: [1, 2, 3, 4, 5], start: '17:00', end: '20:00' }] },
    channels: {
      x: { postsPerWeek: 5, windows: [{ days: [1, 2, 3, 4, 5], start: '09:00', end: '11:00' }, { days: [1, 2, 3, 4, 5], start: '17:00', end: '19:00' }] },
      bluesky: { postsPerWeek: 5, windows: [{ days: [1, 2, 3, 4, 5], start: '09:00', end: '12:00' }] },
      mastodon: { postsPerWeek: 4, windows: [{ days: [1, 2, 3, 4, 5], start: '10:00', end: '13:00' }] },
      tiktok: { postsPerWeek: 4, windows: [{ days: [2, 3, 4, 5, 6], start: '18:00', end: '21:00' }] },
      youtube: { postsPerWeek: 3, windows: [{ days: [4, 5, 6], start: '15:00', end: '18:00' }] },
      instagram: { postsPerWeek: 3, windows: [{ days: [1, 3, 5], start: '11:00', end: '13:00' }] },
      reddit: { postsPerWeek: 1, maxPerDay: 1, minGapHours: 96, windows: [{ days: [0, 6], start: '14:00', end: '17:00' }] },
      discord: { postsPerWeek: 2, windows: [{ days: [2, 5], start: '18:00', end: '20:00' }] },
      devlog: { postsPerWeek: 1, windows: [{ days: [4], start: '15:00', end: '17:00' }] },
      email: { postsPerWeek: 1, windows: [{ days: [2], start: '15:00', end: '16:00' }] },
    },
  },
  // Self-imposed daily caps, well under each platform's API limits.
  dailyCaps: { x: 5, bluesky: 6, mastodon: 6, tiktok: 2, youtube: 2, instagram: 2, reddit: 1, discord: 3, email: 1, devlog: 1 },
  bluesky: { service: 'https://bsky.social', handle: null },
  mastodon: { instance: null, visibility: 'public', maxChars: 500 },
  x: {},
  youtube: { privacyStatus: 'public', categoryId: '20' /* Gaming */ },
  reddit: { username: null, ownSubreddits: [], defaultTarget: null, userAgentApp: 'evo-marketing' },
  discord: {},
  email: { provider: 'manual', from: null, listFile: null },
  tiktok: { enabled: false, appApproved: false, mode: 'inbox' },
  instagram: { enabled: false, appApproved: false, graphVersion: 'v23.0', publicMediaBaseUrl: null },
};

// Secret environment variables, per platform. Documented in PUBLISHING.md.
export const SECRET_ENV = {
  claude: ['ANTHROPIC_API_KEY'],
  bluesky: ['BLUESKY_HANDLE', 'BLUESKY_APP_PASSWORD'],
  mastodon: ['MASTODON_INSTANCE', 'MASTODON_ACCESS_TOKEN'],
  x: ['X_USER_ACCESS_TOKEN'],
  youtube: ['YOUTUBE_CLIENT_ID', 'YOUTUBE_CLIENT_SECRET', 'YOUTUBE_REFRESH_TOKEN'],
  reddit: ['REDDIT_CLIENT_ID', 'REDDIT_CLIENT_SECRET', 'REDDIT_USERNAME', 'REDDIT_PASSWORD'],
  discord: ['DISCORD_WEBHOOK_URL'],
  email: ['BUTTONDOWN_API_KEY', 'SMTP_URL'],
  tiktok: ['TIKTOK_ACCESS_TOKEN'],
  instagram: ['INSTAGRAM_USER_ID', 'INSTAGRAM_ACCESS_TOKEN'],
};

function isObj(v) {
  return v && typeof v === 'object' && !Array.isArray(v);
}

export function deepMerge(a, b) {
  if (!isObj(a) || !isObj(b)) return b === undefined ? a : b;
  const out = { ...a };
  for (const [k, v] of Object.entries(b)) out[k] = isObj(v) && isObj(a[k]) ? deepMerge(a[k], v) : v;
  return out;
}

export function loadConfig({ cwd = process.cwd(), env = process.env, file } = {}) {
  const p = file || env.EVO_MARKETING_CONFIG || path.join(cwd, 'marketing.config.json');
  let fromFile = {};
  if (fs.existsSync(p)) {
    fromFile = JSON.parse(fs.readFileSync(p, 'utf8'));
    if (fromFile.secrets) {
      throw new Error(`${p}: put secrets in environment variables, not in the config file`);
    }
  }
  const cfg = deepMerge(DEFAULTS, fromFile);
  cfg.dataDir = path.resolve(cwd, cfg.dataDir);
  cfg.outDir = path.resolve(cwd, cfg.outDir);
  if (env.MARKETING_CLAUDE_MODEL) cfg.claude.model = env.MARKETING_CLAUDE_MODEL;
  if (env.MASTODON_INSTANCE) cfg.mastodon.instance = env.MASTODON_INSTANCE;
  if (env.BLUESKY_HANDLE) cfg.bluesky.handle = env.BLUESKY_HANDLE;
  if (env.REDDIT_USERNAME) cfg.reddit.username = env.REDDIT_USERNAME;
  cfg.env = env;
  return cfg;
}

/** Secrets for one platform, or a list of what is missing. */
export function secretsFor(platform, env = process.env) {
  const names = SECRET_ENV[platform] || [];
  const values = Object.fromEntries(names.map((n) => [n, env[n] || null]));
  return values;
}

export function requireSecrets(platform, env, names) {
  const missing = names.filter((n) => !env[n]);
  if (missing.length) {
    const e = new Error(`${platform}: missing environment variable(s) ${missing.join(', ')} (see PUBLISHING.md)`);
    e.code = 'MISSING_SECRET';
    throw e;
  }
  return Object.fromEntries(names.map((n) => [n, env[n]]));
}

// HTTP for adapters. Two modes:
//   live:    real fetch, with a per-platform minimum interval between requests
//            and honest handling of 429/rate-limit headers (wait as told,
//            or stop; never rotate identities or retry around a limit).
//   dry-run: nothing leaves the machine. Requests are recorded (secrets
//            redacted) and answered by the adapter's fake() so multi-step flows
//            (upload, then post) can be followed end to end.

const SECRET_HEADERS = /^(authorization|x-api-key|cookie)$/i;
const SECRET_FIELDS = /(password|secret|token|access_token|refresh_token|client_secret|api_key)$/i;

export function redactUrl(url) {
  try {
    const u = new URL(url);
    // Discord webhook token is the last path segment.
    if (/discord(app)?\.com$/.test(u.hostname) && u.pathname.startsWith('/api/webhooks/')) {
      u.pathname = u.pathname.replace(/(\/api\/webhooks\/\d+\/)[^/]+/, '$1***');
    }
    for (const k of [...u.searchParams.keys()]) if (SECRET_FIELDS.test(k)) u.searchParams.set(k, '***');
    return u.toString();
  } catch {
    return url;
  }
}

function redactBody(body) {
  if (body == null) return null;
  if (typeof body === 'string') {
    try {
      return redactBody(JSON.parse(body));
    } catch {
      if (body.includes('=') && !body.includes(' ')) {
        const p = new URLSearchParams(body);
        for (const k of [...p.keys()]) if (SECRET_FIELDS.test(k)) p.set(k, '***');
        return p.toString();
      }
      return body.length > 500 ? `${body.slice(0, 500)}… (${body.length} chars)` : body;
    }
  }
  if (body instanceof Uint8Array || Buffer.isBuffer(body)) return `<${body.length} bytes>`;
  if (typeof FormData !== 'undefined' && body instanceof FormData) {
    const o = {};
    for (const [k, v] of body.entries()) o[k] = typeof v === 'string' ? v : `<file ${v.name || ''} ${v.size} bytes>`;
    return { multipart: o };
  }
  if (typeof body === 'object') {
    const o = Array.isArray(body) ? [] : {};
    for (const [k, v] of Object.entries(body)) o[k] = SECRET_FIELDS.test(k) ? '***' : typeof v === 'object' ? redactBody(v) : v;
    return o;
  }
  return body;
}

export function describeRequest(url, init = {}) {
  const headers = {};
  for (const [k, v] of Object.entries(init.headers || {})) headers[k] = SECRET_HEADERS.test(k) ? '***' : v;
  return { method: init.method || 'GET', url: redactUrl(url), headers, body: redactBody(init.body) };
}

function fakeResponse({ status = 200, json, headers = {} }) {
  const h = new Headers(headers);
  if (json !== undefined && !h.has('content-type')) h.set('content-type', 'application/json');
  return new Response(json === undefined ? null : JSON.stringify(json), { status, headers: h });
}

export class RateLimitError extends Error {
  constructor(platform, waitS) {
    super(`${platform}: rate limited; the platform asks to wait ${Math.round(waitS)}s. Stopping; run publish again later.`);
    this.code = 'RATE_LIMITED';
    this.waitS = waitS;
  }
}

export class HttpError extends Error {
  constructor(platform, res, body) {
    super(`${platform}: HTTP ${res.status} ${typeof body === 'string' ? body.slice(0, 300) : JSON.stringify(body).slice(0, 300)}`);
    this.status = res.status;
    this.body = body;
  }
}

/** Seconds to wait from standard and platform-specific rate-limit headers. */
export function retryAfterSeconds(res, now = Date.now()) {
  const h = res.headers;
  const ra = h.get('retry-after');
  if (ra) return /^\d+(\.\d+)?$/.test(ra) ? Number(ra) : Math.max(0, (Date.parse(ra) - now) / 1000);
  const reset = h.get('x-rate-limit-reset') || h.get('x-ratelimit-reset') || h.get('ratelimit-reset');
  if (reset) {
    const n = Number(reset);
    if (Number.isFinite(n)) return n > 1e9 ? Math.max(0, n - now / 1000) : n; // epoch seconds or delta
    const t = Date.parse(reset); // Mastodon sends an ISO date
    if (!Number.isNaN(t)) return Math.max(0, (t - now) / 1000);
  }
  return 60;
}

/**
 * createHttp({ platform, live, fetch, fake, minIntervalMs, maxWaitS, sleep, log })
 * Returns { request(url, init) -> {status, headers, json, text}, log: [] }.
 */
export function createHttp({ platform, live = false, fetch = globalThis.fetch, fake, minIntervalMs = 1000, maxWaitS = 120, sleep, log } = {}) {
  const records = [];
  let last = 0;
  const wait = sleep || ((ms) => new Promise((r) => setTimeout(r, ms)));
  async function request(url, init = {}) {
    const desc = describeRequest(url, init);
    records.push(desc);
    log?.(`${live ? 'LIVE' : 'dry-run'} ${desc.method} ${desc.url}`);
    let res;
    if (!live) {
      res = fakeResponse(fake ? fake(url, init) : { json: {} });
    } else {
      const gap = last + minIntervalMs - Date.now();
      if (gap > 0) await wait(gap);
      for (let attempt = 0; ; attempt++) {
        last = Date.now();
        res = await fetch(url, init);
        if (res.status !== 429) break;
        const s = retryAfterSeconds(res);
        if (attempt >= 1 || s > maxWaitS) throw new RateLimitError(platform, s);
        log?.(`${platform}: 429, waiting ${Math.ceil(s)}s as the platform asks`);
        await wait(Math.ceil(s * 1000));
      }
    }
    const text = res.status === 204 ? '' : await res.text();
    let json = null;
    try {
      json = text ? JSON.parse(text) : null;
    } catch {}
    if (res.status >= 400) throw new HttpError(platform, res, json ?? text);
    return { status: res.status, headers: res.headers, json, text };
  }
  return { request, records, live };
}

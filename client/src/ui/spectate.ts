// Spectator mode: a read-only watch link (`?watch=<world>[&lineage=<n>]
// [&stream=1]`) shows a world with no login: the painted map, every lineage
// as soft discs over coarse cells (head counts, never animals), the followed
// lineage's portrait and numbers, the largest lineages, a population chart,
// the clock and any catastrophe sweeping the map. Made for streams and the
// marketing capture script (tools/marketing); a portrait window stacks the
// panel under the map, a landscape one puts it beside.
//
// Data: `GET /api/spectate` (server `game::spectate`: nothing private, at
// most once a simulated day) and the public terrain. The model is
// `spectate.ts`.

import './spectate.css';
import { api, setWorld } from '../api';
import { h } from './dom';
import type { Spectate, SpectateLineage } from '../protocol';
import { applyDynamic, decodeTerrain } from '../world/terrain';
import { buildFields, paintRegion } from '../world/terrainArt';
import { spriteModel } from '../world/spriteArt';
import { describeAvatar, type AvatarModel } from '../world/avatar';
import {
  catastropheLine, catastropheTint, clockLine, densityOf, discRadius, lerpDensity, lineageColor, lineageFacts, pickFollowed,
  PopulationHistory, topLineages, type Density, type WatchParams,
} from '../spectate';
import { drawPortraitCard, newCanvas, paintCreature, pngBlob } from '../share/card';
import { cardFileName, shareCard, watchUrl } from '../share/share';

/** How often the view asks for news (the server's copy changes once a simulated day). */
const POLL_MS = 1500;
/** How long the discs take to move to a new answer. */
const EASE_MS = 1200;
/** Terrain repaint (snow, water, drought) every this many polls. */
const TERRAIN_EVERY = 8;

declare global {
  interface Window {
    /** The spectator's state, for the capture script and tests. */
    evoSpectator?: {
      last: () => Spectate | null;
      followed: () => SpectateLineage | null;
      follow: (lineage: number) => void;
      /** The followed lineage's portrait card as a PNG data URL. */
      portraitCard: () => Promise<string | null>;
    };
  }
}

function lookModel(l: SpectateLineage): AvatarModel | null {
  return l.look ? spriteModel({ look: l.look, juvenile: false }) : null;
}

export async function mountSpectator(root: HTMLElement, params: WatchParams): Promise<() => void> {
  setWorld(params.world);
  document.documentElement.classList.add('spectate-page');
  const map = h('canvas', { class: 'sp-map', id: 'sp-map', role: 'img', 'aria-label': 'The world map with every lineage as coloured discs' });
  const clock = h('div', { class: 'sp-clock', id: 'sp-clock' }, 'Loading the world…');
  const banner = h('div', { class: 'sp-banner', id: 'sp-banner', hidden: true });
  const portrait = h('canvas', { class: 'sp-portrait', id: 'sp-portrait', width: 360, height: 260 });
  const fName = h('h2', { class: 'sp-name', id: 'sp-name' });
  const fOwner = h('p', { class: 'sp-owner muted' });
  const fFacts = h('p', { class: 'sp-facts', id: 'sp-facts' });
  const list = h('ol', { class: 'sp-list', id: 'sp-list', 'aria-label': 'Largest lineages' });
  const chart = h('canvas', { class: 'sp-chart', id: 'sp-chart', width: 600, height: 220, role: 'img', 'aria-label': 'Population over time' });
  const shareBtn = h('button', { class: 'secondary', id: 'sp-share' }, 'Share card');
  const linkBtn = h('button', { class: 'secondary', id: 'sp-link' }, 'Copy watch link');
  const status = h('p', { class: 'sp-status muted small', id: 'sp-status', 'aria-live': 'polite' });
  const panel = h('aside', { class: 'sp-panel' },
    h('section', { class: 'sp-follow', 'aria-label': 'Followed lineage' }, portrait, fName, fOwner, fFacts),
    h('section', { class: 'sp-section' }, h('h3', {}, 'Population'), chart),
    h('section', { class: 'sp-section' }, h('h3', {}, 'Largest lineages'), list),
    params.stream ? null : h('div', { class: 'sp-actions' }, shareBtn, linkBtn, status),
    h('p', { class: 'sp-brand muted small' }, 'Evolution MMO · watching live, no login'));
  root.replaceChildren(h('div', { class: `screen spectate${params.stream ? ' sp-stream' : ''}`, id: 'spectate' },
    h('div', { class: 'sp-stage' }, map, clock, banner), panel));

  let last: Spectate | null = null;
  let followedId = params.lineage;
  let followed: SpectateLineage | null = null;
  let from: Density = new Map();
  let to: Density = new Map();
  let changedAt = 0;
  const history = new PopulationHistory();
  let terrainImg: HTMLCanvasElement | null = null;
  let polls = 0;
  let sweepStart = 0;
  let sweepKind = '';
  let stopped = false;

  async function paintTerrain(): Promise<void> {
    const t = await api.terrain();
    let grid = decodeTerrain(t);
    try {
      grid = applyDynamic(grid, await api.terrainDynamic());
    } catch {
      /* the static layers alone */
    }
    const tpx = Math.max(2, Math.min(6, Math.floor(768 / Math.max(grid.width, grid.height))));
    const f = buildFields(grid);
    const W = grid.width * tpx, H = grid.height * tpx;
    const buf = new Uint8Array(W * H * 4);
    paintRegion(f, 0, 0, grid.width, grid.height, tpx, buf);
    const c = newCanvas(W, H);
    c.getContext('2d')!.putImageData(new ImageData(new Uint8ClampedArray(buf.buffer), W, H), 0, 0);
    terrainImg = c;
  }

  function showFollowed(): void {
    if (!last) return;
    followed = pickFollowed(last, followedId);
    if (!followed) return;
    fName.textContent = followed.name;
    fOwner.textContent = followed.wild ? 'Wild species' : `Lineage of ${followed.owner}`;
    fFacts.textContent = followed.alive > 0 ? lineageFacts(followed) : 'Extinct';
    const ctx = portrait.getContext('2d')!;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, portrait.width, portrait.height);
    const m = lookModel(followed);
    if (m) {
      paintCreature(ctx, m, 20, 10, portrait.width - 40, portrait.height - 20, () => document.createElement('canvas'));
      portrait.setAttribute('aria-label', describeAvatar(m));
    }
  }

  function showList(): void {
    if (!last) return;
    const rows = topLineages(last, followed?.lineage ?? null, params.stream ? 4 : 6);
    list.replaceChildren(...rows.map((l) => {
      const c = lineageColor(l, followed?.lineage ?? null);
      return h('li', { class: l.lineage === followed?.lineage ? 'sp-on' : '', 'data-lineage': String(l.lineage) },
        h('span', { class: 'sp-swatch', style: `background:${c.fill}` }),
        h('button', { class: 'sp-pick', title: 'Follow', onclick: () => follow(l.lineage) }, l.name),
        h('span', { class: 'sp-count muted' }, `${l.alive}`));
    }));
  }

  function drawChart(): void {
    if (!last) return;
    const ctx = chart.getContext('2d')!;
    const W = chart.width, H = chart.height;
    ctx.clearRect(0, 0, W, H);
    const rows = topLineages(last, followed?.lineage ?? null, 4);
    const ids = rows.map((l) => l.lineage);
    const peak = history.peak(ids);
    const n = history.days.length;
    if (n < 2) return;
    for (const l of rows.slice().reverse()) {
      const v = history.series.get(l.lineage) ?? [];
      ctx.strokeStyle = lineageColor(l, followed?.lineage ?? null).fill;
      ctx.lineWidth = l.lineage === followed?.lineage ? 5 : 3;
      ctx.beginPath();
      v.forEach((y, i) => {
        const px = (i / (n - 1)) * (W - 8) + 4;
        const py = H - 6 - (y / peak) * (H - 16);
        if (i === 0) ctx.moveTo(px, py);
        else ctx.lineTo(px, py);
      });
      ctx.stroke();
    }
  }

  function apply(s: Spectate): void {
    const now = performance.now();
    const shown = lerpDensity(from, to, (now - changedAt) / EASE_MS);
    from = shown;
    to = densityOf(s);
    changedAt = now;
    last = s;
    history.push(s);
    clock.textContent = clockLine(s.clock);
    const cat = s.catastrophes[0];
    if (cat) {
      banner.hidden = false;
      banner.textContent = catastropheLine(cat);
      banner.dataset.kind = cat.kind;
      if (sweepKind !== `${cat.id}`) {
        sweepKind = `${cat.id}`;
        sweepStart = now;
      }
    } else {
      banner.hidden = true;
      sweepKind = '';
    }
    showFollowed();
    showList();
    drawChart();
  }

  function follow(lineage: number): void {
    followedId = lineage;
    const u = new URL(window.location.href);
    u.searchParams.set('lineage', String(lineage));
    window.history.replaceState(null, '', u.toString());
    showFollowed();
    showList();
    drawChart();
  }

  function frame(): void {
    if (stopped) return;
    requestAnimationFrame(frame);
    const dpr = window.devicePixelRatio || 1;
    const rect = map.getBoundingClientRect();
    const cw = Math.max(1, Math.round(rect.width * dpr)), ch = Math.max(1, Math.round(rect.height * dpr));
    if (map.width !== cw || map.height !== ch) {
      map.width = cw;
      map.height = ch;
    }
    const ctx = map.getContext('2d')!;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = '#0d1117';
    ctx.fillRect(0, 0, cw, ch);
    if (!last || !terrainImg) return;
    // The map fitted into the canvas (letterboxed), in tiles.
    const k = Math.min(cw / last.width, ch / last.height);
    const ox = (cw - k * last.width) / 2, oy = (ch - k * last.height) / 2;
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(terrainImg, ox, oy, k * last.width, k * last.height);
    // A catastrophe: a tint, swept in from the left over its first seconds.
    const cat = last.catastrophes[0];
    if (cat) {
      const [r, g, b, a] = catastropheTint(cat.kind);
      const t = Math.min(1, (performance.now() - sweepStart) / 4000);
      const edge = ox + k * last.width * (t * 1.3);
      const grad = ctx.createLinearGradient(edge - k * last.width * 0.3, 0, edge, 0);
      grad.addColorStop(0, `rgba(${r},${g},${b},${a})`);
      grad.addColorStop(1, `rgba(${r},${g},${b},0)`);
      ctx.fillStyle = grad;
      ctx.fillRect(ox, oy, Math.min(k * last.width, edge - ox), k * last.height);
    }
    const shown = lerpDensity(from, to, (performance.now() - changedAt) / EASE_MS);
    const byId = new Map(last.lineages.map((l) => [l.lineage, l]));
    const cell = last.cell;
    const fid = followed?.lineage ?? null;
    // Others first, the followed lineage on top.
    const keys = [...shown.keys()].sort((a, b) => Number(a.split(':')[0] === String(fid)) - Number(b.split(':')[0] === String(fid)));
    for (const key of keys) {
      const n = shown.get(key)!;
      if (n < 0.05) continue;
      const [li, cx, cy] = key.split(':').map(Number);
      const l = byId.get(li);
      if (!l) continue;
      const col = lineageColor(l, fid);
      const r = discRadius(n, cell) * k;
      const x = ox + (cx + 0.5) * cell * k, y = oy + (cy + 0.5) * cell * k;
      const g = ctx.createRadialGradient(x, y, 0, x, y, r);
      const alpha = li === fid ? 0.9 : l.wild ? 0.5 : 0.7;
      g.addColorStop(0, col.fill.replace(')', ` / ${alpha})`));
      g.addColorStop(1, col.fill.replace(')', ' / 0)'));
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  async function poll(): Promise<void> {
    if (stopped) return;
    try {
      if (!terrainImg || polls % TERRAIN_EVERY === 0) await paintTerrain();
      apply(await api.spectate());
      status.textContent = '';
    } catch (e) {
      status.textContent = `Can't reach the world (${e instanceof Error ? e.message : e}); retrying…`;
      if (!last) clock.textContent = 'Waiting for the world…';
    }
    polls++;
  }

  async function portraitCard(): Promise<HTMLCanvasElement | null> {
    if (!followed) return null;
    const m = lookModel(followed);
    if (!m) return null;
    const c = newCanvas(1, 1);
    drawPortraitCard(c, m, {
      title: followed.name,
      subtitle: followed.wild ? 'Wild species' : `Lineage of ${followed.owner}`,
      facts: [lineageFacts(followed), last ? clockLine(last.clock) : ''].filter(Boolean),
      footer: 'Evolution MMO · watch it live',
    });
    return c;
  }

  shareBtn.addEventListener('click', async () => {
    const c = await portraitCard();
    if (!c || !followed) return;
    const out = await shareCard({
      blob: await pngBlob(c),
      fileName: cardFileName(followed.name, 'portrait'),
      title: followed.name,
      text: `${followed.name} in Evolution MMO`,
      url: watchUrl(window.location.href, params.world, followed.lineage),
    });
    status.textContent = out.cancelled ? '' : out.method === 'download' ? (out.copied ? 'Card saved; watch link copied.' : 'Card saved.') : 'Shared.';
  });
  linkBtn.addEventListener('click', async () => {
    const url = watchUrl(window.location.href, params.world, followed?.lineage ?? null);
    const ok = await navigator.clipboard?.writeText(url).then(() => true, () => false);
    status.textContent = ok ? 'Watch link copied.' : url;
  });

  window.evoSpectator = {
    last: () => last,
    followed: () => followed,
    follow,
    portraitCard: async () => (await portraitCard())?.toDataURL('image/png') ?? null,
  };
  await poll();
  const timer = setInterval(() => void poll(), POLL_MS);
  requestAnimationFrame(frame);
  return () => {
    stopped = true;
    clearInterval(timer);
    delete window.evoSpectator;
  };
}

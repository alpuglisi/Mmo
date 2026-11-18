// What capture.mjs writes for the publishing tool (PUBLISHING.md, "Inputs"):
// `manifest.json` (every file as a media item) and `stories.json` (one
// story per scene, its title, summary and facts from what the run measured,
// so copy only cites numbers that are true). Pure functions, tested with the
// publisher's own loaders (test/capture.test.mjs).

/** A scene's frame size per format, in pixels. */
export function frameSize(format) {
  const vp = format.viewport;
  return { width: Math.round(vp.width * format.scale), height: Math.round(vp.height * format.scale) };
}

const CARD_SIZES = { portrait: [1080, 1350], 'family-tree': [1080, 1920] };

/**
 * Media items of one scene: `recordings` is `[{ format, formatName, files:
 * { mp4?, gif?, frames? } }]`, `cards` is `{ kind: file }`, `gifHeight` per
 * format name. Paths are relative to the manifest (the output directory).
 */
export function sceneItems(scene, recordings, cards, seconds) {
  const items = [];
  for (const r of recordings) {
    const { width, height } = frameSize(r.format);
    const shape = height > width ? 'vertical 9:16' : 'square';
    if (r.files.mp4) {
      items.push({
        id: `${scene.id}-${r.formatName}`, path: r.files.mp4, type: 'video', duration_s: seconds,
        width, height, story_id: scene.id, alt: `${scene.alt} (${shape} video)`,
      });
    }
    if (r.files.gif) {
      const gw = r.format.gifWidth;
      items.push({
        id: `${scene.id}-${r.formatName}-gif`, path: r.files.gif, type: 'image', duration_s: seconds,
        width: gw, height: Math.round((gw * height) / width), story_id: scene.id, alt: `${scene.alt} (${shape} GIF)`,
      });
    }
  }
  for (const [kind, file] of Object.entries(cards)) {
    const [width, height] = CARD_SIZES[kind] ?? [null, null];
    items.push({
      id: `${scene.id}-${kind}`, path: file, type: 'card', width, height, story_id: scene.id,
      alt: kind === 'family-tree' ? `Family-tree card of ${scene.subject}: its stages from founding to today` : `Portrait card of ${scene.subject}`,
    });
  }
  return items;
}

/** A scene's story: its measured `title`, `summary` and `facts`, and its media ids. */
export function sceneStory(scene, measured, items) {
  return {
    id: scene.id,
    kind: scene.kind,
    title: measured.title,
    summary: measured.summary,
    facts: measured.facts,
    media: items.filter((i) => i.story_id === scene.id).map((i) => i.id),
    players: measured.players ?? [],
    // For people: the capture script's own tag ideas (the publisher picks its own by kind).
    hashtags: scene.hashtags,
  };
}

/** The two files' contents. */
export function outputs({ generatedAt, fps, ffmpeg, stories, items }) {
  const head = { generated_at: generatedAt, source: 'tools/marketing/capture.mjs', fps, ffmpeg };
  return { manifest: { ...head, items }, stories: { ...head, stories } };
}

/** Peak, the low after it, and the last value of a series, with their indices. */
export function peakAndLow(values) {
  let peak = -Infinity, peakAt = 0;
  values.forEach((v, i) => {
    if (v > peak) {
      peak = v;
      peakAt = i;
    }
  });
  let low = peak, lowAt = peakAt;
  for (let i = peakAt; i < values.length; i++) {
    if (values[i] < low) {
      low = values[i];
      lowAt = i;
    }
  }
  return { peak, peakAt, low, lowAt, last: values[values.length - 1] ?? 0 };
}

/** "year 3" for a 0-based day and a year length. */
export const yearOf = (day, daysPerYear) => Math.floor(day / daysPerYear) + 1;

/**
 * The boom-and-bust story of a predator's head counts over `days`: a bust
 * when it fell to at most 60% of its peak after it, else a plain population
 * story. Every number in it is in `facts`.
 */
export function boomBust(name, days, counts, daysPerYear) {
  const p = peakAndLow(counts);
  const peakYear = yearOf(days[p.peakAt], daysPerYear);
  const lowYear = yearOf(days[p.lowAt], daysPerYear);
  const years = yearOf(days[days.length - 1], daysPerYear);
  const facts = { lineage: name, start: counts[0], peak: p.peak, peak_year: peakYear, low_after_peak: p.low, low_year: lowYear, end: p.last, years };
  if (p.peak > 0 && p.low <= 0.6 * p.peak && p.lowAt > p.peakAt) {
    return {
      title: `Boom and bust: the ${name}`,
      summary: `The ${name} grew from ${counts[0]} to a peak of ${p.peak} in year ${peakYear}, then fell to ${p.low} by year ${lowYear}.`,
      facts,
    };
  }
  return {
    title: `The ${name} over ${years} game years`,
    summary: `The ${name} went from ${counts[0]} to ${p.last}, peaking at ${p.peak} in year ${peakYear}.`,
    facts,
  };
}

/** Story text for the staged severe-winter scene, framed by what happened. */
export function catastropheStory(start, end, days) {
  const fell = end.animals < start.animals;
  const lost = start.lineages - end.lineages;
  const title = fell ? 'A severe winter sweeps the map' : 'A severe winter, and the world keeps growing';
  const outcome = fell
    ? `the world went from ${start.animals} animals to ${end.animals}${lost > 0 ? `, and ${lost} of ${start.lineages} lineages died out` : ''}`
    : `the world still grew from ${start.animals} animals to ${end.animals}`;
  return {
    title,
    summary: `In a test world, a severe winter (triggered for this capture) began in year ${start.year}; over the next ${days} game days ${outcome}.`,
    facts: { catastrophe: 'severe winter', staged: 'triggered for the capture', year: start.year, start_day: start.day, days, animals_before: start.animals, animals_after: end.animals, lineages_before: start.lineages, lineages_after: end.lineages },
  };
}

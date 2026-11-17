// Content calendar: assigns approved drafts to the next free slot per channel,
// respecting posts-per-week, posts-per-day, a minimum gap, and best-time
// windows in the owner's time zone. All arithmetic is in UTC; local times are
// converted with Intl (no dependencies).

const DAY_MS = 86400000;

function partsInZone(date, timeZone) {
  const f = new Intl.DateTimeFormat('en-US', {
    timeZone,
    hourCycle: 'h23',
    year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit', weekday: 'short',
  });
  const o = Object.fromEntries(f.formatToParts(date).map((p) => [p.type, p.value]));
  const wd = { Sun: 0, Mon: 1, Tue: 2, Wed: 3, Thu: 4, Fri: 5, Sat: 6 }[o.weekday];
  return { y: +o.year, m: +o.month, d: +o.day, h: +o.hour, min: +o.minute, s: +o.second, wd };
}

function offsetMs(date, timeZone) {
  const p = partsInZone(date, timeZone);
  return Date.UTC(p.y, p.m - 1, p.d, p.h, p.min, p.s) - Math.floor(date.getTime() / 1000) * 1000;
}

/** UTC Date for a wall-clock time in `timeZone`. */
export function zonedToUtc(y, m, d, h, min, timeZone) {
  const guess = Date.UTC(y, m - 1, d, h, min);
  let t = guess - offsetMs(new Date(guess), timeZone);
  t = guess - offsetMs(new Date(t), timeZone); // second pass handles DST edges
  return new Date(t);
}

export function localDayKey(date, timeZone) {
  const p = partsInZone(date, timeZone);
  return `${p.y}-${String(p.m).padStart(2, '0')}-${String(p.d).padStart(2, '0')}`;
}

/** ISO week key (Monday start) of the local date. */
export function localWeekKey(date, timeZone) {
  const p = partsInZone(date, timeZone);
  const day = new Date(Date.UTC(p.y, p.m - 1, p.d));
  const dow = (day.getUTCDay() + 6) % 7;
  day.setUTCDate(day.getUTCDate() - dow + 3);
  const firstThu = new Date(Date.UTC(day.getUTCFullYear(), 0, 4));
  const week = 1 + Math.round(((day - firstThu) / DAY_MS - 3 + ((firstThu.getUTCDay() + 6) % 7)) / 7);
  return `${day.getUTCFullYear()}-W${String(week).padStart(2, '0')}`;
}

export function channelRules(config, platform) {
  const c = config.calendar;
  return { ...c.defaults, ...(c.channels[platform] || {}) };
}

function hm(s) {
  const [h, m] = s.split(':').map(Number);
  return { h, m };
}

/** Candidate slot starts (UTC) from `from`, in window order, for `days` days. */
export function* candidateSlots(rules, timeZone, from, days = 60) {
  const p0 = partsInZone(from, timeZone);
  for (let i = 0; i < days; i++) {
    const base = new Date(Date.UTC(p0.y, p0.m - 1, p0.d + i));
    const y = base.getUTCFullYear(), m = base.getUTCMonth() + 1, d = base.getUTCDate(), wd = base.getUTCDay();
    const wins = rules.windows.filter((w) => w.days.includes(wd)).sort((a, b) => a.start.localeCompare(b.start));
    for (const w of wins) {
      const s = hm(w.start), e = hm(w.end);
      const start = zonedToUtc(y, m, d, s.h, s.m, timeZone);
      const end = zonedToUtc(y, m, d, e.h, e.m, timeZone);
      // Slot at the window start, or "now" if we are inside the window.
      const t = from > start && from < end ? new Date(Math.ceil(from.getTime() / 60000) * 60000) : start;
      if (t >= from && t < end) yield t;
    }
  }
}

/** Is `t` allowed given already-booked times for this channel? */
export function fits(t, booked, rules, timeZone) {
  const week = localWeekKey(t, timeZone);
  const day = localDayKey(t, timeZone);
  if (booked.filter((b) => localWeekKey(b, timeZone) === week).length >= rules.postsPerWeek) return false;
  if (booked.filter((b) => localDayKey(b, timeZone) === day).length >= rules.maxPerDay) return false;
  if (booked.some((b) => Math.abs(b - t) < rules.minGapHours * 3600000)) return false;
  return true;
}

/**
 * Schedule every approved item. Items keep their order of approval within a
 * channel. Returns the items it scheduled.
 */
export function scheduleApproved(queue, config, now = new Date()) {
  const tz = config.timezone || 'UTC';
  const done = [];
  const byPlatform = {};
  for (const it of queue.items) {
    if (['scheduled', 'posted', 'posting'].includes(it.status) && (it.scheduledAt || it.result?.postedAt)) {
      (byPlatform[it.platform] ||= []).push(new Date(it.result?.postedAt || it.scheduledAt));
    }
  }
  const approved = queue.items
    .filter((i) => i.status === 'approved')
    .sort((a, b) => (a.approval?.at || '').localeCompare(b.approval?.at || ''));
  for (const it of approved) {
    const rules = channelRules(config, it.platform);
    const booked = (byPlatform[it.platform] ||= []);
    for (const t of candidateSlots(rules, tz, now)) {
      if (fits(t, booked, rules, tz)) {
        booked.push(t);
        queue.mark(it.id, 'scheduled', { scheduledAt: t.toISOString() }, { actor: 'calendar', now });
        done.push(it);
        break;
      }
    }
  }
  return done;
}

/** Plain-text calendar for the next `days` days. */
export function renderCalendar(queue, config, now = new Date(), days = 14) {
  const tz = config.timezone || 'UTC';
  const end = new Date(now.getTime() + days * DAY_MS);
  const rows = queue.items
    .filter((i) => i.scheduledAt && ['scheduled', 'posted', 'posting', 'failed'].includes(i.status))
    .map((i) => ({ t: new Date(i.scheduledAt), i }))
    .filter(({ t }) => t >= new Date(now.getTime() - DAY_MS) && t <= end)
    .sort((a, b) => a.t - b.t);
  const fmt = new Intl.DateTimeFormat('en-GB', { timeZone: tz, weekday: 'short', day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' });
  if (!rows.length) return `Nothing scheduled in the next ${days} days (${tz}).`;
  return [`Calendar (${tz})`, ...rows.map(({ t, i }) => `${fmt.format(t).padEnd(22)} ${i.platform.padEnd(9)} ${i.status.padEnd(9)} ${i.id}  ${summaryOf(i)}`)].join('\n');
}

export function summaryOf(item) {
  const f = item.fields;
  const s = f.title || f.subject || f.text || f.caption || f.content || '';
  return s.replace(/\s+/g, ' ').slice(0, 60);
}

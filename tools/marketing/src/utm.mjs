// UTM link scheme:
//   utm_source   = platform key (x, bluesky, reddit, ...)
//   utm_medium   = social | video | community | email | devlog | press | outreach
//   utm_campaign = config.site.campaign (e.g. "launch")
//   utm_content  = story id (slug), so clicks can be traced to the story
const MEDIUM = {
  x: 'social', bluesky: 'social', mastodon: 'social',
  tiktok: 'video', youtube: 'video', instagram: 'video',
  reddit: 'community', discord: 'community',
  email: 'email', devlog: 'devlog', press: 'press', outreach: 'outreach',
};

export function utmLink(base, { source, campaign, content, medium } = {}) {
  const u = new URL(base);
  u.searchParams.set('utm_source', source);
  u.searchParams.set('utm_medium', medium || MEDIUM[source] || 'social');
  if (campaign) u.searchParams.set('utm_campaign', campaign);
  if (content) u.searchParams.set('utm_content', String(content).toLowerCase().replace(/[^a-z0-9_-]+/g, '-').slice(0, 60));
  return u.toString();
}

export function parseUtm(url) {
  const u = new URL(url);
  return Object.fromEntries([...u.searchParams].filter(([k]) => k.startsWith('utm_')));
}

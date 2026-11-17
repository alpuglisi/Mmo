// Per-platform format limits used by drafting and validation.
// Sources: each platform's developer documentation (see PUBLISHING.md).
// `measure` names a counter in text.mjs.

export const PLATFORMS = {
  x: {
    label: 'X',
    kind: 'short',
    fields: { text: { max: 280, measure: 'x' } },
    maxHashtags: 2,
    media: { video: true, image: true, maxVideoSeconds: 140 },
  },
  bluesky: {
    label: 'Bluesky',
    kind: 'short',
    fields: { text: { max: 300, measure: 'graphemes' } },
    maxHashtags: 2,
    media: { video: true, image: true, maxVideoSeconds: 180, maxImages: 4, altRequired: true },
  },
  mastodon: {
    label: 'Mastodon',
    kind: 'short',
    // 500 is the stock instance limit; override with config.mastodon.maxChars.
    fields: { text: { max: 500, measure: 'mastodon' } },
    maxHashtags: 4,
    media: { video: true, image: true, maxImages: 4, altRequired: true },
  },
  tiktok: {
    label: 'TikTok',
    kind: 'video',
    fields: { caption: { max: 2200, measure: 'codePoints' } },
    maxHashtags: 5,
    media: { video: true, required: true, vertical: true },
  },
  youtube: {
    label: 'YouTube Shorts',
    kind: 'video',
    fields: {
      title: { max: 100, measure: 'codePoints', noAngleBrackets: true },
      description: { max: 5000, measure: 'utf8Bytes', noAngleBrackets: true },
    },
    maxHashtags: 3,
    media: { video: true, required: true, vertical: true, maxVideoSeconds: 180 },
  },
  instagram: {
    label: 'Instagram Reels',
    kind: 'video',
    fields: { caption: { max: 2200, measure: 'codePoints' } },
    maxHashtags: 5, // platform hard limit is 30; we stay well under
    media: { video: true, required: true, vertical: true, maxVideoSeconds: 90 },
  },
  reddit: {
    label: 'Reddit',
    kind: 'long',
    fields: {
      title: { max: 300, measure: 'codePoints' },
      body: { max: 40000, measure: 'codePoints' },
    },
    maxHashtags: 0,
    media: { video: false, image: false },
  },
  discord: {
    label: 'Discord',
    kind: 'announce',
    fields: { content: { max: 2000, measure: 'codePoints' } },
    maxHashtags: 0,
    media: { video: true, image: true, maxFileBytes: 10 * 1024 * 1024 },
  },
  devlog: {
    label: 'Devlog (Markdown)',
    kind: 'long',
    fields: {
      title: { max: 120, measure: 'codePoints' },
      body: { max: 60000, measure: 'codePoints' },
    },
    maxHashtags: 0,
    media: { video: true, image: true },
  },
  email: {
    label: 'Newsletter',
    kind: 'long',
    fields: {
      subject: { max: 78, measure: 'codePoints' },
      body: { max: 60000, measure: 'codePoints' },
    },
    maxHashtags: 0,
    media: { image: true },
  },
};

export const ALL_PLATFORMS = Object.keys(PLATFORMS);

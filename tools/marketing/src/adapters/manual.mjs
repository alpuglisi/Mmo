import fs from 'node:fs';
import path from 'node:path';
import { PLATFORMS } from '../platforms.mjs';

// Manual export: a folder per draft with the media and copy-paste-ready text,
// for platforms without API access (TikTok/Instagram before app approval,
// the devlog, or anything the owner prefers to post by hand).

export function exportDraft(item, outDir) {
  const day = (item.scheduledAt || item.createdAt || new Date().toISOString()).slice(0, 10);
  const dir = path.join(outDir, 'manual', item.platform, `${day}-${item.id}`);
  fs.mkdirSync(dir, { recursive: true });
  const files = [];
  for (const [field, value] of Object.entries(item.fields)) {
    const name = item.platform === 'devlog' && field === 'body' ? 'post.md' : `${field}.txt`;
    fs.writeFileSync(path.join(dir, name), value + '\n');
    files.push(name);
  }
  for (const m of item.media || []) {
    if (fs.existsSync(m.path)) {
      fs.copyFileSync(m.path, path.join(dir, path.basename(m.path)));
      files.push(path.basename(m.path));
    }
  }
  const lines = [
    `# ${PLATFORMS[item.platform].label}: post by hand`,
    '',
    `Queue item ${item.id}, story ${item.storyId}${item.scheduledAt ? `, scheduled ${item.scheduledAt}` : ''}.`,
    '',
    ...Object.keys(item.fields).map((f) => `- ${f}: copy from \`${f === 'body' && item.platform === 'devlog' ? 'post.md' : f + '.txt'}\``),
    ...(item.media || []).map((m) => `- media: \`${path.basename(m.path)}\`${m.alt ? ` (alt text: ${m.alt})` : ''}`),
    '',
    'After posting, record it so the calendar and duplicate check know:',
    '',
    `    node bin/evo-marketing.mjs posted ${item.id} --url <post url>`,
    '',
  ];
  fs.writeFileSync(path.join(dir, 'README.md'), lines.join('\n'));
  return { dir, files };
}

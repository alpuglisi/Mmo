#!/usr/bin/env node
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { loadConfig, SECRET_ENV } from '../src/config.mjs';
import { loadStories, loadManifest, factCorpus } from '../src/stories.mjs';
import { ALL_PLATFORMS, PLATFORMS } from '../src/platforms.mjs';
import { draftStory } from '../src/drafting/index.mjs';
import { createClient } from '../src/drafting/claude.mjs';
import { Queue } from '../src/queue.mjs';
import { scheduleApproved, renderCalendar, summaryOf } from '../src/calendar.mjs';
import { publishDue } from '../src/publish.mjs';
import { exportDraft } from '../src/adapters/manual.mjs';
import { ADAPTERS, manualReason } from '../src/adapters/index.mjs';
import { createReviewServer } from '../src/review-server.mjs';
import { suggestFor, writePressKit, writeOutreach } from '../src/promote.mjs';
import { collectMetrics, summarize, renderSummary } from '../src/analytics.mjs';
import { utmLink } from '../src/utm.mjs';

const HELP = `evo-marketing: drafts → human review → calendar → official-API publishing

Drafting
  draft --stories FILE [--manifest FILE] [--platforms x,bluesky,...] [--story ID] [--no-claude] [--force]

Review (nothing posts without approval)
  queue [--status S] [--platform P]      list drafts
  show ID                                full draft with checks
  edit ID --field F --value TEXT         edit one field (or: edit ID --editor)
  approve ID [--target SUB] [--confirm-third-party]
  reject ID [--reason TEXT]
  review [--port 8787]                   local web page for the same

Calendar and publishing (dry-run unless --live)
  schedule                               put approved drafts into calendar slots
  calendar [--days 14]
  publish [--live] [--id ID,...]         post due items (dry-run shows the requests)
  export [--id ID,...]                   manual-upload folders for approved/scheduled drafts
  posted ID --url URL                    record a post you made by hand

Promotion and analytics
  promote --stories FILE [--story ID]    communities and hashtags to consider
  press-kit --stories FILE [--manifest FILE]
  outreach --creators FILE --stories FILE  personalised email drafts (never sent)
  links [--story ID]                     UTM-tagged links per channel
  metrics [--live]                       pull post metrics through the official APIs
  report                                 analytics summary (Markdown)
  check                                  show which channels are configured

Hard boundaries are listed in PUBLISHING.md.`;

function parse(argv) {
  const args = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith('--')) {
      const [k, v] = a.slice(2).split('=');
      if (v !== undefined) args[k] = v;
      else if (argv[i + 1] && !argv[i + 1].startsWith('--')) args[k] = argv[++i];
      else args[k] = true;
    } else args._.push(a);
  }
  return args;
}

const args = parse(process.argv.slice(2));
const cmd = args._[0];
if (!cmd || args.help || cmd === 'help') {
  console.log(HELP);
  process.exit(0);
}

const config = loadConfig({ file: args.config });
const queue = Queue.open(config.dataDir);
const save = () => queue.save();
const list = (s) => (s ? String(s).split(',').map((x) => x.trim()).filter(Boolean) : null);

function printItem(i) {
  console.log(`${i.id}  ${i.platform}  ${i.status}  story=${i.storyId}  source=${i.source}${i.model ? ` (${i.model})` : ''}${i.target ? `  target=${i.target}` : ''}${i.scheduledAt ? `  at=${i.scheduledAt}` : ''}`);
  for (const [f, v] of Object.entries(i.fields)) console.log(`\n--- ${f} ---\n${v}`);
  if (i.media?.length) console.log(`\n--- media ---\n${i.media.map((m) => `${m.type} ${m.path}${m.alt ? ` [alt: ${m.alt}]` : ''}`).join('\n')}`);
  const v = i.validation || { errors: [], warnings: [] };
  console.log(`\nchecks: ${v.ok ? 'OK' : 'FAIL'}`);
  for (const e of v.errors) console.log(`  error: ${e}`);
  for (const w of v.warnings) console.log(`  warning: ${w}`);
}

async function main() {
  switch (cmd) {
    case 'draft': {
      if (!args.stories) throw new Error('draft needs --stories FILE');
      let stories = loadStories(args.stories);
      if (args.story) stories = stories.filter((s) => list(args.story).includes(s.id));
      const manifest = loadManifest(args.manifest);
      const platforms = list(args.platforms) || ALL_PLATFORMS;
      for (const p of platforms) if (!PLATFORMS[p]) throw new Error(`unknown platform ${p}`);
      const client = args['no-claude'] ? null : await createClient(config.env);
      console.log(client ? `Drafting with ${config.claude.model}` : 'Drafting with templates (no ANTHROPIC_API_KEY, or --no-claude)');
      for (const story of stories) {
        const { drafts, skipped, notes } = await draftStory(story, manifest, { platforms, config, client });
        for (const n of notes) console.log(`  note: ${n}`);
        if (skipped.length) console.log(`  ${story.id}: skipped ${skipped.join(', ')} (no video)`);
        for (const d of drafts) {
          const r = queue.add(d, { force: !!args.force });
          console.log(r.added ? `  + ${r.item.id} ${d.platform.padEnd(9)} ${d.source.padEnd(8)} ${d.validation.ok ? 'ok' : 'FAILS CHECKS'}  ${story.id}` : `  = ${d.platform} ${story.id}: duplicate of ${r.duplicateOf}, not added`);
        }
      }
      save();
      console.log('\nReview with: queue, show ID, approve ID / reject ID, or the local page: review');
      break;
    }
    case 'queue': {
      const items = queue.list({ status: args.status, platform: args.platform });
      for (const i of items) console.log(`${i.id}  ${i.platform.padEnd(9)} ${i.status.padEnd(9)} ${i.validation?.ok ? 'ok  ' : 'FAIL'}  ${i.storyId.padEnd(16)} ${summaryOf(i)}`);
      if (!items.length) console.log('Queue is empty.');
      break;
    }
    case 'show':
      printItem(queue.get(args._[1]));
      break;
    case 'edit': {
      const item = queue.get(args._[1]);
      let fields;
      if (args.editor) {
        const tmp = path.join(os.tmpdir(), `evo-draft-${item.id}.json`);
        fs.writeFileSync(tmp, JSON.stringify(item.fields, null, 2));
        spawnSync(process.env.EDITOR || 'vi', [tmp], { stdio: 'inherit' });
        fields = JSON.parse(fs.readFileSync(tmp, 'utf8'));
        fs.unlinkSync(tmp);
      } else {
        if (!args.field || typeof args.value !== 'string') throw new Error('edit ID --field F --value TEXT (or --editor)');
        fields = { [args.field]: args.value.replace(/\\n/g, '\n') };
      }
      const it = queue.edit(item.id, fields, { actor: os.userInfo().username, config });
      save();
      printItem(it);
      break;
    }
    case 'approve': {
      const it = queue.approve(args._[1], { actor: os.userInfo().username, target: args.target, confirmThirdParty: !!args['confirm-third-party'], config });
      save();
      console.log(`${it.id} approved${it.target ? ` for ${it.target}` : ''}. Run "schedule" to give it a slot.`);
      break;
    }
    case 'reject': {
      const it = queue.reject(args._[1], { actor: os.userInfo().username, reason: args.reason || '' });
      save();
      console.log(`${it.id} rejected.`);
      break;
    }
    case 'review': {
      const port = Number(args.port || 8787);
      const { server } = createReviewServer({ queue, config, save });
      server.listen(port, '127.0.0.1', () => console.log(`Review page: http://127.0.0.1:${port}/  (Ctrl-C to stop)`));
      return;
    }
    case 'schedule': {
      const done = scheduleApproved(queue, config);
      save();
      for (const i of done) console.log(`${i.id} ${i.platform} → ${i.scheduledAt}`);
      if (!done.length) console.log('No approved drafts to schedule (or every slot in the next 60 days is taken).');
      console.log('\n' + renderCalendar(queue, config));
      break;
    }
    case 'calendar':
      console.log(renderCalendar(queue, config, new Date(), Number(args.days || 14)));
      break;
    case 'publish': {
      const live = args.live === true;
      if (!live) console.log('DRY RUN: nothing will be posted. Add --live to post.\n');
      const report = await publishDue(queue, config, { live, ids: list(args.id), save: live ? save : () => {}, log: (m) => console.log(`  ${m}`) });
      for (const r of report) {
        const head = `${r.id} ${r.platform}`;
        if (r.skipped) console.log(`${head}: skipped (${r.skipped})`);
        else if (r.exported) console.log(`${head}: manual export → ${r.exported} (${r.note})`);
        else if (r.error) console.log(`${head}: FAILED ${r.error}`);
        else console.log(`${head}: ${live ? 'posted' : 'would post'}${r.result?.url ? ` ${r.result.url}` : ''}${r.result?.note ? ` (${r.result.note})` : ''}`);
        if (!live && r.requests && args.verbose) console.log(JSON.stringify(r.requests, null, 2));
      }
      if (!report.length) console.log('Nothing due.');
      break;
    }
    case 'export': {
      const items = list(args.id) ? list(args.id).map((id) => queue.get(id)) : queue.items.filter((i) => ['approved', 'scheduled'].includes(i.status));
      for (const i of items) {
        if (!i.approval) {
          console.log(`${i.id}: not approved, skipped`);
          continue;
        }
        console.log(`${i.id} ${i.platform} → ${exportDraft(i, config.outDir).dir}`);
      }
      break;
    }
    case 'posted': {
      const i = queue.get(args._[1]);
      if (!i.approval) throw new Error(`${i.id} was never approved`);
      queue.mark(i.id, 'posted', { result: { url: args.url || null, postedAt: new Date().toISOString(), manual: true } }, { actor: os.userInfo().username });
      save();
      console.log(`${i.id} recorded as posted.`);
      break;
    }
    case 'promote': {
      let stories = loadStories(args.stories);
      if (args.story) stories = stories.filter((s) => list(args.story).includes(s.id));
      for (const s of stories) {
        const r = suggestFor(s);
        console.log(`\n## ${s.id} (${r.kind}): ${s.title}\n\nHashtags:`);
        for (const [p, t] of Object.entries(r.hashtags)) console.log(`  ${p.padEnd(9)} ${t.map((x) => '#' + x).join(' ')}`);
        console.log('\nCommunities to consider (read the rules; each third-party post needs your approval):');
        for (const c of r.communities) console.log(`  ${c.where.padEnd(9)} ${c.name}: ${c.note}`);
      }
      break;
    }
    case 'press-kit': {
      const dir = writePressKit({ config, stories: args.stories ? loadStories(args.stories) : [], manifest: loadManifest(args.manifest), outDir: config.outDir });
      console.log(`Press kit: ${dir}`);
      break;
    }
    case 'outreach': {
      if (!args.creators) throw new Error('outreach needs --creators FILE');
      const r = writeOutreach({ config, creators: JSON.parse(fs.readFileSync(args.creators, 'utf8')), stories: args.stories ? loadStories(args.stories) : [], outDir: config.outDir });
      for (const f of r.written) console.log(`draft: ${f}`);
      for (const f of r.refused) console.log(`not drafted: ${f}`);
      console.log('These are drafts. Personalise and send each one yourself.');
      break;
    }
    case 'links': {
      const content = args.story || 'general';
      for (const p of [...ALL_PLATFORMS, 'press', 'outreach']) console.log(`${p.padEnd(10)} ${utmLink(config.site.url, { source: p, campaign: config.site.campaign, content })}`);
      break;
    }
    case 'metrics': {
      const live = args.live === true;
      if (!live) console.log('DRY RUN: showing the metric requests; add --live to call the APIs.\n');
      const snaps = await collectMetrics(queue, config, { live, log: (m) => console.log(`  ${m}`) });
      for (const s of snaps) console.log(`${s.itemId} ${s.platform}: ${s.error ? 'ERROR ' + s.error : JSON.stringify(s.metrics)}`);
      if (!snaps.length) console.log('No posted items with remote ids.');
      break;
    }
    case 'report': {
      const f = path.join(config.dataDir, 'metrics.json');
      const snaps = fs.existsSync(f) ? JSON.parse(fs.readFileSync(f, 'utf8')).snapshots : [];
      console.log(renderSummary(summarize(queue, snaps)));
      break;
    }
    case 'check': {
      for (const p of [...Object.keys(ADAPTERS), 'devlog']) {
        const manual = manualReason(p, config);
        const names = SECRET_ENV[p] || [];
        const set = names.filter((n) => config.env[n]);
        console.log(`${p.padEnd(9)} ${manual ? `manual export (${manual})` : `API; env set: ${set.length ? set.join(', ') : 'none'}${names.length ? ` of ${names.join(', ')}` : ''}`}`);
      }
      console.log(`claude    ${config.env.ANTHROPIC_API_KEY ? `on (${config.claude.model})` : 'off: template drafting'}`);
      break;
    }
    default:
      console.log(HELP);
      process.exitCode = 2;
  }
}

main().catch((e) => {
  console.error(`error: ${e.message}`);
  process.exitCode = 1;
});

#!/usr/bin/env node
// App Transport Security for plain-http dev servers in the iOS app, the iOS
// side of EVO_CAP_CLEARTEXT (Android gets usesCleartextTraffic and mixed
// content from capacitor.config.ts).
//
// Run by Capacitor after every `cap copy` / `cap sync` / `cap add` (the
// `capacitor:copy:after` hook in package.json; a no-op for Android and
// before `cap add ios`). With EVO_CAP_CLEARTEXT=1 it puts an exception into
// ios/App/App/Info.plist:
//
//   NSAppTransportSecurity
//     NSAllowsArbitraryLoadsInWebContent  true   (the WKWebView's fetch and
//                                                 WebSocket to any http host)
//     NSAllowsLocalNetworking             true   (localhost, .local, bare names)
//
// and without it takes exactly these two keys out again (and the dict, if
// nothing else is left in it), so a plain `npm run cap:sync` before a
// release build always gives an https-only app. Other ATS keys someone
// added by hand are left alone.
//
//   node scripts/ios-ats.mjs [path/to/Info.plist]     (EVO_CAP_CLEARTEXT=1 to allow)

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const ATS_KEYS = ['NSAllowsArbitraryLoadsInWebContent', 'NSAllowsLocalNetworking'];

/**
 * Where the NSAppTransportSecurity entry is: `start`..`end` spans its lines
 * from the key's indent to the newline after its value, `body` is the inside
 * of its dict ('' for `<dict/>`). Nested dicts (NSExceptionDomains) are
 * matched by depth.
 */
function findAts(plist) {
  const keyAt = plist.search(/<key>NSAppTransportSecurity<\/key>/);
  if (keyAt < 0) return null;
  const start = plist.lastIndexOf('\n', keyAt) + 1;
  const indent = plist.slice(start, keyAt);
  const open = plist.slice(keyAt).match(/^<key>NSAppTransportSecurity<\/key>\s*<dict(\s*\/)?>/);
  if (!open) throw new Error('NSAppTransportSecurity is not a dict');
  let end = keyAt + open[0].length;
  let body = '';
  if (!open[1]) {
    const bodyStart = end;
    const tag = /<(\/?)dict(\s*\/)?>/g;
    tag.lastIndex = end;
    let depth = 1;
    let t;
    while ((t = tag.exec(plist))) {
      if (t[2]) continue; // an empty <dict/>
      depth += t[1] ? -1 : 1;
      if (depth === 0) break;
    }
    if (!t) throw new Error('unterminated NSAppTransportSecurity dict');
    body = plist.slice(bodyStart, t.index);
    end = t.index + t[0].length;
  }
  if (plist[end] === '\n') end += 1;
  return { start, end, indent, body };
}

/** The `<key>K</key><true/>` (or false) entry for key K in a dict body, with its line. */
const entryRe = (key) => new RegExp(`[ \\t]*<key>${key}</key>\\s*<(?:true|false)\\s*/>[ \\t]*\\n?`);

const atsEntry = (indent, body) => `${indent}<key>NSAppTransportSecurity</key>\n${indent}<dict>${body}${indent}</dict>\n`;
const ourKeys = (indent) => ATS_KEYS.map((k) => `${indent}\t<key>${k}</key>\n${indent}\t<true/>\n`).join('');

/** Whether the plist text allows plain http in web content (both keys true). */
export function hasCleartext(plist) {
  const ats = findAts(plist);
  if (!ats) return false;
  return ATS_KEYS.every((k) => new RegExp(`<key>${k}</key>\\s*<true\\s*/>`).test(ats.body));
}

/** The plist text with the dev exception added (`allow`) or removed. Idempotent. */
export function applyAts(plist, allow) {
  const ats = findAts(plist);
  if (allow) {
    if (hasCleartext(plist)) return plist;
    if (ats) {
      // Merge into the existing dict: drop false copies of our keys, add true ones.
      let body = ats.body;
      for (const k of ATS_KEYS) body = body.replace(entryRe(k), '');
      if (!body.endsWith('\n')) body += '\n';
      return plist.slice(0, ats.start) + atsEntry(ats.indent, body + ourKeys(ats.indent)) + plist.slice(ats.end);
    }
    // A new dict just before the top-level dict closes.
    const close = plist.lastIndexOf('</dict>');
    if (close < 0) throw new Error('not a property list: no top-level </dict>');
    const lineStart = plist.lastIndexOf('\n', close) + 1;
    return plist.slice(0, lineStart) + atsEntry('\t', `\n${ourKeys('\t')}`) + plist.slice(lineStart);
  }
  if (!ats) return plist;
  let body = ats.body;
  for (const k of ATS_KEYS) body = body.replace(entryRe(k), '');
  if (body === ats.body) return plist;
  const replacement = /<key>/.test(body) ? atsEntry(ats.indent, body) : '';
  return plist.slice(0, ats.start) + replacement + plist.slice(ats.end);
}

function main() {
  const platform = process.env.CAPACITOR_PLATFORM_NAME;
  if (platform && platform !== 'ios') return;
  const root = process.env.CAPACITOR_ROOT_DIR ?? path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  const file = process.argv[2] ?? path.join(root, 'ios', 'App', 'App', 'Info.plist');
  if (!existsSync(file)) {
    if (process.argv[2]) throw new Error(`no ${file}`);
    return; // no iOS project yet
  }
  const allow = process.env.EVO_CAP_CLEARTEXT === '1';
  const before = readFileSync(file, 'utf8');
  const after = applyAts(before, allow);
  if (after !== before) writeFileSync(file, after);
  const what = allow ? 'plain http allowed in web content (EVO_CAP_CLEARTEXT=1)' : 'https only';
  console.log(`[ios-ats] ${file}: ${what}${after === before ? ' (unchanged)' : ''}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (e) {
    console.error(`[ios-ats] ${e.message}`);
    process.exit(1);
  }
}

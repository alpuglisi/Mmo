import { describe, expect, it } from 'vitest';
import { applyAts, hasCleartext } from './ios-ats.mjs';

// The shape of Capacitor 8's generated ios/App/App/Info.plist (shortened).
const PLAIN = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CAPACITOR_DEBUG</key>
	<string>$(CAPACITOR_DEBUG)</string>
	<key>CFBundleDisplayName</key>
	<string>Evolution MMO</string>
	<key>UIApplicationSceneManifest</key>
	<dict>
		<key>UIApplicationSupportsMultipleScenes</key>
    <false/>
	</dict>
	<key>UIViewControllerBasedStatusBarAppearance</key>
	<true/>
</dict>
</plist>
`;

describe('ios-ats', () => {
  it('adds the web-content exception before the top-level dict closes', () => {
    const out = applyAts(PLAIN, true);
    expect(hasCleartext(PLAIN)).toBe(false);
    expect(hasCleartext(out)).toBe(true);
    expect(out).toContain(`\t<key>NSAppTransportSecurity</key>
\t<dict>
\t\t<key>NSAllowsArbitraryLoadsInWebContent</key>
\t\t<true/>
\t\t<key>NSAllowsLocalNetworking</key>
\t\t<true/>
\t</dict>
</dict>
</plist>`);
    // Everything else is untouched.
    expect(out.replace(/\t<key>NSAppTransportSecurity<\/key>\n\t<dict>\n(\t\t.*\n){4}\t<\/dict>\n/, '')).toBe(PLAIN);
  });

  it('is idempotent and round-trips to the original', () => {
    const on = applyAts(PLAIN, true);
    expect(applyAts(on, true)).toBe(on);
    expect(applyAts(on, false)).toBe(PLAIN);
    expect(applyAts(PLAIN, false)).toBe(PLAIN);
  });

  it('keeps ATS keys it did not add, including nested exception domains', () => {
    const own = PLAIN.replace('</dict>\n</plist>', `\t<key>NSAppTransportSecurity</key>
\t<dict>
\t\t<key>NSExceptionDomains</key>
\t\t<dict>
\t\t\t<key>example.org</key>
\t\t\t<dict>
\t\t\t\t<key>NSIncludesSubdomains</key>
\t\t\t\t<true/>
\t\t\t</dict>
\t\t</dict>
\t\t<key>NSAllowsLocalNetworking</key>
\t\t<false/>
\t</dict>
</dict>
</plist>`);
    const on = applyAts(own, true);
    expect(hasCleartext(on)).toBe(true);
    expect(on).toContain('<key>example.org</key>');
    expect(on.match(/NSAllowsLocalNetworking/g)).toHaveLength(1);
    expect(on.match(/NSAppTransportSecurity/g)).toHaveLength(1);
    const off = applyAts(on, false);
    expect(hasCleartext(off)).toBe(false);
    expect(off).toContain('<key>NSExceptionDomains</key>');
    expect(off).not.toContain('NSAllowsArbitraryLoadsInWebContent');
    expect(off).not.toContain('NSAllowsLocalNetworking');
    expect(off.endsWith('\t</dict>\n</dict>\n</plist>\n')).toBe(true);
  });

  it('drops a dict that only held its keys, and handles <dict/>', () => {
    const empty = PLAIN.replace('</dict>\n</plist>', '\t<key>NSAppTransportSecurity</key>\n\t<dict/>\n</dict>\n</plist>');
    const on = applyAts(empty, true);
    expect(hasCleartext(on)).toBe(true);
    expect(applyAts(on, false)).toBe(PLAIN);
    expect(applyAts(empty, false)).toBe(empty);
  });

  it('refuses something that is not a plist', () => {
    expect(() => applyAts('nope', true)).toThrow(/not a property list/);
  });
});

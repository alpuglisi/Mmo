// Capacitor wraps the built client (`dist/`) as an iOS / Android app. The
// native projects (`android/`, `ios/`) are generated locally with
// `npx cap add android|ios` and are not committed; see README "Mobile apps".
//
// The app's page lives on capacitor://localhost (iOS) or https://localhost
// (Android), so it talks to a remote game server: set its address on the
// login screen (remembered), bake a default in with VITE_EVO_SERVER at build
// time, or open the app with ?server=… during development. evo-server
// answers cross-origin API calls for this.
//
// Env when running `cap sync` / `cap run`:
//   EVO_CAP_CLEARTEXT=1  allow plain-http servers (a LAN dev server); off by
//                        default, as stores expect https.
//   EVO_CAP_LIVE_URL     load the page from a dev server for live reload
//                        (e.g. http://192.168.1.20:5173) instead of dist/.

import type { CapacitorConfig } from '@capacitor/cli';

const cleartext = process.env.EVO_CAP_CLEARTEXT === '1';
const liveUrl = process.env.EVO_CAP_LIVE_URL;

const config: CapacitorConfig = {
  appId: 'org.evolutionmmo.client',
  appName: 'Evolution MMO',
  webDir: 'dist',
  backgroundColor: '#0d1117',
  server: {
    androidScheme: 'https',
    cleartext,
    ...(liveUrl ? { url: liveUrl } : {}),
  },
  android: {
    // An https page calling an http dev server is mixed content.
    allowMixedContent: cleartext,
  },
  ios: {
    contentInset: 'never',
  },
};

export default config;

// Build-time constants (vite.config.ts `define`).

/**
 * Whether the Android app was built with its Firebase config
 * (`android/app/google-services.json` present at build time, or
 * `VITE_EVO_FCM=1`): without it, registering for push would crash the app.
 */
declare const __EVO_FCM__: boolean;

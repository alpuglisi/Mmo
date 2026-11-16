// The design editor's Simple / Builder mode, remembered per browser
// (docs/morphology.md, section 6: Simple is the default for new players).
// Storage can be missing or throw (private mode, blocked site data): the
// editor then starts in Simple and the choice lasts for the visit.

export type EditorMode = 'simple' | 'builder';

export const MODE_KEY = 'evo.designMode';

export function readMode(): EditorMode {
  try {
    return globalThis.localStorage?.getItem(MODE_KEY) === 'builder' ? 'builder' : 'simple';
  } catch {
    return 'simple';
  }
}

export function writeMode(m: EditorMode): void {
  try {
    globalThis.localStorage?.setItem(MODE_KEY, m);
  } catch {
    // Private mode: the choice holds for this visit.
  }
}

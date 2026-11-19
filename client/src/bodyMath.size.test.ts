import { describe, expect, it } from 'vitest';
import { sizeWord } from './bodyMath';

describe('sizeWord', () => {
  it('uses the server\'s weight classes (one scale: designer and inspector)', () => {
    // Playtest 7: a 4.2 kg body was "medium" in the designer, "small" in the inspector.
    expect([0.5, 4.2, 20, 60].map(sizeWord)).toEqual(['tiny', 'small', 'medium', 'large']);
  });
});

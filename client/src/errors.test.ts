import { describe, expect, it } from 'vitest';
import { commandLabel, commandError } from './errors';

describe('friendly command errors', () => {
  it('names commands as the interface does, never by their wire name', () => {
    expect(commandLabel('request_mating')).toBe('Mating request');
    expect(commandLabel('message')).toBe('Command');
    expect(commandLabel(undefined)).toBe('Command');
    expect(commandLabel('fly_to_the_moon')).toBe('Command');
  });

  it('replaces raw and technical server text', () => {
    expect(commandError('message', { code: 'bad_message', message: 'unknown variant `fly`, expected one of `possess`, ...' }))
      .toBe("Command: The server didn't understand that. Reloading the page may help.");
    expect(commandError('possess', { code: 'odd', message: 'missing field `organism` at line 1 column 20' }))
      .toBe("Possess: That didn't work.");
    expect(commandError('set_view', { code: 'out_of_sight', message: 'x' })).toBe('View: You can only look where your animals are.');
    expect(commandError('request_mating', { code: 'timeout', message: 'no reply from the server' }))
      .toBe('Mating request: No reply from the server: check your connection.');
  });

  it("keeps the server's plain wording, capitalized, without the code", () => {
    const t = commandError('bid', { code: 'bid_too_low', message: 'the lowest bid now is 12 renown' });
    expect(t).toBe('Bid: The lowest bid now is 12 renown');
    expect(t).not.toContain('bid_too_low');
  });
});

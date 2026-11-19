// Unique, letter-only suffixes for the accounts e2e scripts register.
//
// The server refuses names that read as staff ("gm", "mod", "root",
// "claude" ...) even with digits mixed in, so a base36 timestamp such as
// "mi8k3gm2" could be refused at random. These suffixes use consonants
// only, none of which spell a staff word (no c, d, g, m, o, r, a, e, i, u,
// l, n, w), so no suffix reads as a word at all.
const SAFE = 'bfhjkpqstvxyz';
let counter = 0;

/** A fresh letter-only suffix (10 letters, so "m_iphone13_"
 * plus a suffix stays under the 24-character account limit), unique per
 * call within a run and across runs some milliseconds apart. */
export function stamp() {
  // Scrambled (a multiply modulo 13^10, one to one): consecutive stamps
  // differ in most letters, not one, which the server's one-edit look-alike
  // rule would refuse (playtest 10).
  const M = 13n ** 10n;
  let n = ((BigInt(Date.now() % 1e9) * 100n + BigInt(counter++ % 100)) * 2654435761n + 977n) % M;
  let out = '';
  for (let i = 0; i < 10; i++) {
    out = SAFE[Number(n % 13n)] + out;
    n /= 13n;
  }
  return out;
}

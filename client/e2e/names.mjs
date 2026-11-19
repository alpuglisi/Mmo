// Unique, letter-only suffixes for the accounts e2e scripts register.
//
// The server refuses names that read as staff ("gm", "mod", "root",
// "claude" ...) even with digits mixed in, so a base36 timestamp such as
// "mi8k3gm2" could be refused at random. These suffixes use consonants
// only, none of which spell a staff word (no c, d, g, m, o, r, a, e, i, u,
// l, n, w), so no suffix reads as a word at all.
const SAFE = 'bfhjkpqstvxyz';
let counter = 0;

/** A fresh letter-only suffix (10 letters at most, so "m_iphone13_"
 * plus a suffix stays under the 24-character account limit), unique per
 * call within a run and across runs some milliseconds apart. */
export function stamp() {
  let n = (Date.now() % 1e9) * 100 + (counter++ % 100);
  let out = '';
  while (n > 0) {
    out = SAFE[n % SAFE.length] + out;
    n = Math.floor(n / SAFE.length);
  }
  return out;
}

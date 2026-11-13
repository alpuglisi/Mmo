//! Deterministic random streams.
//!
//! Every random draw is derived from `(world seed, stream, a, b)`, typically
//! `(seed, DECIDE, tick, organism id)`. That keeps runs reproducible even
//! though the decision phase runs on many threads in arbitrary order.

use rand::{Rng, SeedableRng};
use rand_pcg::Pcg64Mcg;

pub type SimRng = Pcg64Mcg;

/// Stream identifiers, so different systems never share draws.
pub mod stream {
    pub const MAP: u64 = 1;
    pub const DECIDE: u64 = 2;
    pub const APPLY: u64 = 3;
    pub const ENVIRONMENT: u64 = 4;
    pub const FOUNDING: u64 = 5;
    pub const COMMANDS: u64 = 6;
    /// Disease and hidden-recessive inheritance, kept off the apply stream
    /// so these rules change a world only through their effects.
    pub const GENETICS: u64 = 7;
}

pub fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

pub fn hash4(seed: u64, stream: u64, a: u64, b: u64) -> u64 {
    splitmix64(seed ^ splitmix64(stream ^ splitmix64(a ^ splitmix64(b))))
}

pub fn rng_for(seed: u64, stream: u64, a: u64, b: u64) -> SimRng {
    SimRng::seed_from_u64(hash4(seed, stream, a, b))
}

/// Standard normal sample (Box–Muller).
pub fn normal(rng: &mut impl Rng) -> f32 {
    let u1: f32 = rng.gen_range(f32::EPSILON..1.0);
    let u2: f32 = rng.gen();
    (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
}

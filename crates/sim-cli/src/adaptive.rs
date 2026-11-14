//! Adaptive (sequential) stopping for `compare`: run seeds in batches and
//! stop as soon as the paired difference resolves.
//!
//! Looking at a 95% interval after every batch and stopping the first time it
//! excludes 0 would declare a winner far more often than 5% of the time when
//! there is no difference (every look is another chance for noise to cross).
//! So each look uses Pocock's group-sequential level: with `K` planned looks,
//! every look tests at the nominal two-sided level that keeps the overall
//! false-positive rate at 5% (0.0294 for 2 looks, 0.0182 for 4, 0.0106 for
//! 10), with Student's t in place of the normal for small samples. The
//! intervals printed by an adaptive compare are these wider ones.

use crate::stats::{t_crit, Summary};

/// Stopping rule settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdaptiveArgs {
    /// Seeds in the first look.
    pub initial: u64,
    /// Seeds added per further look.
    pub batch: u64,
    /// Stop after this many seeds even if unresolved.
    pub max_seeds: u64,
    /// Equivalence margin in score points: stop when the whole (sequential)
    /// interval lies inside `(-tolerance, +tolerance)`. 0 disables it.
    pub tolerance: f64,
}

impl AdaptiveArgs {
    /// Seed counts at each look: `initial`, `initial + batch`, … up to
    /// `max_seeds` (the last look is at `max_seeds` exactly).
    pub fn looks(&self) -> Vec<u64> {
        let max = self.max_seeds.max(self.initial).max(1);
        let mut v = vec![self.initial.clamp(1, max)];
        while *v.last().expect("non-empty") < max {
            let next = (v.last().expect("non-empty") + self.batch.max(1)).min(max);
            v.push(next);
        }
        v
    }

    /// Nominal two-sided level of each look.
    pub fn alpha(&self) -> f64 {
        pocock_alpha(self.looks().len())
    }
}

/// Pocock's nominal per-look two-sided level for `k` equally spaced looks at
/// an overall 5% level (from the Pocock constants 1.960, 2.178, 2.289, …;
/// linear between the tabulated 10, 15 and 20 looks, Bonferroni beyond,
/// which is conservative).
pub fn pocock_alpha(k: usize) -> f64 {
    const TABLE: [f64; 10] = [0.0500, 0.0294, 0.0221, 0.0182, 0.0158, 0.0142, 0.0130, 0.0120, 0.0112, 0.0106];
    match k {
        0 | 1 => TABLE[0],
        2..=10 => TABLE[k - 1],
        11..=15 => 0.0106 + (0.0086 - 0.0106) * (k - 10) as f64 / 5.0,
        16..=20 => 0.0086 + (0.0075 - 0.0086) * (k - 15) as f64 / 5.0,
        _ => 0.05 / k as f64,
    }
}

/// Why an adaptive compare stopped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stop {
    /// The sequential interval excludes 0: A (`true`) or B is better.
    Resolved { a_better: bool },
    /// The whole interval lies within ±tolerance: no difference that matters.
    Equivalent,
    /// Seed cap reached without either.
    Cap,
}

/// One look: the seeds so far, the difference summary and the sequential
/// interval half-width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub seeds: u64,
    pub diff: Summary,
    pub half_width: f64,
}

/// The decision at one look, or `None` to continue.
pub fn decide(args: &AdaptiveArgs, look: &Look, last: bool) -> Option<Stop> {
    let d = &look.diff;
    if d.n >= 2 {
        let zero_spread = d.se == 0.0;
        if (zero_spread && d.mean != 0.0) || d.mean.abs() > look.half_width {
            return Some(Stop::Resolved { a_better: d.mean > 0.0 });
        }
        if args.tolerance > 0.0 && d.mean.abs() + look.half_width < args.tolerance {
            return Some(Stop::Equivalent);
        }
    }
    last.then_some(Stop::Cap)
}

/// Run the looks. `extend(from, to)` must run seeds `from..to` (offsets from
/// the base seed) and return all paired differences so far, seeds ascending.
pub fn run(args: &AdaptiveArgs, mut extend: impl FnMut(u64, u64) -> Vec<f64>) -> (Vec<Look>, Stop) {
    let looks = args.looks();
    let alpha = args.alpha();
    let mut done = 0;
    let mut history = Vec::new();
    for (i, &n) in looks.iter().enumerate() {
        let diffs = extend(done, n);
        done = n;
        let diff = Summary::of(&diffs);
        let half_width = if diff.n >= 2 { t_crit(alpha, diff.n - 1) * diff.se } else { f64::INFINITY };
        let look = Look { seeds: n, diff, half_width };
        history.push(look);
        if let Some(stop) = decide(args, &look, i + 1 == looks.len()) {
            return (history, stop);
        }
    }
    unreachable!("the last look always stops")
}

#[cfg(test)]
mod tests {
    use super::*;
    use evo_sim::rng::rng_for;
    use rand::Rng;

    fn args(initial: u64, batch: u64, max_seeds: u64, tolerance: f64) -> AdaptiveArgs {
        AdaptiveArgs { initial, batch, max_seeds, tolerance }
    }

    #[test]
    fn looks_end_at_the_cap() {
        assert_eq!(args(12, 12, 48, 0.0).looks(), vec![12, 24, 36, 48]);
        assert_eq!(args(12, 8, 30, 0.0).looks(), vec![12, 20, 28, 30]);
        assert_eq!(args(12, 12, 12, 0.0).looks(), vec![12]);
        // A cap below the first batch: one look at the first batch.
        assert_eq!(args(12, 12, 6, 0.0).looks(), vec![12]);
        assert_eq!(args(12, 12, 48, 0.0).alpha(), 0.0182);
        assert_eq!(args(12, 12, 12, 0.0).alpha(), 0.05);
    }

    #[test]
    fn stops_on_clear_differences_and_equivalence() {
        // A constant +1 difference with a little noise resolves at once.
        let (h, stop) = run(&args(4, 4, 16, 0.0), |_, to| (0..to).map(|i| 1.0 + 0.01 * (i % 3) as f64).collect());
        assert_eq!(stop, Stop::Resolved { a_better: true });
        assert_eq!(h.len(), 1);
        // Pure symmetric noise of ±0.05 is equivalent within ±0.25.
        let (_, stop) =
            run(&args(8, 8, 32, 0.25), |_, to| (0..to).map(|i| if i % 2 == 0 { 0.05 } else { -0.05 }).collect());
        assert_eq!(stop, Stop::Equivalent);
        // Without a tolerance the same noise runs to the cap.
        let (h, stop) =
            run(&args(8, 8, 32, 0.0), |_, to| (0..to).map(|i| if i % 2 == 0 { 0.05 } else { -0.05 }).collect());
        assert_eq!(stop, Stop::Cap);
        assert_eq!(h.last().map(|l| l.seeds), Some(32));
    }

    /// Monte Carlo check of the error rate the Pocock levels exist for: on
    /// normal differences with true mean 0, 12 + 12k seeds up to 48 (4 looks)
    /// declare a winner about 5% of the time. Stopping at the first naive 95%
    /// look would do so roughly twice as often.
    #[test]
    fn false_positive_rate_stays_near_five_percent() {
        let a = args(12, 12, 48, 0.0);
        let trials = 4000;
        let mut false_pos = 0;
        let mut naive = 0;
        for t in 0..trials {
            let mut rng = rng_for(7, 0xada, t, 0);
            let data: Vec<f64> = (0..48)
                .map(|_| {
                    // Box-Muller.
                    let (u, v): (f64, f64) = (rng.gen_range(1e-12..1.0), rng.gen_range(0.0..1.0));
                    (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
                })
                .collect();
            let (_, stop) = run(&a, |_, to| data[..to as usize].to_vec());
            if matches!(stop, Stop::Resolved { .. }) {
                false_pos += 1;
            }
            if a.looks().iter().any(|&n| Summary::of(&data[..n as usize]).significant()) {
                naive += 1;
            }
        }
        let rate = false_pos as f64 / trials as f64;
        let naive_rate = naive as f64 / trials as f64;
        assert!((0.035..0.065).contains(&rate), "sequential false-positive rate {rate}");
        assert!(naive_rate > 0.09, "naive rate {naive_rate}");
    }
}

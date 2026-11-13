//! Small-sample statistics for sweep and compare scores: mean, standard
//! error and Student-t 95% confidence intervals, plus Wilson intervals for
//! survival rates.

/// Mean, sample standard deviation, standard error and 95% CI half-width of
/// a set of values (one per seed).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Summary {
    pub n: usize,
    pub mean: f64,
    pub sd: f64,
    pub se: f64,
    /// Half-width of the two-sided 95% Student-t interval for the mean
    /// (`t(0.975, n-1) · se`); infinite for fewer than 2 values.
    pub ci95: f64,
}

impl Summary {
    pub fn of(values: &[f64]) -> Summary {
        let n = values.len();
        if n == 0 {
            return Summary { n, mean: f64::NAN, sd: f64::NAN, se: f64::NAN, ci95: f64::INFINITY };
        }
        let mean = values.iter().sum::<f64>() / n as f64;
        if n < 2 {
            return Summary { n, mean, sd: f64::NAN, se: f64::NAN, ci95: f64::INFINITY };
        }
        let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
        let sd = var.sqrt();
        let se = sd / (n as f64).sqrt();
        Summary { n, mean, sd, se, ci95: t95(n - 1) * se }
    }

    /// True when the 95% interval excludes zero (for paired differences).
    pub fn significant(&self) -> bool {
        self.n >= 2 && (self.mean.abs() > self.ci95 || (self.se == 0.0 && self.mean != 0.0))
    }

    /// The t statistic `mean / se` (0 when both are zero).
    pub fn t(&self) -> f64 {
        if self.se > 0.0 {
            self.mean / self.se
        } else if self.mean == 0.0 {
            0.0
        } else {
            self.mean.signum() * f64::INFINITY
        }
    }
}

/// Two-sided 95% critical value of Student's t with `df` degrees of freedom.
pub fn t95(df: usize) -> f64 {
    const TABLE: [f64; 30] = [
        12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160, 2.145, 2.131,
        2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056, 2.052, 2.048, 2.045, 2.042,
    ];
    match df {
        0 => f64::INFINITY,
        1..=30 => TABLE[df - 1],
        // Cornish-Fisher style expansion around the normal quantile: within
        // 0.001 of the exact value beyond 30 degrees of freedom.
        _ => {
            let z = 1.959964f64;
            let d = df as f64;
            z + (z.powi(3) + z) / (4.0 * d) + (5.0 * z.powi(5) + 16.0 * z.powi(3) + 3.0 * z) / (96.0 * d * d)
        }
    }
}

/// Wilson score 95% interval for `k` successes out of `n`: unlike the normal
/// approximation it stays inside [0, 1] and is not zero-width at 0% or 100%.
pub fn wilson(k: usize, n: usize) -> (f64, f64) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let z = 1.959964f64;
    let n = n as f64;
    let p = k as f64 / n;
    let denom = 1.0 + z * z / n;
    let centre = (p + z * z / (2.0 * n)) / denom;
    let half = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt() / denom;
    ((centre - half).max(0.0), (centre + half).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_matches_hand_computation() {
        let s = Summary::of(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(s.n, 4);
        assert!((s.mean - 2.5).abs() < 1e-12);
        assert!((s.sd - (5.0f64 / 3.0).sqrt()).abs() < 1e-12);
        assert!((s.se - s.sd / 2.0).abs() < 1e-12);
        assert!((s.ci95 - 3.182 * s.se).abs() < 1e-12);
        assert!(s.significant());
        assert!(!Summary::of(&[-1.0, 1.0, -1.0, 1.5]).significant());
        assert!(Summary::of(&[5.0]).ci95.is_infinite());
        assert!(!Summary::of(&[5.0]).significant());
        // Identical non-zero differences: zero spread, clearly significant.
        assert!(Summary::of(&[0.5, 0.5, 0.5]).significant());
        assert!(!Summary::of(&[0.0, 0.0, 0.0]).significant());
    }

    #[test]
    fn t_critical_values() {
        assert!(t95(0).is_infinite());
        assert!((t95(11) - 2.201).abs() < 1e-9);
        // Exact: df 40 → 2.0211, df 60 → 2.0003, df 120 → 1.9799.
        assert!((t95(40) - 2.0211).abs() < 1e-3);
        assert!((t95(60) - 2.0003).abs() < 1e-3);
        assert!((t95(120) - 1.9799).abs() < 1e-3);
        // Monotone decreasing towards the normal quantile.
        for df in 1..200 {
            assert!(t95(df + 1) < t95(df));
        }
    }

    #[test]
    fn wilson_interval() {
        let (lo, hi) = wilson(12, 12);
        assert!(lo > 0.7 && lo < 0.8 && hi == 1.0, "{lo} {hi}");
        let (lo, hi) = wilson(0, 12);
        assert!(lo == 0.0 && hi > 0.2 && hi < 0.3);
        let (lo, hi) = wilson(6, 12);
        assert!((lo + hi - 1.0).abs() < 1e-12 && lo > 0.2);
    }
}

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

/// Two-sided critical value of Student's t at level `alpha` with `df`
/// degrees of freedom: the `t` with `P(|T| > t) = alpha`. Found by bisection
/// on the exact tail (a regularized incomplete beta function), so any level
/// works, not only 5% (the sequential compare uses stricter per-look levels).
pub fn t_crit(alpha: f64, df: usize) -> f64 {
    if df == 0 || alpha <= 0.0 {
        return f64::INFINITY;
    }
    if alpha >= 1.0 {
        return 0.0;
    }
    let tail = |t: f64| {
        let d = df as f64;
        incomplete_beta(d / 2.0, 0.5, d / (d + t * t))
    };
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    while tail(hi) > alpha {
        hi *= 2.0;
        if hi > 1e12 {
            return f64::INFINITY;
        }
    }
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if tail(mid) > alpha {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo < 1e-12 * hi.max(1.0) {
            break;
        }
    }
    0.5 * (lo + hi)
}

/// Natural log of the gamma function (Lanczos approximation, |error| < 1e-10
/// for positive arguments).
fn ln_gamma(x: f64) -> f64 {
    const G: [f64; 6] = [
        76.18009172947146,
        -86.50532032941677,
        24.01409824083091,
        -1.231739572450155,
        0.1208650973866179e-2,
        -0.5395239384953e-5,
    ];
    let mut y = x;
    let tmp = x + 5.5;
    let tmp = tmp - (x + 0.5) * tmp.ln();
    let mut ser = 1.000000000190015;
    for g in G {
        y += 1.0;
        ser += g / y;
    }
    -tmp + (2.5066282746310005 * ser / x).ln()
}

/// Regularized incomplete beta function `I_x(a, b)` (continued fraction, as
/// in Numerical Recipes' `betai`).
fn incomplete_beta(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let front = (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        front * beta_cf(a, b, x) / a
    } else {
        1.0 - front * beta_cf(b, a, 1.0 - x) / b
    }
}

fn beta_cf(a: f64, b: f64, x: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let (qab, qap, qam) = (a + b, a + 1.0, a - 1.0);
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..300 {
        let m = m as f64;
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < 1e-14 {
            break;
        }
    }
    h
}

/// Ranks of `values` (1 = smallest), ties sharing their average rank.
pub fn ranks(values: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..values.len()).collect();
    idx.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
    let mut r = vec![0.0; values.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && values[idx[j + 1]] == values[idx[i]] {
            j += 1;
        }
        let avg = (i + j) as f64 / 2.0 + 1.0;
        for &k in &idx[i..=j] {
            r[k] = avg;
        }
        i = j + 1;
    }
    r
}

/// Pearson correlation of two equally long series (NaN if either is constant).
pub fn pearson(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len().min(y.len());
    if n < 2 {
        return f64::NAN;
    }
    let mx = x[..n].iter().sum::<f64>() / n as f64;
    let my = y[..n].iter().sum::<f64>() / n as f64;
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (dx, dy) = (x[i] - mx, y[i] - my);
        sxy += dx * dy;
        sxx += dx * dx;
        syy += dy * dy;
    }
    sxy / (sxx * syy).sqrt()
}

/// Spearman rank correlation: the Pearson correlation of the ranks (ties get
/// average ranks).
pub fn spearman(x: &[f64], y: &[f64]) -> f64 {
    pearson(&ranks(x), &ranks(y))
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

    #[test]
    fn t_crit_matches_tables() {
        assert!((t_crit(0.05, 11) - 2.201).abs() < 1e-3);
        assert!((t_crit(0.05, 1) - 12.706).abs() < 1e-2);
        for df in [2, 5, 23, 47, 100] {
            assert!((t_crit(0.05, df) - t95(df)).abs() < 2e-3, "df {df}");
        }
        // 1% level: t(0.995, 10) = 3.169, t(0.995, 30) = 2.750.
        assert!((t_crit(0.01, 10) - 3.169).abs() < 1e-3);
        assert!((t_crit(0.01, 30) - 2.750).abs() < 1e-3);
        assert!(t_crit(0.05, 0).is_infinite());
    }

    #[test]
    fn rank_correlation() {
        assert_eq!(ranks(&[3.0, 1.0, 2.0, 1.0]), vec![4.0, 1.5, 3.0, 1.5]);
        assert!((spearman(&[1.0, 2.0, 3.0], &[10.0, 20.0, 90.0]) - 1.0).abs() < 1e-12);
        assert!((spearman(&[1.0, 2.0, 3.0], &[3.0, 2.0, 1.0]) + 1.0).abs() < 1e-12);
        // Hand-computed: ranks (1,2,3,4) vs (2,1,4,3): 1 − 6·4/(4·15) = 0.6.
        assert!((spearman(&[1.0, 2.0, 3.0, 4.0], &[2.0, 1.0, 4.0, 3.0]) - 0.6).abs() < 1e-12);
    }
}

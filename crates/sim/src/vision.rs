//! Directional vision: what an organism's eyes see around its heading
//! (`Balance::directional_vision`; the eye model is morphology contract v1,
//! `docs/morphology-contract.md`, and its vision effects layer
//! `docs/morphology.md`).
//!
//! Angles are degrees on the head, 0 = straight ahead, +90 = right, ±180 =
//! straight back. An odd eye count puts one eye on the midline (0°); pair
//! `k` (k = 0, 1, 2) sits at ±min(170°, azimuth + 50°·k). Each eye sees a
//! field `140° + 60° × eye_size` wide, centered on its azimuth.
//!
//! Everything here is computed once per organism at birth ([`Vision::new`],
//! held in `Traits::vision`). Per neighbor the simulation then needs one
//! bearing (two products, no trigonometry) and a table lookup
//! ([`Vision::eyes_on`]).

use crate::genome::{Gene, Phenotype};
use crate::math::Vec2;
use crate::morph_effects::{body_plan, BodyPlan};

/// Eye layout (and snout, which occludes binocular overlap), as in the
/// morphology contract.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyeLayout {
    /// 1–6 eyes.
    pub count: u32,
    /// Angle of the first pair in degrees, 0–180 (0 = both forward).
    pub azimuth: f32,
    /// 0–1: widens each eye's field.
    pub size: f32,
    /// 0–1 (default 0.5): above 0.5 the muzzle hides part of the
    /// binocular field.
    pub snout: f32,
}

impl EyeLayout {
    /// Field width of one eye in degrees: `140 + 60 × size`.
    pub fn field(&self) -> f32 {
        140.0 + 60.0 * self.size.clamp(0.0, 1.0)
    }

    /// Azimuth of every eye, degrees in [-170, 170].
    fn azimuths(&self) -> impl Iterator<Item = f32> + '_ {
        let count = self.count.clamp(1, 6);
        let az = self.azimuth.clamp(0.0, 180.0);
        let midline = (count % 2 == 1).then_some(0.0);
        let pair = move |k: u32| (az + 50.0 * k as f32).min(170.0);
        midline.into_iter().chain((0..count / 2).flat_map(move |k| [pair(k), -pair(k)]))
    }

    /// How many eyes see a target at `bearing` degrees from straight ahead.
    pub fn eyes_at(&self, bearing: f32) -> u32 {
        let half = 0.5 * self.field();
        self.azimuths().filter(|&a| angle_between(bearing, a) <= half).count() as u32
    }

    /// Degrees of binocular field the snout hides ([`snout_occlusion`]).
    pub fn occlusion(&self) -> f32 {
        snout_occlusion(self.snout)
    }

    /// The eyes and snout of a body plan, with the `Eyes` gene as size.
    pub fn of_plan(plan: &BodyPlan, eyes_gene: f32) -> EyeLayout {
        EyeLayout {
            count: u32::from(plan.eye_count),
            azimuth: plan.eye_azimuth,
            size: eyes_gene,
            snout: plan.snout_length,
        }
    }
}

/// Degrees of binocular field a snout of length `snout` (0–1) hides:
/// `40 × max(0, snout − 0.5)`. The one formula for it: the eye geometry
/// ([`EyeLayout::occlusion`]) and the body plan readout
/// (`MorphEffects::snout_occlusion_deg`) both call it.
pub fn snout_occlusion(snout: f32) -> f32 {
    40.0 * (snout - 0.5).max(0.0)
}

/// The eye layout read from a phenotype: the body plan's eyes and snout
/// (`morph_effects::body_plan`, the contract's defaults of two eyes at ±60°
/// and snout 0.5 until the genome carries the loci) with the `Eyes` gene as
/// size.
pub fn eye_layout(p: &Phenotype) -> EyeLayout {
    EyeLayout::of_plan(&body_plan(p), p[Gene::Eyes])
}

/// The sight channel of the perception stat: the vision share of the 0.15
/// base (0.10) plus the eyes' `0.3 × Eyes` (hearing keeps 0.03 of the base,
/// smell 0.02).
pub fn sight(eyes_gene: f32) -> f32 {
    0.10 + 0.3 * eyes_gene
}

/// Absolute difference of two angles in degrees, 0–180.
fn angle_between(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// Bins of [`Vision`]'s lookup table over the folded bearing 0–180°.
const BINS: usize = 32;

/// Pseudo-angle of a direction `(x, y)` with `y >= 0` (x ahead, y to the
/// side): 0 straight ahead, 1 abeam, 2 straight back, monotone in the true
/// angle and within a few degrees of `angle / 90`. Needs no trigonometry.
fn pseudo_angle(x: f32, y: f32) -> f32 {
    if x >= 0.0 {
        let s = x + y;
        if s > 0.0 {
            y / s
        } else {
            0.0
        }
    } else {
        1.0 - x / (y - x)
    }
}

/// True angle in degrees of a pseudo-angle (the inverse of [`pseudo_angle`]).
fn true_angle(p: f32) -> f32 {
    let (x, y) = if p <= 1.0 { (1.0 - p, p) } else { (1.0 - p, 2.0 - p) };
    y.atan2(x).to_degrees()
}

/// True angle of each table bin's center.
fn bin_angles() -> &'static [f32; BINS] {
    static ANGLES: std::sync::OnceLock<[f32; BINS]> = std::sync::OnceLock::new();
    ANGLES.get_or_init(|| std::array::from_fn(|b| true_angle((b as f32 + 0.5) * 2.0 / BINS as f32)))
}

/// What an organism's eyes see, precomputed at birth.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vision {
    /// Eyes on a target, 2 bits per bin (0, 1 or 2 = two or more), over
    /// [`BINS`] bins of the folded bearing's pseudo-angle.
    table: u64,
    /// Fraction of the full circle seen by at least one eye.
    pub coverage: f32,
    /// Depth field: degrees within ±60° of straight ahead seen by two or
    /// more eyes, less the snout's occlusion.
    pub binocular: f32,
    /// Degrees around straight back seen by no eye.
    pub blind: f32,
    /// Degrees around straight ahead seen by no eye (side eyes).
    pub front_gap: f32,
    /// The sight channel's share of the perception stat
    /// (`(0.10 + 0.3 × Eyes) / perception`): what a target in a blind
    /// sector takes off the distance it is noticed at, at full
    /// `directional_vision`.
    pub visual_share: f32,
}

impl Vision {
    /// `sight` is the perception stat's sight channel ([`sight`]) and
    /// `perception` the whole stat. Exact: the eye count on the folded
    /// bearing 0–180° (the layout is mirrored, so one side tells both) only
    /// changes at a field edge, so it is evaluated once per stretch between
    /// edges (at most 13 stretches), plus once per table bin.
    pub fn new(eyes: &EyeLayout, sight: f32, perception: f32) -> Vision {
        let mut table = 0u64;
        for (b, &angle) in bin_angles().iter().enumerate() {
            table |= (eyes.eyes_at(angle).min(2) as u64) << (2 * b);
        }
        let half = 0.5 * eyes.field();
        let fold = |x: f32| angle_between(x, 0.0);
        let mut edges = [180.0f32; 14];
        edges[0] = 0.0;
        let mut n = 2;
        for a in eyes.azimuths() {
            edges[n] = fold(a - half);
            edges[n + 1] = fold(a + half);
            n += 2;
        }
        let edges = &mut edges[..n];
        edges.sort_by(f32::total_cmp);
        // (start, end, eyes) of each stretch.
        let stretches = edges.windows(2).map(|w| (w[0], w[1], eyes.eyes_at(0.5 * (w[0] + w[1]))));
        let (mut unseen, mut both) = (0.0f32, 0.0f32);
        for (l, r, k) in stretches.clone() {
            if k == 0 {
                unseen += r - l;
            }
            if k >= 2 {
                both += (r.min(60.0) - l).max(0.0);
            }
        }
        let gap = |it: &mut dyn Iterator<Item = (f32, f32, u32)>| {
            it.filter(|&(l, r, _)| r > l).take_while(|&(_, _, k)| k == 0).map(|(l, r, _)| r - l).sum::<f32>()
        };
        let front = gap(&mut stretches.clone());
        let back = gap(&mut stretches.rev());
        Vision {
            table,
            coverage: 1.0 - unseen / 180.0,
            binocular: (2.0 * both - eyes.occlusion()).max(0.0),
            blind: 2.0 * back,
            front_gap: 2.0 * front,
            visual_share: if perception > 0.0 { (sight / perception).clamp(0.0, 1.0) } else { 0.0 },
        }
    }

    /// From a phenotype and its perception stat (see [`eye_layout`]).
    pub fn of(p: &Phenotype, perception: f32) -> Vision {
        Vision::new(&eye_layout(p), sight(p[Gene::Eyes]), perception)
    }

    /// Eyes (0, 1, or 2 for two or more) on a target `to` away (target minus
    /// viewer) from a viewer facing unit vector `heading`. With no heading
    /// (zero) or no offset, everything counts as straight ahead.
    #[inline]
    pub fn eyes_on(&self, heading: Vec2, to: Vec2) -> u32 {
        let x = to.x * heading.x + to.y * heading.y;
        let y = (to.x * heading.y - to.y * heading.x).abs();
        let bin = ((pseudo_angle(x, y) * (BINS as f32 / 2.0)) as usize).min(BINS - 1);
        ((self.table >> (2 * bin)) & 3) as u32
    }

    /// Factor on a hunter's kill probability for depth judgment, at
    /// `directional_vision` `k`: `1 + k × DEPTH_GAIN × (min(binocular, 90)
    /// − DEPTH_DEFAULT) / 90` (the spec's depth multiplier).
    pub fn depth_factor(&self, k: f32) -> f32 {
        1.0 + k * DEPTH_GAIN * (self.binocular.min(90.0) - DEPTH_DEFAULT) / 90.0
    }
}

/// `k_depth` of the spec's depth multiplier (its post-calibration estimate).
pub const DEPTH_GAIN: f32 = 0.35;
/// Binocular degrees of the contract's default eyes (two at ±60°, size 0,
/// snout 0.5): the depth multiplier is 1 there.
pub const DEPTH_DEFAULT: f32 = 20.0;

/// Factor on the distance a target is noticed at (`World::decide`), at
/// `directional_vision` `k`. The sight channel is gated by `g`: 1 where an
/// eye sees, 0 in a blind sector, and for a viewer standing still
/// (`scanning`) blended halfway toward the mean (`coverage`), scaled by
/// `scan` (`Balance::vision_scan`): `g = 0.5 × scan × coverage`. Then
/// `1 − k × visual_share × (1 − g)`: hearing and smell still work.
#[inline]
pub fn detect_factor(eyes: u32, v: &Vision, k: f32, scanning: bool, scan: f32) -> f32 {
    if eyes > 0 {
        return 1.0;
    }
    let g = if scanning { 0.5 * scan * v.coverage } else { 0.0 };
    (1.0 - k * v.visual_share * (1.0 - g)).max(0.0)
}

/// Facing for an organism that hasn't moved yet: a fixed direction from its
/// id (one of 16), so newborns don't all face the same way.
pub fn birth_heading(id: u64) -> Vec2 {
    const R: f32 = std::f32::consts::FRAC_1_SQRT_2;
    const DIRS: [(f32, f32); 16] = {
        // cos/sin of k·22.5°, k = 0..16.
        [
            (1.0, 0.0),
            (0.923_879_5, 0.382_683_43),
            (R, R),
            (0.382_683_43, 0.923_879_5),
            (0.0, 1.0),
            (-0.382_683_43, 0.923_879_5),
            (-R, R),
            (-0.923_879_5, 0.382_683_43),
            (-1.0, 0.0),
            (-0.923_879_5, -0.382_683_43),
            (-R, -R),
            (-0.382_683_43, -0.923_879_5),
            (0.0, -1.0),
            (0.382_683_43, -0.923_879_5),
            (R, -R),
            (0.923_879_5, -0.382_683_43),
        ]
    };
    let (x, y) = DIRS[(crate::rng::splitmix64(id) >> 60) as usize];
    Vec2::new(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(count: u32, azimuth: f32, size: f32) -> EyeLayout {
        EyeLayout { count, azimuth, size, snout: 0.5 }
    }

    fn vision(l: &EyeLayout) -> Vision {
        Vision::new(l, sight(l.size), 0.6)
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 0.6
    }

    /// The vision prototype's table (`docs/morphology.md`, appendix): view,
    /// depth, blind ahead, blind behind.
    #[test]
    fn matches_the_spec_prototype_table() {
        let snout = |mut l: EyeLayout, s: f32| {
            l.snout = s;
            l
        };
        let cases = [
            (layout(2, 60.0, 0.0), 260.0, 20.0, 0.0, 100.0),
            (layout(2, 90.0, 0.0), 280.0, 0.0, 40.0, 40.0),
            (layout(3, 90.0, 0.0), 320.0, 80.0, 0.0, 40.0),
            (layout(2, 100.0, 0.5), 330.0, 0.0, 30.0, 0.0),
            (layout(2, 25.0, 0.6), 226.0, 120.0, 0.0, 134.0),
            (layout(4, 25.0, 0.6), 326.0, 120.0, 0.0, 34.0),
            (snout(layout(2, 25.0, 0.6), 1.0), 226.0, 100.0, 0.0, 134.0),
            (snout(layout(2, 60.0, 0.0), 1.0), 260.0, 0.0, 0.0, 100.0),
        ];
        for (l, view, depth, ahead, behind) in cases {
            let v = vision(&l);
            let got = (v.coverage * 360.0, v.binocular, v.front_gap, v.blind);
            assert!(
                close(got.0, view) && close(got.1, depth) && close(got.2, ahead) && close(got.3, behind),
                "{l:?}: {got:?}"
            );
        }
    }

    #[test]
    fn default_eyes_widen_with_the_eyes_gene() {
        // Two eyes at ±60, field 140 + 60e: overlap 20 + 60e ahead, blind
        // 100 - 60e behind.
        for e in [0.0f32, 0.25, 0.5, 1.0] {
            let v = vision(&layout(2, 60.0, e));
            assert!(close(v.binocular, 20.0 + 60.0 * e), "e {e}: binocular {}", v.binocular);
            assert!(close(v.blind, 100.0 - 60.0 * e), "e {e}: blind {}", v.blind);
            assert!((v.coverage - (360.0 - v.blind) / 360.0).abs() < 0.01);
        }
    }

    #[test]
    fn odd_counts_add_a_midline_eye_and_extra_pairs_step_back() {
        let one = layout(1, 60.0, 0.0);
        assert_eq!((one.eyes_at(0.0), one.eyes_at(69.0), one.eyes_at(71.0)), (1, 1, 0));
        // Six eyes: pairs at 60, 110, 160 see all round.
        let six = vision(&layout(6, 60.0, 0.0));
        assert_eq!((six.blind, six.coverage), (0.0, 1.0));
        // Pairs never pass 170 degrees.
        let back = layout(4, 150.0, 0.0);
        assert_eq!(back.azimuths().collect::<Vec<_>>(), [150.0, -150.0, 170.0, -170.0]);
    }

    #[test]
    fn lookup_follows_the_bearing_relative_to_heading() {
        let v = vision(&layout(2, 60.0, 0.5));
        let north = Vec2::new(0.0, 1.0);
        // Ahead: both eyes; abeam: one; behind: none (blind 70 degrees).
        assert_eq!(v.eyes_on(north, Vec2::new(0.0, 3.0)), 2);
        assert_eq!(v.eyes_on(north, Vec2::new(-2.0, 0.0)), 1);
        assert_eq!(v.eyes_on(north, Vec2::new(2.0, 0.0)), 1);
        assert_eq!(v.eyes_on(north, Vec2::new(0.3, -3.0)), 0);
        // Rotating viewer and target together changes nothing.
        let east = Vec2::new(1.0, 0.0);
        assert_eq!(v.eyes_on(east, Vec2::new(-3.0, 0.3)), 0);
        assert_eq!(v.eyes_on(east, Vec2::new(3.0, 0.0)), 2);
        // No heading or no offset: straight ahead.
        assert_eq!(v.eyes_on(Vec2::ZERO, Vec2::new(0.0, -3.0)), 2);
        assert_eq!(v.eyes_on(north, Vec2::ZERO), 2);
        // Side eyes leave a gap straight ahead.
        let side = vision(&layout(2, 90.0, 0.0));
        assert_eq!(side.eyes_on(east, Vec2::new(3.0, 0.0)), 0);
    }

    #[test]
    fn table_agrees_with_the_geometry_away_from_edges() {
        let eyes = layout(3, 75.0, 0.3);
        let v = vision(&eyes);
        let half = 0.5 * eyes.field();
        for deg in (0..360).step_by(3) {
            let a = (deg as f32).to_radians();
            let n = eyes.eyes_at(deg as f32).min(2);
            // Bins are about 6 degrees wide: skip bearings near a field edge.
            let near_edge = [0.0f32, 75.0, -75.0].iter().any(|&c| (angle_between(deg as f32, c) - half).abs() < 7.0);
            if !near_edge {
                assert_eq!(v.eyes_on(Vec2::new(1.0, 0.0), Vec2::new(a.cos(), a.sin())), n, "bearing {deg}");
            }
        }
    }

    #[test]
    fn pseudo_angle_round_trips() {
        for deg in [0.0f32, 10.0, 45.0, 90.0, 120.0, 179.0] {
            let a = deg.to_radians();
            let p = pseudo_angle(a.cos(), a.sin());
            assert!((true_angle(p) - deg).abs() < 0.01, "{deg}");
        }
    }

    #[test]
    fn blind_targets_lose_only_the_sight_channel() {
        // Eyes, ears and nose 0.5: perception 0.6, sight 0.25.
        let v = vision(&layout(2, 60.0, 0.5));
        assert!((v.visual_share - 0.25 / 0.6).abs() < 1e-6);
        assert_eq!(detect_factor(0, &v, 0.0, false, 1.0), 1.0);
        assert!((detect_factor(0, &v, 1.0, false, 1.0) - (1.0 - 0.25 / 0.6)).abs() < 1e-6);
        assert_eq!(detect_factor(1, &v, 1.0, false, 1.0), 1.0);
        // Standing still, it scans: half the mean sight returns.
        let g = 0.5 * v.coverage;
        assert!((detect_factor(0, &v, 1.0, true, 1.0) - (1.0 - v.visual_share * (1.0 - g))).abs() < 1e-6);
    }

    #[test]
    fn depth_factor_is_one_for_default_eyes() {
        let v = vision(&layout(2, 60.0, 0.0));
        assert!((v.depth_factor(1.0) - 1.0).abs() < 0.01);
        let stalker = vision(&layout(2, 25.0, 0.6));
        assert!((stalker.depth_factor(1.0) - (1.0 + DEPTH_GAIN * 70.0 / 90.0)).abs() < 0.01);
        assert_eq!(stalker.depth_factor(0.0), 1.0);
        let side = vision(&layout(2, 90.0, 0.0));
        assert!(side.depth_factor(1.0) < 1.0);
    }

    #[test]
    fn birth_headings_are_unit_vectors() {
        for id in 0..64 {
            assert!((birth_heading(id).len() - 1.0).abs() < 1e-5);
        }
    }
}

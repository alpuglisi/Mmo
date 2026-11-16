//! What the player's own organisms see, for the client's vision cones
//! (`docs/morphology.md`, "Decisions taken" #8: cones are shown only to the
//! owner, on their own organisms).
//!
//! Two things go out with each own organism ([`crate::protocol::OwnOrganism`]),
//! never with anyone else's ([`crate::protocol::NearbyOrganism`] has neither):
//!
//! - [`heading`]: the facing (`Organism::heading`, the last movement)
//!   quantized to [`HEADINGS`] directions, a byte that changes only when the
//!   animal turns.
//! - [`VisionView`]: the static eye geometry (eye count, first pair's
//!   azimuth, field width, snout occlusion) and the sim's derived numbers
//!   (coverage, depth field, blind spots), in whole degrees. It is fixed at
//!   birth, so protocol 3 sends each distinct one once per connection (a
//!   palette, like `look`) and an organism refers to it by index.
//!
//! The geometry is read from the sim (`evo_sim::vision`) so the cones show
//! what the detection model uses, whether or not `Balance::directional_vision`
//! is on (`directional` says which).

use evo_sim::organism::Organism;
use evo_sim::vision::eye_layout;
use serde::{Deserialize, Serialize};

/// Directions a heading is quantized to: 11.25° each.
pub const HEADINGS: u32 = 32;

/// The facing of `o` as one of [`HEADINGS`] directions: `k` is the angle
/// `k · 360° / HEADINGS` from the +x axis toward +y (map coordinates: east,
/// then south). `None` without a heading (a zero vector).
pub fn heading(o: &Organism) -> Option<u8> {
    let h = o.heading;
    if !(h.x.is_finite() && h.y.is_finite()) || h.x * h.x + h.y * h.y < 1e-12 {
        return None;
    }
    let step = std::f32::consts::TAU / HEADINGS as f32;
    let k = (h.y.atan2(h.x) / step).round() as i32;
    Some(k.rem_euclid(HEADINGS as i32) as u8)
}

/// One organism's eyes, compact and fixed at birth. Angles are whole
/// degrees on the head: 0 straight ahead, +90 right.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct VisionView {
    /// 1–6. An odd count puts one eye on the midline; pair `k` sits at
    /// ±min(170°, `azimuth` + 50°·k).
    pub eyes: u8,
    /// The first pair's azimuth, 0–180.
    pub azimuth: u8,
    /// Each eye's field width (`140 + 60 × eye size`).
    pub field: u8,
    /// Degrees of the binocular field a long snout hides.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub occlusion: u8,
    /// Share of the full circle seen by at least one eye, whole percent.
    pub coverage: u8,
    /// Depth field: degrees within ±60° of straight ahead seen by two or
    /// more eyes, less the snout's occlusion.
    pub binocular: u16,
    /// The unseen arc straight behind.
    pub blind: u16,
    /// The unseen arc straight ahead (eyes set far to the sides).
    #[serde(default, skip_serializing_if = "is_zero16")]
    pub front_gap: u16,
    /// `Balance::directional_vision` is on in this world: blind sectors
    /// shorten detection and depth sharpens a hunter's strike. Off (absent),
    /// the cones only describe the eyes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub directional: bool,
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

fn is_zero16(v: &u16) -> bool {
    *v == 0
}

fn deg8(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

fn deg16(v: f32) -> u16 {
    v.round().clamp(0.0, 360.0) as u16
}

impl VisionView {
    /// The eyes of `o`, in a world whose `directional_vision` is `k`.
    pub fn of(o: &Organism, k: f32) -> Self {
        let eyes = eye_layout(&o.morph);
        let v = &o.traits.vision;
        VisionView {
            eyes: eyes.count.clamp(1, 6) as u8,
            azimuth: deg8(eyes.azimuth.clamp(0.0, 180.0)),
            field: deg8(eyes.field()),
            occlusion: deg8(eyes.occlusion()),
            coverage: deg8(v.coverage * 100.0).min(100),
            binocular: deg16(v.binocular),
            blind: deg16(v.blind),
            front_gap: deg16(v.front_gap),
            directional: k > 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evo_sim::math::Vec2;

    fn org() -> Organism {
        let world = crate::game::build_world(&crate::game::WorldSettings {
            seed: 3,
            width: 48,
            height: 48,
            wild: true,
            ..Default::default()
        });
        world.organisms.iter().find(|o| o.alive).unwrap().clone()
    }

    #[test]
    fn headings_quantize_to_32_directions() {
        let mut o = org();
        for (x, y, k) in
            [(1.0, 0.0, 0), (0.0, 1.0, 8), (-1.0, 0.0, 16), (0.0, -1.0, 24), (1.0, -0.01, 0), (0.7, 0.7, 4)]
        {
            o.heading = Vec2::new(x, y);
            assert_eq!(heading(&o), Some(k), "({x}, {y})");
        }
        o.heading = Vec2::ZERO;
        assert_eq!(heading(&o), None);
    }

    #[test]
    fn default_eyes_match_the_contract() {
        let o = org();
        let v = VisionView::of(&o, 0.0);
        let e = o.morph.eye_size;
        // Two eyes at ±a (founders vary a little around the default 60°),
        // field 140 + 60e; the snout hides 40·max(0, s − 0.5) of the overlap.
        let a = o.morph.eye_azimuth;
        let occ = 40.0 * (o.morph.snout_length - 0.5).max(0.0);
        assert_eq!((v.eyes, v.azimuth, v.front_gap), (2, a.round() as u8, 0));
        assert_eq!(v.occlusion, occ.round() as u8);
        assert_eq!(v.field, (140.0 + 60.0 * e).round() as u8);
        // Depth: the two fields overlap by field − 2a in front, less the snout.
        let depth = (140.0 + 60.0 * e - 2.0 * a - occ).max(0.0);
        let blind = (360.0 - 2.0 * a - (140.0 + 60.0 * e)).max(0.0);
        assert!((v.binocular as f32 - depth).abs() <= 1.0, "{v:?} vs {depth}");
        assert!((v.blind as f32 - blind).abs() <= 1.0, "{v:?} vs {blind}");
        assert!(!v.directional && VisionView::of(&o, 0.5).directional);
        let json = serde_json::to_value(v).unwrap();
        assert!(json.get("directional").is_none() && json.get("front_gap").is_none(), "{json}");
    }
}

//! What a migration order is told about the trip and the destination
//! beyond the sim's own checks (playtest 8): a group about to bed down,
//! rival ground at the destination, water that won't last a drought, the
//! journey's food shared with the other plant eaters along the way and
//! the crowd at the destination, and where founders landed relative to
//! the nearest rival.
//!
//! These read the world through its public state only (the sim's world
//! code is left alone): the estimates mirror the sim's rules
//! (`World::dormancy_wanted`, `World::water_access`, `World::food_check`)
//! closely enough to warn, not to the decimal.

use std::collections::BTreeSet;

use evo_sim::math::Vec2;
use evo_sim::organism::Dormancy;
use evo_sim::organism::OrgId;
use evo_sim::terrain::{Map, Season, DROUGHT_WETNESS};
use evo_sim::world::groups::{JOURNEY_CORRIDOR, WATER_REACH};
use evo_sim::{PlayerId, WILD};

use super::states::OUTLINES_NEAR_GROUP;
use super::Game;

/// Dryness past which an estivator sleeps (the sim's `ESTIVATION_DRYNESS`).
const ESTIVATION_DRYNESS: f32 = 1.0 - DROUGHT_WETNESS;
/// Ground water of a lake's deep part, which lasts a drought (the sim's
/// `LASTING_WATER`).
const LASTING_WATER: f32 = 1.0;
/// Days ahead a migration order looks for a group bedding down: a trip
/// ordered days before the group sleeps stalls on the way (playtest 8:
/// ended "94% dormant" five days after the order).
pub(super) const SLEEP_LOOKAHEAD_DAYS: u32 = 7;
/// Share of a group asleep or bedding down within the lookahead at which
/// an order warns.
pub(super) const SLEEPY_WARN_SHARE: f32 = 0.5;
/// Dryness at the destination past which only lasting water counts: the
/// drought line (0.5 is normal).
pub(super) const DRY_DESTINATION: f32 = ESTIVATION_DRYNESS;

/// A trip's food, shared (`Game::journey_food`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct JourneyFood {
    /// Standing plants along the way against what the group and the other
    /// plant eaters there eat in the trip's days.
    pub route: f32,
    /// What the destination feeds against the group's members (its
    /// crowding, others' grazing counted), when rated.
    pub destination: Option<f32>,
    /// Other plant eaters along the way.
    pub others: usize,
}

impl JourneyFood {
    /// The share the trip is rated by: the worse of the way and the arrival.
    pub fn share(&self) -> f32 {
        self.destination.map_or(self.route, |d| self.route.min(d))
    }
}

impl Game {
    /// Share of `player`'s `group` that is in deep dormancy or would bed down
    /// within `days` by the season, the seasonal temperature where it stands
    /// and today's dryness (hunger and thirst left out: a hungry sleeper
    /// beds down once fed).
    pub(super) fn sleepy_share(&self, player: PlayerId, group: u8, days: u32) -> f32 {
        let w = &self.world;
        let bal = &w.config.balance;
        let dpy = w.config.days_per_year.max(1);
        let doy = w.day_of_year();
        let ahead: Vec<(Season, f32)> = (0..=days)
            .map(|d| {
                let day = (doy + d) % dpy;
                (Season::from_day(day, dpy), Map::seasonal_offset(day, dpy) + w.map.climate_temp)
            })
            .collect();
        let (mut n, mut sleepy) = (0usize, 0usize);
        for o in w.organisms.iter().filter(|o| o.alive && o.owner == player && o.group == group) {
            n += 1;
            if matches!(o.dormancy, Dormancy::Winter | Dormancy::Summer) {
                sleepy += 1;
                continue;
            }
            let s = o.traits.seasonal;
            if s.winter.is_none() && s.estivation <= 0.0 {
                continue;
            }
            let base = w.map.ground_at(o.pos).base_temp;
            let dry = w.map.dryness_at(o.pos);
            let beds = ahead.iter().any(|&(season, off)| {
                let temp = base + off;
                (s.winter.is_some() && matches!(season, Season::Autumn | Season::Winter) && temp < bal.dormancy_temp)
                    || (s.estivation > 0.0
                        && season == Season::Summer
                        && (temp > bal.estivation_temp || dry > ESTIVATION_DRYNESS))
                    || (s.estivation > 0.0
                        && bal.founding_rivals > 0.0
                        && season != Season::Winter
                        && dry > ESTIVATION_DRYNESS)
            });
            sleepy += usize::from(beds);
        }
        if n == 0 {
            0.0
        } else {
            sleepy as f32 / n as f32
        }
    }

    /// Drinking spots in or within [`WATER_REACH`] of the circle beside a
    /// lake's deep water, which lasts a drought: streams and shallows dry
    /// up (playtest 8: 256 spots at 96% dryness, none left days later).
    pub(super) fn lasting_water(&self, center: Vec2, radius: f32) -> usize {
        let map = &self.world.map;
        let ground = map.ground();
        let (w, h) = (map.width as i32, map.height as i32);
        let reach = radius + WATER_REACH;
        let (cx, cy) = (center.x.floor() as i32, center.y.floor() as i32);
        let span = reach.ceil() as i32 + 1;
        let lake = |x: i32, y: i32| {
            x >= 0 && y >= 0 && x < w && y < h && {
                let i = map.idx(x as usize, y as usize);
                map.tiles[i].has_drinkable_water() && ground[i].base_water >= LASTING_WATER
            }
        };
        let mut n = 0;
        for y in (cy - span).max(0)..=(cy + span).min(h - 1) {
            for x in (cx - span).max(0)..=(cx + span).min(w - 1) {
                let c = Map::tile_center(x as usize, y as usize);
                if c.dist(center) > reach || !map.can_drink_at(c) {
                    continue;
                }
                if (y - 1..=y + 1).any(|ny| (x - 1..=x + 1).any(|nx| lake(nx, ny))) {
                    n += 1;
                }
            }
        }
        n
    }

    /// The trip's food along `route` (tiles) over `days`: the standing
    /// plants of the corridor the sim's journey check reads, shared with
    /// the other plant eaters standing in it (each eating about what one of
    /// the group's members does), and the destination's supports against
    /// the group (`supports`, from its food check). Playtest 8: the plants
    /// counted for the group alone promised 2.02, and 194 arrived 128.
    pub(super) fn journey_food(
        &self,
        player: PlayerId,
        group: u8,
        route: &[Vec2],
        days: f32,
        supports: Option<usize>,
    ) -> Option<JourneyFood> {
        let (members, need) = self.world.plant_need(player, group)?;
        let map = &self.world.map;
        let mut seen = BTreeSet::new();
        let r = JOURNEY_CORRIDOR as i32;
        for pair in route.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let steps = a.dist(b).ceil().max(1.0) as usize;
            for k in 0..=steps {
                let p = a + (b - a) * (k as f32 / steps as f32);
                let (x, y) = map.tile_coords(p);
                for dy in -r..=r {
                    for dx in -r..=r {
                        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                        if nx >= 0 && ny >= 0 && (nx as usize) < map.width && (ny as usize) < map.height {
                            seen.insert(map.idx(nx as usize, ny as usize));
                        }
                    }
                }
            }
        }
        let standing: f32 = seen.iter().map(|&k| map.tiles[k].veg).sum();
        let others = self
            .world
            .organisms
            .iter()
            .filter(|o| o.alive && !(o.owner == player && o.group == group) && o.traits.carnivory <= 0.6)
            .filter(|o| {
                let (x, y) = map.tile_coords(o.pos);
                seen.contains(&map.idx(x, y))
            })
            .count();
        let per_animal = need / members.max(1) as f32;
        let eaten = (need + others as f32 * per_animal) * days.max(1.0);
        let route_share = if eaten > 0.0 { (standing / eaten).min(10.0) } else { 1.0 };
        let destination = supports.map(|s| (s as f32 / members.max(1) as f32).min(10.0));
        Some(JourneyFood { route: route_share, destination, others })
    }

    /// Rival territories the player can see (within [`OUTLINES_NEAR_GROUP`]
    /// of one of their groups, as the state shows them) that `at`'s circle
    /// of `radius` reaches into: (owner, centre, radius, inside).
    pub(super) fn rival_ground_at(&self, player: PlayerId, at: Vec2, radius: f32) -> Vec<(PlayerId, Vec2, f32, bool)> {
        let middles: Vec<Vec2> =
            self.world.groups(player).iter().filter_map(|g| self.world.group_middle(player, g.id)).collect();
        let mut v: Vec<(PlayerId, Vec2, f32, bool)> = self
            .world
            .all_territories()
            .filter(|&(p, _, t)| {
                p != player
                    && middles.iter().any(|m| m.dist(t.center) <= OUTLINES_NEAR_GROUP + t.radius)
                    && t.center.dist(at) < t.radius + radius
            })
            .map(|(p, _, t)| (p, t.center, t.radius, t.center.dist(at) <= t.radius))
            .collect();
        v.sort_by(|a, b| b.3.cmp(&a.3).then(a.1.dist(at).total_cmp(&b.1.dist(at))));
        v
    }

    /// Where `founders` landed and how far the nearest rival is (other
    /// players' animals and territories; wild animals aside), in words, for
    /// the founding reply and alert (playtest 8: nothing told a late joiner
    /// a rival herd was beside them). None without founders.
    pub(super) fn landing_text(&self, player: PlayerId, founders: &[OrgId]) -> Option<String> {
        let pos: Vec<Vec2> = founders.iter().filter_map(|&id| self.world.organism(id)).map(|o| o.pos).collect();
        if pos.is_empty() {
            return None;
        }
        let mid = pos.iter().fold(Vec2::ZERO, |a, &p| a + p) * (1.0 / pos.len() as f32);
        // The nearest rival animal (to any founder) and its owner.
        let mut near: Option<(f32, PlayerId)> = None;
        for o in self.world.organisms.iter().filter(|o| o.alive && o.owner != player && o.owner != WILD) {
            let d = pos.iter().map(|p| p.dist(o.pos)).fold(f32::INFINITY, f32::min);
            if near.is_none_or(|(b, _)| d < b) {
                near = Some((d, o.owner));
            }
        }
        // The nearest rival territory's edge.
        let mut land: Option<(f32, PlayerId)> = None;
        for (p, _, t) in self.world.all_territories().filter(|&(p, _, _)| p != player) {
            let d = (t.center.dist(mid) - t.radius).max(0.0);
            if land.is_none_or(|(b, _)| d < b) {
                land = Some((d, p));
            }
        }
        let at = format!("Your founders landed around ({:.0}, {:.0})", mid.x, mid.y);
        let animal =
            near.map(|(d, p)| format!("the nearest rival animal ({}'s) is {} away", self.name(p), super::tiles(d)));
        let ground = land.map(|(d, p)| {
            if d <= 0.0 {
                format!("they are on {}'s territory", self.name(p))
            } else {
                format!("{}'s territory is {} away", self.name(p), super::tiles(d))
            }
        });
        Some(match (animal, ground) {
            (None, None) => format!("{at}: no other player's animals or territories on the map yet."),
            (Some(a), None) => format!("{at}: {a}."),
            (None, Some(g)) => format!("{at}: {g}."),
            (Some(a), Some(g)) => format!("{at}: {a}; {g}."),
        })
    }
}

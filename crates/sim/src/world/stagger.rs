//! Staggered decisions (`Balance::decide_interval`, off by default): a calm
//! organism keeps its last intent for a few ticks instead of deciding
//! afresh every tick. An approximation for speed: perception and choice
//! are most of a tick's cost in a crowd.

use super::{Intent, World};
use crate::organism::Action;

impl World {
    /// The intent organism `i` keeps this tick instead of deciding, if
    /// staggered decisions are on and nothing calls for a fresh decision:
    /// its turn to decide comes every `decide_interval` ticks (by id); it
    /// decides on the day's first tick (indices change when the dead are
    /// removed), when possessed, if it is a hunter, after seeing a threat, while fleeing,
    /// hunting, courting, provisioning, defending or following an order,
    /// and when its target organism has died. A kept intent makes no
    /// one-off changes (wander or forage targets, alerts) and follows its
    /// target organism to where it stands now.
    pub(super) fn reused_intent(&self, i: usize) -> Option<Intent> {
        let k = self.config.balance.decide_interval;
        if k < 2.0 || self.tick.is_multiple_of(self.config.ticks_per_day as u64) {
            return None;
        }
        let o = &self.organisms[i];
        // Hunters (few, and their chases and pack hunts need a decision
        // every tick) always decide.
        let hunter = o.traits.carnivory > super::decide::HUNTER_CARNIVORY;
        if o.id.wrapping_add(self.tick).is_multiple_of(k as u64) || o.possessed_by.is_some() || hunter {
            return None;
        }
        let &(id, last, target) = self.last_intents.get(i)?;
        use Action::*;
        let active = matches!(last.action, Flee | Hunt | SeekMate | Scavenge | Defend | Ordered);
        if id != o.id || last.threat > 0.0 || active {
            return None;
        }
        let mut intent = Intent { new_wander: None, new_forage: None, alert: None, ..last };
        if let Some(j) = last.target_org {
            let q = self.organisms.get(j).filter(|q| q.alive && Some(q.id) == target)?;
            intent.target = Some(q.pos);
        }
        Some(intent)
    }

    /// Keep this tick's intents for [`Self::reused_intent`] (only with
    /// staggered decisions on).
    pub(super) fn remember_intents(&mut self, intents: &[Intent]) {
        self.last_intents.clear();
        if self.config.balance.decide_interval < 2.0 {
            return;
        }
        let orgs = &self.organisms;
        self.last_intents
            .extend(intents.iter().enumerate().map(|(i, it)| (orgs[i].id, *it, it.target_org.map(|j| orgs[j].id))));
    }
}

#[cfg(test)]
mod tests {
    use crate::{presets, WorldConfig};

    fn run(threads: usize, interval: f32) -> Vec<(u64, u32, u32)> {
        let mut config = WorldConfig { seed: 5, width: 64, height: 64, ..Default::default() };
        config.balance.decide_interval = interval;
        let mut w = presets::standard_world(config, 1);
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
        pool.install(|| w.run_days(3));
        w.organisms.iter().filter(|o| o.alive).map(|o| (o.id, o.pos.x.to_bits(), o.pos.y.to_bits())).collect()
    }

    /// Staggered decisions stay reproducible whatever the thread count, and
    /// change the world (organisms do keep intents).
    #[test]
    fn staggered_decisions_are_deterministic() {
        let a = run(1, 2.0);
        assert_eq!(a, run(4, 2.0));
        assert_ne!(a, run(1, 0.0));
    }
}

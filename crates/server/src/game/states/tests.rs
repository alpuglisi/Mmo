//! The cached state build against the one it replaced.

use std::collections::HashMap;

use evo_sim::{BirthType, FounderDesign, Thermo};
use rand::{Rng, SeedableRng};
use serde_json::json;

use evo_sim::world::commands::MEETING_DISTANCE;

use super::super::{build_world, GameConfig, OrgId, WorldSettings};
use super::*;
use crate::wire::DeltaEncoder;

/// State building as it was before [`StateCache`], verbatim but for `self`
/// becoming `g`: a fresh index at every push and sub-step, every view made
/// from its organism, the leaderboard cut by a scan.
mod old {
    use super::*;

    struct StateIndex {
        by_owner: BTreeMap<PlayerId, Vec<u32>>,
        cols: usize,
        rows: usize,
        starts: Vec<u32>,
        items: Vec<u32>,
        offers: BTreeMap<PlayerId, Vec<u64>>,
        bids: BTreeMap<PlayerId, u64>,
        clock: ClockView,
        weather: WeatherView,
        leaderboard: Leaderboard,
    }

    impl StateIndex {
        fn new(g: &Game) -> Self {
            let w = &g.world;
            let cols = (w.map.width as f32 / STATE_CELL).ceil().max(1.0) as usize;
            let rows = (w.map.height as f32 / STATE_CELL).ceil().max(1.0) as usize;
            let cell_of = |p: Vec2| {
                let cx = ((p.x / STATE_CELL).floor().max(0.0) as usize).min(cols - 1);
                let cy = ((p.y / STATE_CELL).floor().max(0.0) as usize).min(rows - 1);
                cy * cols + cx
            };
            let mut by_owner: BTreeMap<PlayerId, Vec<u32>> = BTreeMap::new();
            let mut starts = vec![0u32; cols * rows + 1];
            let mut cell = vec![u32::MAX; w.organisms.len()];
            for (i, o) in w.organisms.iter().enumerate() {
                if !o.alive {
                    continue;
                }
                if o.owner != WILD {
                    by_owner.entry(o.owner).or_default().push(i as u32);
                }
                let k = cell_of(o.pos);
                cell[i] = k as u32;
                starts[k + 1] += 1;
            }
            for k in 1..starts.len() {
                starts[k] += starts[k - 1];
            }
            let mut fill = starts.clone();
            let mut items = vec![0u32; starts[cols * rows] as usize];
            for (i, &k) in cell.iter().enumerate() {
                if k != u32::MAX {
                    items[fill[k as usize] as usize] = i as u32;
                    fill[k as usize] += 1;
                }
            }
            let mut offers: BTreeMap<PlayerId, Vec<u64>> = BTreeMap::new();
            for o in g.offers.values().filter(|o| o.status.is_open()) {
                offers.entry(o.from).or_default().push(o.id);
                if o.to != o.from {
                    offers.entry(o.to).or_default().push(o.id);
                }
            }
            let mut bids: BTreeMap<PlayerId, u64> = BTreeMap::new();
            for b in g.economy.listings.values().filter_map(|l| l.auction.as_ref()?.high.as_ref()) {
                *bids.entry(b.bidder).or_default() += b.amount;
            }
            StateIndex {
                by_owner,
                cols,
                rows,
                starts,
                items,
                offers,
                bids,
                clock: g.clock(),
                weather: g.weather(),
                leaderboard: g.leaderboard(),
            }
        }

        fn owned(&self, player: PlayerId) -> &[u32] {
            self.by_owner.get(&player).map_or(&[], Vec::as_slice)
        }

        fn in_window(&self, v: &ViewWindow) -> Vec<u32> {
            if !(v.x.is_finite() && v.y.is_finite() && v.radius.is_finite()) {
                return Vec::new();
            }
            let span = |lo: f32, hi: f32, n: usize| {
                let a = ((lo / STATE_CELL).floor().max(0.0) as usize).min(n - 1);
                let b = ((hi / STATE_CELL).floor().max(0.0) as usize).min(n - 1);
                a..=b
            };
            let mut out = Vec::new();
            for cy in span(v.y - v.radius, v.y + v.radius, self.rows) {
                for cx in span(v.x - v.radius, v.x + v.radius, self.cols) {
                    let k = cy * self.cols + cx;
                    out.extend_from_slice(&self.items[self.starts[k] as usize..self.starts[k + 1] as usize]);
                }
            }
            out
        }
    }

    fn state_with(g: &Game, conn: ConnId, idx: &StateIndex) -> Option<StateView> {
        let c = g.conns.get(&conn)?;
        let player = c.player;
        let tpd = g.world.config.ticks_per_day;
        let orgs = &g.world.organisms;
        let mine = idx.owned(player);
        let own: Vec<OwnOrganism> = mine.iter().map(|&i| g.own_view(&orgs[i as usize], tpd)).collect();
        let view = c.view.unwrap_or_else(|| {
            let possessed = mine.iter().map(|&i| &orgs[i as usize]).find(|o| o.possessed_by == Some(player));
            let center = possessed.map(|o| o.pos).unwrap_or_else(|| g.lineage_center_of(player, mine));
            ViewWindow { x: center.x, y: center.y, radius: g.cfg.default_view_radius }
        });
        let center = Vec2::new(view.x, view.y);
        let mut cand = idx.in_window(&view);
        cand.sort_unstable();
        let mut nearby: Vec<(f32, u32)> = cand
            .into_iter()
            .filter(|&i| {
                let o = &orgs[i as usize];
                o.owner != player && (o.pos.x - view.x).abs() <= view.radius && (o.pos.y - view.y).abs() <= view.radius
            })
            .map(|i| (orgs[i as usize].pos.dist(center), i))
            .collect();
        if nearby.len() > g.cfg.max_nearby {
            nearby.sort_by(|a, b| a.0.total_cmp(&b.0));
            nearby.truncate(g.cfg.max_nearby);
        }
        let nearby: Vec<NearbyOrganism> =
            nearby.into_iter().map(|(_, i)| NearbyOrganism::from_org(&orgs[i as usize])).collect();
        let ids = idx.offers.get(&player).map_or(&[][..], Vec::as_slice);
        let offers = ids.iter().map(|&id| g.offer_view(id)).collect();
        let escrow = ids.iter().map(|id| &g.offers[id]).filter(|o| o.from == player).map(|o| o.escrow).sum::<u64>()
            + idx.bids.get(&player).copied().unwrap_or(0);
        let leaderboard = state_leaderboard(g, &idx.leaderboard, player, &nearby);
        Some(StateView {
            clock: idx.clock.clone(),
            weather: idx.weather.clone(),
            player,
            lineage: g.lineage_of(player),
            slots_left: g.world.mating_slots_left(player),
            population: g.owner_population(player),
            migration: g.world.migration_order(player, 0).map(|p| [p.x, p.y]),
            groups: g
                .world
                .groups_of(player, mine.iter().map(|&i| &orgs[i as usize]))
                .iter()
                .map(GroupView::from)
                .collect(),
            view,
            organisms: own,
            nearby,
            leaderboard,
            offers,
            balance: g.balance(player),
            escrow,
        })
    }

    fn state_leaderboard(g: &Game, full: &Leaderboard, player: PlayerId, nearby: &[NearbyOrganism]) -> Leaderboard {
        let top = g.cfg.state_leaderboard_rows;
        if full.rows.len() <= top {
            return full.clone();
        }
        let mut owners: Vec<PlayerId> = nearby.iter().map(|n| n.owner).collect();
        owners.sort_unstable();
        owners.dedup();
        let rows = full
            .rows
            .iter()
            .enumerate()
            .filter(|(rank, r)| *rank < top || r.player == player || owners.binary_search(&r.player).is_ok())
            .map(|(_, r)| r.clone())
            .collect();
        Leaderboard { day: full.day, breeding_adults: full.breeding_adults, rows }
    }

    pub fn states(g: &Game) -> Outbox {
        if g.conns.is_empty() {
            return Vec::new();
        }
        let idx = StateIndex::new(g);
        g.conns
            .keys()
            .filter_map(|&c| state_with(g, c, &idx).map(|s| (Target::Conn(c), ServerMsg::State(Box::new(s)))))
            .collect()
    }

    pub fn substep(g: &mut Game) -> Outbox {
        let moved = g.world.substep();
        if moved.is_empty() {
            return Vec::new();
        }
        let players: BTreeSet<PlayerId> =
            moved.iter().filter_map(|&id| g.world.organism(id).and_then(|o| o.possessed_by)).collect();
        if !g.conns.values().any(|c| players.contains(&c.player)) {
            return Vec::new();
        }
        let idx = StateIndex::new(g);
        g.conns
            .iter()
            .filter(|(_, c)| players.contains(&c.player))
            .filter_map(|(&c, _)| state_with(g, c, &idx).map(|s| (Target::Conn(c), ServerMsg::State(Box::new(s)))))
            .collect()
    }
}

const PLAYERS: u32 = 16;

fn design(name: &str, warm: bool) -> FounderDesign {
    let thermo = if warm { Thermo::Warm } else { Thermo::Cold };
    FounderDesign::new(name, thermo, BirthType::Live, 2).founders(10)
}

/// A busy world: the wild ecosystem and sixteen players, each connected.
fn busy(cfg: GameConfig) -> Game {
    let world = build_world(&WorldSettings { seed: 11, width: 96, height: 96, wild: true, ..Default::default() });
    let mut g = Game::new(world, cfg);
    for p in 1..=PLAYERS {
        g.set_name(p, &format!("p{p}"));
        g.found(p, &design(&format!("L{p}"), p % 2 == 0)).unwrap();
        g.connect(p as u64 * 10, p, &format!("p{p}"));
    }
    g.world.run_days(1);
    let _ = g.after_tick();
    g
}

fn owned(g: &Game, p: PlayerId) -> Vec<OrgId> {
    g.world.organisms.iter().filter(|o| o.alive && o.owner == p).map(|o| o.id).collect()
}

fn pick(rng: &mut impl Rng, v: &[OrgId]) -> Option<OrgId> {
    (!v.is_empty()).then(|| v[rng.gen_range(0..v.len())])
}

/// A random command for player `p`, as players send them.
fn command(g: &Game, p: PlayerId, rng: &mut impl Rng) -> Option<ClientMsg> {
    let mine = owned(g, p);
    let possessed = g.world.organisms.iter().find(|o| o.alive && o.possessed_by == Some(p)).map(|o| (o.id, o.pos));
    let near = |at: Vec2| {
        (g.world.organisms.iter())
            .filter(|o| o.alive && o.owner != p && o.owner != WILD && o.pos.dist(at) < 30.0)
            .map(|o| o.id)
            .collect::<Vec<_>>()
    };
    let foreign = possessed.map_or_else(Vec::new, |(_, at)| near(at));
    let offers: Vec<u64> = g.offers.values().filter(|o| o.from == p || o.to == p).map(|o| o.id).collect();
    let listings: Vec<(u64, bool)> =
        g.economy.listings.values().filter(|l| l.owner != p).map(|l| (l.id, l.auction.is_some())).collect();
    let (x, y) = (rng.gen_range(-4.0..100.0f32), rng.gen_range(-4.0..100.0f32));
    let v = match rng.gen_range(0..24) {
        0..=2 => json!({"type": "set_view", "x": x, "y": y, "radius": rng.gen_range(4.0..40.0f32)}),
        3 => json!({"type": "clear_view"}),
        4..=8 => match possessed {
            Some((id, at)) => json!({"type": "order_move", "organism": id,
                "x": at.x + rng.gen_range(-15.0..15.0f32), "y": at.y + rng.gen_range(-15.0..15.0f32)}),
            None => json!({"type": "possess", "organism": pick(rng, &mine)?}),
        },
        9 => json!({"type": "possess", "organism": pick(rng, &mine)?}),
        10 => json!({"type": "release", "organism": possessed?.0}),
        // An own pair close enough to mate at once, else any.
        11 => {
            let adult = |id: &OrgId| g.world.organism(*id).filter(|o| o.matured && o.pregnancy.is_none());
            let pair = mine.iter().filter_map(&adult).find_map(|a| {
                let close = |b: &&Organism| b.sex != a.sex && b.pos.dist(a.pos) <= MEETING_DISTANCE;
                mine.iter().filter_map(&adult).find(close).map(|b| (a.id, b.id))
            });
            let (a, b) = match pair {
                Some(pair) => pair,
                None => (pick(rng, &mine)?, pick(rng, &mine)?),
            };
            json!({"type": "request_mating", "mine": a, "partner": b})
        }
        12 => json!({"type": "request_mating", "mine": pick(rng, &mine)?, "partner": pick(rng, &foreign)?,
            "price": rng.gen_bool(0.5).then_some(3)}),
        13 => {
            let id = *offers.get(rng.gen_range(0..offers.len().max(1)))?;
            match rng.gen_range(0..3) {
                0 => json!({"type": "respond_mating", "request": id, "accept": rng.gen_bool(0.8)}),
                1 => json!({"type": "complete_mating", "request": id}),
                _ => json!({"type": "cancel_mating", "request": id}),
            }
        }
        14 => json!({"type": "create_listing", "organism": pick(rng, &mine)?, "fee": rng.gen_range(0..5)}),
        15 => json!({"type": "create_auction", "organism": pick(rng, &mine)?, "reserve": 1, "ticks": 3}),
        16 => {
            let (l, auction) = *listings.get(rng.gen_range(0..listings.len().max(1)))?;
            let m = pick(rng, &mine)?;
            if auction {
                json!({"type": "bid", "listing": l, "amount": rng.gen_range(1..6), "mine": m})
            } else {
                json!({"type": "buy_listing", "listing": l, "mine": m})
            }
        }
        17 => json!({"type": "migrate", "x": x, "y": y}),
        18 => json!({"type": "clear_migration"}),
        19 => json!({"type": "split_group", "organisms": [pick(rng, &mine)?, pick(rng, &mine)?]}),
        20 => json!({"type": "merge_groups", "from": 1, "into": 0}),
        21 => json!({"type": "rally", "x": x, "y": y}),
        22 => json!({"type": "claim_territory", "x": x, "y": y, "radius": 6.0}),
        _ => match rng.gen_range(0..3) {
            0 => json!({"type": "browse_market"}),
            1 => json!({"type": "ledger"}),
            _ => json!({"type": "preview_mating", "mine": pick(rng, &mine)?, "partner": pick(rng, &foreign)?}),
        },
    };
    Some(serde_json::from_value(v).expect("a valid command"))
}

/// Per state, the bytes its client gets: the JSON message (protocol 1) and
/// the protocol 2 frame from that connection's delta encoder.
fn wire(out: &Outbox, enc: &mut HashMap<ConnId, DeltaEncoder>) -> Vec<(ConnId, Vec<u8>, Vec<u8>)> {
    out.iter()
        .map(|(t, m)| {
            let (Target::Conn(c), ServerMsg::State(s)) = (t, m) else { panic!("not a state: {m:?}") };
            let e = enc.entry(*c).or_default();
            let keyframe = e.seq().is_multiple_of(5);
            (*c, serde_json::to_vec(m).unwrap(), e.encode(s, keyframe))
        })
        .collect()
}

/// Apply a command to both games; their replies must agree.
fn both(new: &mut Game, old: &mut Game, conn: ConnId, p: PlayerId, msg: ClientMsg) -> Outbox {
    let a = new.command(conn, p, None, msg.clone());
    let b = old.command(conn, p, None, msg);
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
    a
}

/// The cached build against the old one over a busy world: two games from
/// the same seed, every command applied to both. Between every two
/// sub-steps most players send one (views, possession and orders, matings
/// run at once, offers, the market, group orders); a player connects
/// twice, another founds a lineage mid-tick. Every state each client
/// receives (pushes and sub-steps, as JSON and as protocol 2 frames) is the
/// same byte for byte, with a whole and a cut leaderboard and a wide and a
/// tight `max_nearby`.
#[test]
fn cached_substep_states_match_the_old_build() {
    for (max_nearby, rows) in [(256, 50), (7, 4)] {
        let cfg = GameConfig { max_nearby, state_leaderboard_rows: rows, ..Default::default() };
        let (mut new, mut old) = (busy(cfg.clone()), busy(cfg));
        let mut rng = rand::rngs::StdRng::seed_from_u64(max_nearby as u64);
        let (mut enc_new, mut enc_old) = (HashMap::new(), HashMap::new());
        for p in 1..=PLAYERS {
            let id = owned(&new, p)[0];
            both(&mut new, &mut old, p as u64 * 10, p, ClientMsg::Possess { organism: id });
        }
        let (mut sub_states, mut ok, mut matings) = (0, 0, 0);
        for round in 0..30u32 {
            let (a, b) = (new.states(), old::states(&old));
            assert!(!a.is_empty());
            assert_eq!(wire(&a, &mut enc_new), wire(&b, &mut enc_old), "push, round {round}");
            for sub in 0..3 {
                for p in 1..=PLAYERS {
                    if !rng.gen_bool(0.7) {
                        continue;
                    }
                    let Some(msg) = command(&new, p, &mut rng) else { continue };
                    let before = new.world.counters.directed_matings;
                    let out = both(&mut new, &mut old, p as u64 * 10, p, msg);
                    ok += usize::from(matches!(&out[0].1, ServerMsg::Result { ok: true, .. }));
                    matings += new.world.counters.directed_matings - before;
                }
                if sub == 0 {
                    // One player switches to another organism mid-tick and
                    // walks it off: it moves in the sub-steps that follow.
                    let p = 1 + round % PLAYERS;
                    let conn = p as u64 * 10;
                    let held = new.world.organisms.iter().find(|o| o.alive && o.possessed_by == Some(p)).map(|o| o.id);
                    if let Some(id) = held {
                        both(&mut new, &mut old, conn, p, ClientMsg::Release { organism: id });
                    }
                    let mine = owned(&new, p);
                    if let Some(&id) = mine.iter().find(|&&id| Some(id) != held) {
                        both(&mut new, &mut old, conn, p, ClientMsg::Possess { organism: id });
                        let at = new.world.organism(id).unwrap().pos;
                        let msg = ClientMsg::OrderMove { organism: id, x: at.x + 20.0, y: at.y - 20.0 };
                        both(&mut new, &mut old, conn, p, msg);
                    }
                }
                if round == 4 && sub == 1 {
                    // A second connection, then a lineage founded mid-tick.
                    assert_eq!(new.connect(11, 1, "p1").len(), old.connect(11, 1, "p1").len());
                    let (late, d) = (PLAYERS + 1, design("late", true));
                    assert!(new.found(late, &d).is_ok() && old.found(late, &d).is_ok());
                    new.connect(170, late, "late");
                    old.connect(170, late, "late");
                    let id = owned(&new, late)[0];
                    both(&mut new, &mut old, 170, late, ClientMsg::Possess { organism: id });
                }
                if round == 8 && sub == 0 {
                    new.disconnect(11);
                    old.disconnect(11);
                }
                let (a, b) = (new.substep(), old::substep(&mut old));
                sub_states += a.len();
                assert_eq!(wire(&a, &mut enc_new), wire(&b, &mut enc_old), "sub-step {sub}, round {round}");
            }
            new.world.step();
            old.world.step();
            let _ = (new.after_tick(), old.after_tick());
            let pos = |g: &Game| g.world.organisms.iter().map(|o| (o.id, o.pos.x, o.pos.y)).collect::<Vec<_>>();
            assert_eq!(pos(&new), pos(&old));
        }
        assert!(sub_states > 200 && ok > 200 && matings > 0, "{sub_states} {ok} {matings}");
    }
}

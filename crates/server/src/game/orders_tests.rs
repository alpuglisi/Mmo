//! Group and possession orders through the protocol (playtest 1): a
//! migration without a group is the main group's, targets on water move
//! ashore and off-map ones are refused, the route and the arrival reach the
//! player, rallies last minutes of real time, and a lineage can be retired
//! (with the founding ramp keeping a re-founding from paying).

use evo_sim::design::{BirthType, FounderDesign, Thermo};
use evo_sim::math::Vec2;
use evo_sim::organism::OrgId;

use super::*;

fn design(name: &str) -> FounderDesign {
    FounderDesign::new(name, Thermo::Warm, BirthType::Live, 2).founders(6)
}

fn game_with(cfg: GameConfig) -> (Game, Vec<OrgId>) {
    let world = build_world(&WorldSettings { seed: 7, width: 64, height: 64, wild: false, ..Default::default() });
    let mut g = Game::new(world, cfg);
    g.set_name(1, "ann");
    let a = g.found(1, &design("A")).unwrap().founders;
    g.connect(10, 1, "ann");
    (g, a)
}

fn game() -> (Game, Vec<OrgId>) {
    game_with(GameConfig::default())
}

/// The command's result: `Ok(data)` or `Err(code)`.
fn run(g: &mut Game, msg: ClientMsg) -> Result<serde_json::Value, String> {
    let out = g.command(10, 1, Some(1), msg);
    match &out[0].1 {
        ServerMsg::Result { ok: true, data, .. } => Ok(data.clone().unwrap_or_default()),
        ServerMsg::Result { error: Some(e), .. } => Err(e.code.clone()),
        m => panic!("expected a result, got {m:?}"),
    }
}

fn alerts(g: &Game, kind: AlertKind) -> Vec<String> {
    g.alerts.get(&1).map_or(Vec::new(), |a| a.iter().filter(|x| x.kind == kind).map(|x| x.text.clone()).collect())
}

/// A dry, flat tile centre at least `min` tiles from `from`.
fn land_away(g: &Game, from: Vec2, min: f32) -> Vec2 {
    let m = &g.world.map;
    (0..m.height)
        .flat_map(|y| (0..m.width).map(move |x| evo_sim::terrain::Map::tile_center(x, y)))
        .find(|&p| {
            let t = m.tile_at(p);
            p.dist(from) >= min && t.water_depth == 0.0 && t.ice == 0.0 && m.ground_at(p).slope < 0.4
        })
        .expect("dry land")
}

fn tick(g: &mut Game) {
    g.world.step();
    g.after_tick();
}

/// A tile of deep water on the map (and its centre), if any.
fn deep_water(g: &Game) -> Option<Vec2> {
    let m = &g.world.map;
    (0..m.height).flat_map(|y| (0..m.width).map(move |x| (x, y))).find_map(|(x, y)| {
        let t = &m.tiles[m.idx(x, y)];
        (t.water_depth > 2.0 && t.ice == 0.0).then(|| evo_sim::terrain::Map::tile_center(x, y))
    })
}

#[test]
fn a_migration_without_a_group_is_the_main_groups_and_ends_on_arrival() {
    let (mut g, _) = game();
    let d = run(&mut g, ClientMsg::SplitGroup { group: 0, organisms: None }).unwrap();
    assert_eq!(d["group"], 1);
    let home = g.world.organisms.iter().find(|o| o.alive && o.owner == 1 && o.group == 0).unwrap().pos;
    // Somewhere a few tiles off that the main group can stand on.
    let target = (0..64)
        .map(|k| home + Vec2::new(8.0 * (k as f32 * 0.4).cos(), 8.0 * (k as f32 * 0.4).sin()))
        .map(|p| g.world.map.clamp(p))
        .find(|&p| g.world.map.tile_at(p).water_depth == 0.0 && g.world.map.ground_at(p).slope < 0.5)
        .unwrap();
    let d = run(&mut g, ClientMsg::Migrate { x: target.x, y: target.y, group: None }).unwrap();
    assert_eq!((d["group"].as_u64(), d.get("moved")), (Some(0), None), "{d}");
    assert_eq!(g.world.migration_order(1, 0), Some(target));
    assert_eq!(g.world.migration_order(1, 1), None, "group B is not ordered");
    // The planned route comes back, and is in the state for drawing.
    let route = d["route"].as_array().unwrap();
    assert!(route.len() >= 2, "{d}");
    let s = g.state_for(10).unwrap();
    let main = s.groups.iter().find(|v| v.id == 0).unwrap();
    assert_eq!(main.migration, Some([target.x, target.y]));
    assert_eq!(main.route.last(), Some(&[target.x, target.y]));
    assert!(s.groups.iter().find(|v| v.id == 1).unwrap().route.is_empty());
    // Walk there: the order ends and the player hears of it.
    for _ in 0..g.world.config.ticks_per_day * 3 {
        tick(&mut g);
        if g.world.migration_order(1, 0).is_none() {
            break;
        }
    }
    assert_eq!(g.world.migration_order(1, 0), None, "never arrived");
    let done = alerts(&g, AlertKind::Group);
    assert!(done.iter().any(|t| t.starts_with("Group A (main) reached its migration target")), "{done:?}");
    assert!(g.state_for(10).unwrap().groups.iter().all(|v| v.migration.is_none() && v.route.is_empty()));
}

#[test]
fn migration_and_rally_targets_are_checked() {
    let (mut g, _) = game();
    assert_eq!(run(&mut g, ClientMsg::Migrate { x: 99_999.0, y: -5.0, group: None }), Err("off_map".into()));
    assert_eq!(run(&mut g, ClientMsg::Rally { group: 0, x: 99_999.0, y: -5.0 }), Err("off_map".into()));
    if let Some(sea) = deep_water(&g) {
        match run(&mut g, ClientMsg::Migrate { x: sea.x, y: sea.y, group: Some(0) }) {
            Ok(d) => {
                assert_eq!(d["moved"], true, "{d}");
                let to = Vec2::new(d["x"].as_f64().unwrap() as f32, d["y"].as_f64().unwrap() as f32);
                assert!(g.world.map.tile_at(to).water_depth < 2.0, "moved ashore: {d}");
            }
            Err(code) => assert_eq!(code, "unreachable"),
        }
    }
    // A rally stands for at least the configured minutes of real time.
    let home = g.world.organisms.iter().find(|o| o.alive && o.owner == 1).unwrap().pos;
    let now = g.world.tick();
    let r: RallyView =
        serde_json::from_value(run(&mut g, ClientMsg::Rally { group: 0, x: home.x, y: home.y }).unwrap()).unwrap();
    assert!(r.expires_tick - now >= 300, "{r:?}");
    let (mut fast, _) = game_with(GameConfig { tick_ms: 100.0, rally_min_minutes: 5.0, ..Default::default() });
    let now = fast.world.tick();
    let r: RallyView =
        serde_json::from_value(run(&mut fast, ClientMsg::Rally { group: 0, x: home.x, y: home.y }).unwrap()).unwrap();
    assert_eq!(r.expires_tick - now, 3000.max(fast.world.rally_ticks()), "5 minutes at 100 ms a tick");
    assert!(fast.world.rally_ticks() >= 3000);
}

#[test]
fn moving_an_unpossessed_animal_says_to_possess_it() {
    let (mut g, a) = game();
    let msg = ClientMsg::OrderMove { organism: a[0], x: 3.0, y: 3.0 };
    assert_eq!(run(&mut g, msg.clone()), Err("not_possessed".into()));
    assert_eq!(run(&mut g, ClientMsg::Release { organism: a[0] }), Err("not_possessed".into()));
    run(&mut g, ClientMsg::Possess { organism: a[0] }).unwrap();
    let d = run(&mut g, msg).unwrap();
    assert!(d["x"].is_number() && d["y"].is_number(), "{d}");
}

#[test]
fn retiring_a_lineage_lets_the_player_start_over() {
    let (mut g, a) = game();
    let lineage = g.lineage_of(1).unwrap();
    run(&mut g, ClientMsg::Possess { organism: a[0] }).unwrap();
    run(&mut g, ClientMsg::Migrate { x: 30.0, y: 30.0, group: None }).unwrap();
    // Only the living lineage, named.
    assert_eq!(run(&mut g, ClientMsg::RetireLineage { lineage: lineage + 7 }), Err("no_lineage".into()));
    let d = run(&mut g, ClientMsg::RetireLineage { lineage }).unwrap();
    assert_eq!(d["removed"], a.len());
    assert!(g.is_extinct(1));
    assert_eq!(g.world.migration_order(1, 0), None);
    assert_eq!(run(&mut g, ClientMsg::RetireLineage { lineage }), Err("no_lineage".into()));
    tick(&mut g);
    // Removed, not dead: no death alerts, no deaths in the counters.
    assert!(alerts(&g, AlertKind::Death).is_empty(), "{:?}", alerts(&g, AlertKind::Death));
    assert!(g.world.counters.deaths_by_lineage.keys().all(|&(l, _)| l != lineage));
    let s = g.state_for(10).unwrap();
    assert!(s.organisms.is_empty());
    // And founds again at once.
    let again = g.found(1, &design("A2")).unwrap();
    assert_ne!(again.lineage, lineage);
    assert_eq!(g.founded_day.get(&1), Some(&g.world.day()));
}

#[test]
fn a_new_lineage_ramps_into_the_seasons_legacy_score() {
    assert_eq!(founding_ramp(None, 5), 1.0);
    assert_eq!(founding_ramp(Some(10), 10), 0.0);
    assert_eq!(founding_ramp(Some(10), 15), 0.5);
    assert_eq!(founding_ramp(Some(10), 10 + FOUNDING_RAMP_DAYS + 3), 1.0);
    let (mut g, _) = game();
    let today = 100;
    g.founded_day.insert(1, today);
    g.founded_day.insert(2, today - FOUNDING_RAMP_DAYS / 2);
    g.founded_day.insert(3, today - 2 * FOUNDING_RAMP_DAYS);
    let mut r = g.daily_record();
    r.day = today;
    r.legacy = [1, 2, 3]
        .map(|player| crate::store::LegacyEntry { player, share: 0.3, weighted_share: 0.2 })
        .into_iter()
        .collect();
    let ramped = g.ramped(&r);
    let share = |p: PlayerId| ramped.legacy.iter().find(|e| e.player == p).unwrap().share;
    // Founded today: nothing yet; a lineage halfway through counts half.
    assert_eq!(share(1), 0.0);
    assert!((share(2) - 0.15).abs() < 1e-9, "{}", share(2));
    assert_eq!(share(3), 0.3);
    // The record kept and shown is untouched.
    assert!(r.legacy.iter().all(|e| e.share == 0.3));
}

/// An attack order (`possess_attack`) through the protocol: refusals with
/// their codes and specifics, and the target's owner alerted once it goes
/// through.
/// Make `player`'s animals meat eaters: only they take attack orders.
fn hunters(g: &mut Game, player: PlayerId) {
    for o in g.world.organisms.iter_mut().filter(|o| o.owner == player) {
        o.traits.carnivory = 0.9;
    }
}

#[test]
fn an_attack_order_alerts_the_targets_owner() {
    let (mut g, a) = game();
    g.world.config.balance.attack_grace = 0.0;
    // bo joins a running world (a newcomer, with a founding grace).
    tick(&mut g);
    g.set_name(2, "bo");
    let b = g.found(2, &design("B")).unwrap().founders;
    g.connect(20, 2, "bo");
    // Bring one of bo's next to ann's first.
    let at = g.world.organism(a[0]).unwrap().pos;
    let i = g.world.organisms.iter().position(|o| o.id == b[0]).unwrap();
    g.world.organisms[i].pos = at + Vec2::new(1.0, 0.0);
    g.world.organisms[i].sheltered = false;
    g.world.organisms[i].growth = 0.3;
    let msg = ClientMsg::PossessAttack { organism: a[0], target: b[0] };
    assert_eq!(run(&mut g, msg.clone()), Err("not_possessed".into()));
    run(&mut g, ClientMsg::Possess { organism: a[0] }).unwrap();
    assert_eq!(run(&mut g, ClientMsg::PossessAttack { organism: a[0], target: a[1] }), Err("own_target".into()));
    // A plant eater doesn't hunt (playtest 6: a possessed grazer one-shot a fox).
    assert_eq!(run(&mut g, msg.clone()), Err("not_a_hunter".into()));
    hunters(&mut g, 1);
    // Not in its deep sleep (playtest 8: accepted for a hibernating animal).
    let k = g.world.organisms.iter().position(|o| o.id == a[0]).unwrap();
    g.world.organisms[k].dormancy = evo_sim::organism::Dormancy::Winter;
    assert_eq!(run(&mut g, msg.clone()), Err("asleep".into()));
    g.world.organisms[k].dormancy = evo_sim::organism::Dormancy::Awake;
    // A newcomer's grace, with the days left.
    g.world.config.balance.attack_grace = 1e6;
    // Shown: bo's lineage is listed as protected, and bo's stats give the days left.
    let protected = g.summary().protected;
    assert!(protected.iter().any(|p| p.player == 2 && p.days_left > 1.0), "{protected:?}");
    assert!(g.lineage_stats(2).grace_days_left.is_some_and(|d| d > 1.0));
    assert!(g.lineage_stats(2).ramp.is_some_and(|r| r.days == FOUNDING_RAMP_DAYS && r.weight < 1.0));
    let out = g.command(10, 1, Some(2), msg.clone());
    match &out[0].1 {
        ServerMsg::Result { error: Some(e), .. } if e.code == "newcomer_grace" => {
            assert!(e.detail.as_ref().is_some_and(|d| d["days_left"].as_f64().is_some_and(|x| x > 1.0)), "{e:?}");
        }
        m => panic!("expected newcomer_grace, got {m:?}"),
    }
    g.world.config.balance.attack_grace = 0.0;
    let d = run(&mut g, msg.clone()).unwrap();
    assert_eq!((d["organism"].as_u64(), d["target"].as_u64()), (Some(a[0]), Some(b[0])));
    assert_eq!(g.world.attack_target(a[0]), Some(b[0]));
    let told: Vec<String> = g
        .alerts
        .get(&2)
        .map_or(Vec::new(), |l| l.iter().filter(|x| x.kind == AlertKind::Attack).map(|x| x.text.clone()).collect());
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(alerts(&g, AlertKind::Attack).is_empty(), "the attacker isn't alerted");
    assert!(told[0].starts_with(&format!("#{} of your B is being hunted", b[0])), "{told:?}");
    // Repeating the order, or turning on another of bo's animals, doesn't
    // alert bo again within the window; after it, a new order does.
    let told = |g: &Game| g.alerts.get(&2).map_or(0, |l| l.iter().filter(|x| x.kind == AlertKind::Attack).count());
    run(&mut g, msg.clone()).unwrap();
    let j = g.world.organisms.iter().position(|o| o.id == b[1]).unwrap();
    g.world.organisms[j].pos = at + Vec2::new(0.0, 1.0);
    (g.world.organisms[j].sheltered, g.world.organisms[j].growth) = (false, 0.3);
    run(&mut g, ClientMsg::PossessAttack { organism: a[0], target: b[1] }).unwrap();
    assert_eq!(told(&g), 1);
    // A 3-tick window (3 s at 1 s a tick), run out with the order released.
    g.cfg.attack_alert_minutes = 0.05;
    run(&mut g, ClientMsg::Release { organism: a[0] }).unwrap();
    for _ in 0..3 {
        tick(&mut g);
    }
    run(&mut g, ClientMsg::Possess { organism: a[0] }).unwrap();
    run(&mut g, msg).unwrap();
    assert_eq!(told(&g), 2);
    // The target gets away: the attacker's player is told how it ended.
    let before = alerts(&g, AlertKind::Attack).len();
    g.world.organisms[i].pos = at + Vec2::new(40.0, 0.0);
    tick(&mut g);
    let ended = alerts(&g, AlertKind::Attack);
    assert_eq!(ended.len(), before + 1, "{ended:?}");
    assert!(ended.last().unwrap().contains("escaped") && ended.last().unwrap().contains("lost sight"), "{ended:?}");
    // A released attacker's order ends silently.
    g.world.organisms[i].pos = at + Vec2::new(1.0, 0.0);
    run(&mut g, ClientMsg::PossessAttack { organism: a[0], target: b[0] }).unwrap();
    run(&mut g, ClientMsg::Release { organism: a[0] }).unwrap();
    tick(&mut g);
    assert_eq!(alerts(&g, AlertKind::Attack).len(), before + 1);
}

#[test]
fn the_newcomer_grace_is_in_the_state_and_lasts_real_minutes() {
    // At 100 ms a tick, 20 game days pass in under two minutes; the grace
    // lasts the configured 30 minutes instead, but at most 60 game days.
    let (mut g, a) = game_with(GameConfig { tick_ms: 100.0, ..Default::default() });
    g.world.config.balance.attack_grace = 20.0;
    assert_eq!(g.grace_view(1), None, "founded at tick 0: no grace");
    for _ in 0..3 {
        tick(&mut g);
    }
    g.set_name(2, "bo");
    let b = g.found(2, &design("B")).unwrap().founders;
    g.connect(20, 2, "bo");
    hunters(&mut g, 1);
    hunters(&mut g, 2);
    let grace = g.grace_view(2).expect("a newcomer has its grace");
    let tpd = g.world.config.ticks_per_day as u64;
    let length = 18_000.min(60 * tpd).max(20 * tpd);
    assert_eq!(g.world.grace_length(), length);
    assert_eq!(grace.until_tick - g.world.tick(), length, "{grace:?}");
    let secs = grace.real_seconds_left.unwrap();
    assert_eq!(secs, (length / 10).div_ceil(10) * 10, "{grace:?}");
    assert!((grace.days_left - length as f32 / tpd as f32).abs() < 0.11, "{grace:?}");
    let s = g.states().into_iter().find(|(c, _)| *c == Target::Conn(20)).map(|(_, m)| m).unwrap();
    match s {
        ServerMsg::State(s) => assert_eq!(s.grace, Some(grace)),
        m => panic!("expected a state, got {m:?}"),
    }
    // Ann sees bo's animals marked as protected newcomers; bo sees ann's
    // unmarked (no grace).
    let at = g.world.organism(a[0]).unwrap().pos;
    for &id in &b {
        let i = g.world.organisms.iter().position(|o| o.id == id).unwrap();
        g.world.organisms[i].pos = at + Vec2::new(1.0, 1.0);
    }
    let ann = g.state_for(10).unwrap();
    let theirs: Vec<_> = ann.nearby.iter().filter(|n| n.owner == 2).collect();
    assert!(!theirs.is_empty() && theirs.iter().all(|n| n.protected == Some(true)), "{theirs:?}");
    let bo = g.state_for(20).unwrap();
    assert!(bo.nearby.iter().filter(|n| n.owner == 1).all(|n| n.protected.is_none()));
    // Two-sided: bo's graced animals can't attack ann's.
    let at = g.world.organism(a[0]).unwrap().pos;
    let i = g.world.organisms.iter().position(|o| o.id == b[0]).unwrap();
    (g.world.organisms[i].pos, g.world.organisms[i].growth) = (at + Vec2::new(1.0, 0.0), 1.0);
    let msg = ClientMsg::PossessAttack { organism: b[0], target: a[0] };
    g.command(20, 2, Some(1), ClientMsg::Possess { organism: b[0] });
    let out = g.command(20, 2, Some(2), msg);
    match &out[0].1 {
        ServerMsg::Result { error: Some(e), .. } => {
            assert_eq!(e.code, "own_grace", "{e:?}");
            // In real time too.
            let d = e.detail.as_ref().unwrap();
            assert!(
                d["real_seconds_left"].as_u64().is_some_and(|s| s > 0) && e.message.contains("real minutes"),
                "{e:?}"
            );
        }
        m => panic!("expected own_grace, got {m:?}"),
    }
    // Bo ends the grace (a predator's owner wants to hunt): told so, and
    // the attack goes through; a second time there is nothing to end.
    let out = g.command(20, 2, Some(3), ClientMsg::EndGrace);
    assert!(matches!(&out[0].1, ServerMsg::Result { ok: true, .. }), "{out:?}");
    assert!(out.iter().any(|(_, m)| matches!(m, ServerMsg::Alert { alert } if alert.text.contains("grace has ended"))));
    assert_eq!(g.grace_view(2), None);
    let out = g.command(20, 2, Some(4), ClientMsg::EndGrace);
    assert!(matches!(&out[0].1, ServerMsg::Result { error: Some(e), .. } if e.code == "no_grace"), "{out:?}");
    let out = g.command(20, 2, Some(5), ClientMsg::PossessAttack { organism: b[0], target: a[0] });
    assert!(matches!(&out[0].1, ServerMsg::Result { ok: true, .. }), "{out:?}");
    // The choice survives a restart.
    let r = Game::resume(g.world.clone(), GameConfig { tick_ms: 100.0, ..Default::default() }, g.saved(), Vec::new());
    assert_eq!(r.grace_view(2), None);
}

#[test]
fn a_grace_running_out_is_announced() {
    let (mut g, _) = game();
    g.world.config.balance.attack_grace = 0.05;
    g.cfg.grace_min_minutes = 0.0;
    g.world.set_attack_grace_min_ticks(0);
    tick(&mut g);
    g.set_name(2, "bo");
    g.found(2, &design("B")).unwrap();
    g.connect(20, 2, "bo");
    tick(&mut g);
    assert!(g.grace_view(2).is_some());
    let told =
        |g: &Game| g.alerts.get(&2).map_or(0, |l| l.iter().filter(|x| x.text.contains("grace has ended")).count());
    for _ in 0..(g.world.grace_length() + 2) {
        tick(&mut g);
    }
    assert_eq!(g.grace_view(2), None);
    assert_eq!(told(&g), 1);
}

/// The animal a player possesses dies: they hear of it at once, with the cause.
#[test]
fn a_possessed_animals_death_is_told_at_once() {
    let (mut g, a) = game();
    run(&mut g, ClientMsg::Possess { organism: a[0] }).unwrap();
    let i = g.world.organisms.iter().position(|o| o.id == a[0]).unwrap();
    g.world.organisms[i].alive = false;
    g.world.organisms[i].death = Some(DeathCause::Thirst);
    // As the sim's kill does: no longer possessed once dead.
    g.world.organisms[i].possessed_by = None;
    g.after_tick();
    let told = alerts(&g, AlertKind::Death);
    assert!(
        told.iter().any(|t| t.starts_with(&format!("#{}, the animal you were possessing, died of thirst", a[0]))),
        "{told:?}"
    );
}

/// Playtest 2: a migration that never gathers ends with an alert; an
/// arrived group's settle circle is in the state.
#[test]
fn a_migration_that_never_gathers_ends_with_an_alert() {
    let (mut g, a) = game();
    let home = g.world.organisms.iter().find(|o| o.alive && o.owner == 1).unwrap().pos;
    let far = land_away(&g, home, 30.0);
    run(&mut g, ClientMsg::Migrate { x: far.x, y: far.y, group: None }).unwrap();
    let deadline = g.world.migration_plan(1, 0).unwrap().deadline_tick;
    let main = |g: &Game| g.state_for(10).unwrap().groups.into_iter().find(|v| v.id == 0).unwrap();
    assert_eq!(main(&g).migration_deadline_tick, Some(deadline));
    // Keep the herd scattered far from the target until the deadline.
    while g.world.tick() <= deadline && g.world.migration_order(1, 0).is_some() {
        for (k, &id) in a.iter().enumerate() {
            if let Some(i) = g.world.organisms.iter().position(|o| o.id == id && o.alive) {
                g.world.organisms[i].pos = g.world.map.clamp(far + Vec2::new(30.0 - 12.0 * k as f32, 25.0));
            }
        }
        tick(&mut g);
    }
    assert_eq!(g.world.migration_order(1, 0), None);
    let texts = alerts(&g, AlertKind::Group);
    assert!(texts.iter().any(|t| t.contains("didn't gather at its migration target")), "{texts:?}");
    let v = main(&g);
    assert!(v.migration.is_none() && v.migration_deadline_tick.is_none() && v.home.is_none());
    assert!(v.last_outcome.as_deref().is_some_and(|o| o.contains("ended")), "{:?}", v.last_outcome);
}

#[test]
fn territories_and_rallies_check_their_water() {
    let (mut g, _) = game();
    match deep_water(&g) {
        Some(sea) => {
            assert_eq!(
                run(&mut g, ClientMsg::ClaimTerritory { group: 0, x: sea.x, y: sea.y, radius: 3.0 }),
                Err("mostly_water".into())
            );
            assert_eq!(run(&mut g, ClientMsg::Rally { group: 0, x: sea.x, y: sea.y }), Err("mostly_water".into()));
        }
        None => eprintln!("no deep water on this map"),
    }
    // A claim on land reports its water.
    let home = g.world.organisms.iter().find(|o| o.alive && o.owner == 1).unwrap().pos;
    let d = run(&mut g, ClientMsg::ClaimTerritory { group: 0, x: home.x, y: home.y, radius: 5.0 }).unwrap();
    assert!(d["water_share"].is_number() && d["drinkable_tiles"].is_number(), "{d}");
    if d["drinkable_tiles"].as_u64().unwrap() < LOW_WATER_TILES as u64 {
        assert!(d["warning"].as_str().unwrap().contains("drinking water"), "{d}");
    }
    // Abandoning and claiming again right away waits out the cooldown,
    // minutes of real time.
    run(&mut g, ClientMsg::AbandonTerritory { group: 0 }).unwrap();
    assert_eq!(
        run(&mut g, ClientMsg::ClaimTerritory { group: 0, x: home.x, y: home.y, radius: 5.0 }),
        Err("claim_cooldown".into())
    );
    assert_eq!(g.world.claim_cooldown_ticks(), 120, "2 minutes at 1 s a tick");
}

/// Possession ends a minute (real time) after the player's last session
/// closes; coming back within it keeps it. After a restart too.
#[test]
fn possession_ends_when_the_player_is_gone() {
    let (mut g, a) = game();
    let grace = minutes_to_ticks(POSSESSION_GRACE_MINUTES, g.cfg.tick_ms);
    run(&mut g, ClientMsg::Possess { organism: a[0] }).unwrap();
    let possessed = |g: &Game| g.world.organisms.iter().any(|o| o.id == a[0] && o.possessed_by == Some(1));
    g.disconnect(10);
    for _ in 0..grace / 2 {
        tick(&mut g);
    }
    assert!(possessed(&g), "within the grace");
    g.connect(11, 1, "ann");
    for _ in 0..grace + 2 {
        tick(&mut g);
    }
    assert!(possessed(&g), "came back: kept");
    g.disconnect(11);
    for _ in 0..grace + 1 {
        tick(&mut g);
    }
    assert!(!possessed(&g), "gone past the grace: released");
    // A restart drops every session: the grace runs from the restore.
    g.connect(12, 1, "ann");
    g.command(12, 1, Some(1), ClientMsg::Possess { organism: a[0] });
    let mut r = Game::resume(g.world.clone(), GameConfig::default(), g.saved(), Vec::new());
    for _ in 0..grace + 1 {
        r.world.step();
        r.after_tick();
    }
    assert!(!r.world.organisms.iter().any(|o| o.possessed_by == Some(1)));
}

/// Playtest 6: a big lineage lost 20–75 a day unflagged (5% rule); a
/// thirst-only crash read "going hungry"; mass cold deaths had no alert.
#[test]
fn hunger_thirst_and_cold_warnings_fit_the_lineage() {
    let (mut g, _) = game();
    run_days(&mut g, 1);
    let set = |g: &mut Game, alive: u32, deaths: &[(&str, u64)]| {
        let r = g.history.back_mut().unwrap();
        let e = r.lineages.iter_mut().find(|e| e.owner == 1).unwrap();
        e.alive = alive as _;
        e.deaths.clear();
        for &(c, n) in deaths {
            e.deaths.insert(c.into(), n);
        }
    };
    let kinds = |g: &Game| g.lineage_warnings(1).into_iter().map(|w| (w.kind, w.text)).collect::<Vec<_>>();
    // 20 of 2,000 starved: 1%, but past the floor of 15 a day.
    set(&mut g, 2000, &[("starved", 20)]);
    let w = kinds(&g);
    assert!(
        w.iter().any(|(k, t)| k == "starving" && t.contains("20 starved and 0 died of thirst (1% of it)")),
        "{w:?}"
    );
    // All thirst: said as thirst.
    set(&mut g, 300, &[("thirst", 9)]);
    let w = kinds(&g);
    assert!(w.iter().any(|(k, t)| k == "starving" && t.starts_with("Your lineage is dying of thirst")), "{w:?}");
    // A severe winter's cold deaths.
    set(&mut g, 300, &[("cold", 12)]);
    let w = kinds(&g);
    assert!(w.iter().any(|(k, t)| k == "cold" && t.contains("12 died of cold")), "{w:?}");
    // A small day: nothing.
    set(&mut g, 300, &[("starved", 3), ("cold", 2)]);
    assert!(kinds(&g).is_empty(), "{:?}", kinds(&g));
}

/// Playtest 7: 9 drowned at a severe winter's onset with no warning; a pack
/// went 63 → 23 over 40 days at 1–4 a day, never one day's warning.
#[test]
fn drownings_and_slow_die_offs_are_warned() {
    let (mut g, _) = game();
    run_days(&mut g, 12);
    let n = g.history.len();
    for (k, r) in g.history.iter_mut().enumerate() {
        let e = r.lineages.iter_mut().find(|e| e.owner == 1).unwrap();
        e.deaths.clear();
        if k + 11 >= n {
            // The last 11 days: 63 falling by 3 starved a day.
            let day = k + 11 - n;
            e.alive = (63 - 3 * day) as _;
            if day > 0 {
                e.deaths.insert("starved".into(), 3);
            }
        }
    }
    let kinds = |g: &Game| g.lineage_warnings(1).into_iter().map(|w| (w.kind, w.text)).collect::<Vec<_>>();
    let w = kinds(&g);
    assert!(
        w.iter().any(|(k, t)| k == "decline" && t.contains("63 → 33 alive over the last 10 days, 30 of them")),
        "{w:?}"
    );
    // The same deaths in a lineage that grows meanwhile: not a die-off (playtest 8: "954 → 1251").
    {
        let mut grown = g.history.clone();
        let len = grown.len();
        grown.back_mut().unwrap().lineages.iter_mut().find(|e| e.owner == 1).unwrap().alive = 90;
        let saved = std::mem::replace(&mut g.history, grown);
        assert!(!kinds(&g).iter().any(|(k, _)| k == "decline"), "{:?} ({len} days)", kinds(&g));
        g.history = saved;
    }
    // One day's drownings.
    g.history
        .back_mut()
        .unwrap()
        .lineages
        .iter_mut()
        .find(|e| e.owner == 1)
        .unwrap()
        .deaths
        .insert("drowned".into(), 4);
    let w = kinds(&g);
    assert!(w.iter().any(|(k, t)| k == "drowning" && t.starts_with("4 of your animals drowned")), "{w:?}");
}

/// Lineage-wide warnings (playtest 3): a hunger crash, near the hard cap,
/// animals starving inside a territory; each said once as an alert when it
/// starts, again after a few days, and listed in the lineage statistics.
#[test]
fn lineage_warnings_flag_hunger_the_cap_and_starving_territories() {
    let (mut g, a) = game();
    run_days(&mut g, 1);
    assert!(g.lineage_warnings(1).is_empty(), "{:?}", g.lineage_warnings(1));
    // A day with many starved: the last record says so.
    let r = g.history.back_mut().unwrap();
    let e = r.lineages.iter_mut().find(|e| e.owner == 1).unwrap();
    e.alive = 40;
    e.deaths.insert("starved".into(), 6);
    e.deaths.insert("thirst".into(), 2);
    let w = g.lineage_warnings(1);
    assert_eq!(w.iter().map(|w| w.kind.as_str()).collect::<Vec<_>>(), ["starving"], "{w:?}");
    assert!(
        w[0].text.contains("6 starved and 2 died of thirst") && w[0].text.contains("Litters are still full size"),
        "{w:?}"
    );
    // Mostly thirst: said as thirst, with water advice, not the Food layer.
    {
        let r = g.history.back_mut().unwrap();
        let e = r.lineages.iter_mut().find(|e| e.owner == 1).unwrap();
        e.deaths.insert("starved".into(), 0);
        e.deaths.insert("thirst".into(), 8);
    }
    let t = &g.lineage_warnings(1)[0].text;
    assert!(t.starts_with("Your lineage is dying of thirst: ") && !t.contains("Food layer"), "{t}");
    {
        let r = g.history.back_mut().unwrap();
        let e = r.lineages.iter_mut().find(|e| e.owner == 1).unwrap();
        e.deaths.insert("starved".into(), 6);
        e.deaths.insert("thirst".into(), 2);
    }
    // Near the hard cap.
    g.world.config.lineage_cap = 3;
    assert!(g.lineage_warnings(1).iter().any(|w| w.kind == "near_cap" && w.text.starts_with("Near the hard cap: ")));
    g.world.config.lineage_cap = 1500;
    // A territory whose holders starve.
    let at = g.world.organism(a[0]).unwrap().pos;
    for o in g.world.organisms.iter_mut().filter(|o| o.owner == 1) {
        o.pos = at;
        o.needs.hunger = 95.0;
    }
    run(&mut g, ClientMsg::ClaimTerritory { group: 0, x: at.x, y: at.y, radius: 4.0 }).unwrap();
    let w = g.lineage_warnings(1);
    assert!(w.iter().any(|w| w.kind == "territory_starving" && w.text.contains("Group A (main)")), "{w:?}");
    assert_eq!(g.lineage_stats(1).warnings, w);
    // Alerts: once, then again after the repeat interval.
    let mut out = Outbox::new();
    g.warning_alerts(10, &mut out);
    g.warning_alerts(11, &mut out);
    let need = |g: &Game| alerts(g, AlertKind::Need).iter().filter(|t| t.contains("going hungry")).count();
    assert_eq!(need(&g), 1);
    g.warning_alerts(10 + WARNING_REPEAT_DAYS, &mut out);
    assert_eq!(need(&g), 2);
    // Near the cap: once per approach, not every few days; again after dropping below the re-arm share.
    let near = |g: &Game| alerts(g, AlertKind::Need).iter().filter(|t| t.starts_with("Near the hard cap")).count();
    g.world.config.lineage_cap = 3;
    for d in 20..40 {
        g.warning_alerts(d, &mut out);
    }
    assert_eq!(near(&g), 1);
    g.world.config.lineage_cap = 1500;
    g.warning_alerts(41, &mut out);
    g.world.config.lineage_cap = 3;
    g.warning_alerts(42, &mut out);
    assert_eq!(near(&g), 2);
}

fn run_days(g: &mut Game, days: u32) {
    for _ in 0..days * g.world.config.ticks_per_day {
        tick(g);
    }
}

/// Playtest 3: a migrate while most of the group sleeps warns, and its
/// timeout blames the dormancy.
#[test]
fn a_migration_of_a_dormant_group_warns_and_says_why_it_ended() {
    let (mut g, _) = game();
    let ids: Vec<usize> = (0..g.world.organisms.len()).filter(|&i| g.world.organisms[i].owner == 1).collect();
    for &i in &ids {
        g.world.organisms[i].dormancy = evo_sim::organism::Dormancy::Summer;
    }
    let home = g.world.organisms[ids[0]].pos;
    let far = land_away(&g, home, 25.0);
    let d = run(&mut g, ClientMsg::Migrate { x: far.x, y: far.y, group: None }).unwrap();
    assert!(d["warning"].as_str().unwrap().contains("dormant"), "{d}");
    // They sleep through it: the order ends, and the alert says why.
    let deadline = g.world.migration_plan(1, 0).unwrap().ends_tick(g.world.config.ticks_per_day);
    while g.world.tick() <= deadline + g.world.config.ticks_per_hour() && g.world.migration_order(1, 0).is_some() {
        for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == 1) {
            o.dormancy = evo_sim::organism::Dormancy::Summer;
            o.pos = home;
            (o.needs.thirst, o.needs.hunger, o.health) = (0.0, 0.0, 100.0);
        }
        g.world.step();
        g.after_tick();
    }
    assert_eq!(g.world.migration_order(1, 0), None, "alive: {}", g.world.owner_count(1));
    let texts = alerts(&g, AlertKind::Group);
    assert!(texts.iter().any(|t| t.contains("is dormant") && t.contains("once it wakes")), "{texts:?}");
}

#[test]
fn claims_say_what_changed_and_warn_when_far_or_short_of_food() {
    let (mut g, _) = game_with(GameConfig { claim_cooldown_min_minutes: 0.0, ..Default::default() });
    let home = g.world.group_middle(1, 0).unwrap();
    // A radius below the minimum is raised, and says so.
    let d = run(&mut g, ClientMsg::ClaimTerritory { group: 0, x: home.x, y: home.y, radius: 0.5 }).unwrap();
    assert!(d["note"].as_str().unwrap().contains("raised"), "{d}");
    assert!(d["food_supports"].is_number() && d["distance"].is_number(), "{d}");
    // Far from the group: a warning.
    run(&mut g, ClientMsg::AbandonTerritory { group: 0 }).unwrap();
    let ticks = g.world.claim_cooldown_ticks();
    for _ in 0..ticks {
        g.world.step();
    }
    let far = land_away(&g, home, 45.0);
    let d = run(&mut g, ClientMsg::ClaimTerritory { group: 0, x: far.x, y: far.y, radius: 3.0 }).unwrap();
    let warnings: Vec<String> = serde_json::from_value(d["warnings"].clone()).unwrap();
    assert!(warnings.iter().any(|w| w.contains("tiles from the group")), "{d}");
}

#[test]
fn the_claim_wait_is_said_in_minutes_and_days() {
    let (mut g, _) = game();
    let home = g.world.group_middle(1, 0).unwrap();
    run(&mut g, ClientMsg::ClaimTerritory { group: 0, x: home.x, y: home.y, radius: 4.0 }).unwrap();
    run(&mut g, ClientMsg::AbandonTerritory { group: 0 }).unwrap();
    let out = g.command(10, 1, Some(2), ClientMsg::ClaimTerritory { group: 0, x: home.x, y: home.y, radius: 4.0 });
    let ServerMsg::Result { error: Some(e), .. } = &out[0].1 else { panic!("refused") };
    assert_eq!(e.code, "claim_cooldown");
    assert!(e.message.contains(" min (") && e.message.contains("game days"), "{}", e.message);
    let detail = e.detail.as_ref().unwrap();
    assert!(detail["ticks_left"].is_number() && detail["minutes_left"].is_number(), "{detail}");
}

#[test]
fn the_daily_check_alerts_a_dry_territory_and_a_drifted_herd_once() {
    let (mut g, _) = game();
    let home = g.world.group_middle(1, 0).unwrap();
    run(&mut g, ClientMsg::ClaimTerritory { group: 0, x: home.x, y: home.y, radius: 4.0 }).unwrap();
    // The land dries out: no water anywhere.
    let dry = |g: &mut Game| {
        for t in &mut g.world.map.tiles {
            t.water_depth = 0.0;
            t.snow = 0.0;
        }
    };
    dry(&mut g);
    let mut record = g.daily_record();
    if let Some(e) = record.lineages.iter_mut().find(|e| Some(e.lineage) == g.lineage_of(1)) {
        e.deaths.insert("thirst".into(), 7);
    }
    let mut out = Outbox::new();
    g.daily_group_checks(&record, &mut out);
    g.daily_group_checks(&record, &mut out);
    let texts = alerts(&g, AlertKind::Group);
    let dry_alerts: Vec<&String> = texts.iter().filter(|t| t.contains("no drinking water left")).collect();
    assert_eq!(dry_alerts.len(), 1, "once: {texts:?}");
    assert!(dry_alerts[0].contains("7 of the group died of thirst"), "{}", dry_alerts[0]);
    // Counted by group (playtest 8: a 6-animal group was told the whole
    // lineage's deaths): the group's own, not the lineage's 7.
    {
        let (mut g, _) = game();
        let home = g.world.group_middle(1, 0).unwrap();
        run(&mut g, ClientMsg::ClaimTerritory { group: 0, x: home.x, y: home.y, radius: 4.0 }).unwrap();
        dry(&mut g);
        g.group_deaths = Some([((1, 0), (0, 2))].into_iter().collect());
        let mut out = Outbox::new();
        g.daily_group_checks(&record, &mut out);
        let texts = alerts(&g, AlertKind::Group);
        let t = texts.iter().find(|t| t.contains("no drinking water left")).expect("dry alert");
        assert!(t.contains("2 of the group died of thirst"), "{t}");
    }
    // A herd that drifted from the home its migration gave it.
    run(&mut g, ClientMsg::AbandonTerritory { group: 0 }).unwrap();
    let target = land_away(&g, home, 10.0);
    run(&mut g, ClientMsg::Migrate { x: target.x, y: target.y, group: None }).unwrap();
    let ids: Vec<usize> = (0..g.world.organisms.len()).filter(|&i| g.world.organisms[i].owner == 1).collect();
    for &i in &ids {
        g.world.organisms[i].pos = target;
    }
    while g.world.migration_order(1, 0).is_some() {
        g.world.step();
        g.after_tick();
        for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == 1) {
            o.pos = target;
        }
    }
    let s = g.world.settle_circle(1, 0).expect("home");
    let away = g.world.map.clamp(target + Vec2::new(3.0 * s.radius, 0.0));
    for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == 1) {
        o.pos = away;
    }
    g.daily_group_checks(&record, &mut out);
    g.daily_group_checks(&record, &mut out);
    let drifted: Vec<String> =
        alerts(&g, AlertKind::Group).into_iter().filter(|t| t.contains("has drifted from its home")).collect();
    assert_eq!(drifted.len(), 1, "{drifted:?}");
    // Sent away from that home by a new order (here ended at once, so the
    // old home stands): no "drifted" for the home it left (playtest 8).
    for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == 1) {
        o.pos = target;
    }
    g.daily_group_checks(&record, &mut out); // back home: the alert may come again
    let next = land_away(&g, target, 20.0);
    run(&mut g, ClientMsg::Migrate { x: next.x, y: next.y, group: None }).unwrap();
    g.world.clear_migration(1);
    for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == 1) {
        o.pos = away;
    }
    g.daily_group_checks(&record, &mut out);
    let again: Vec<String> =
        alerts(&g, AlertKind::Group).into_iter().filter(|t| t.contains("has drifted from its home")).collect();
    assert_eq!(again.len(), 1, "not for the home a migration left: {again:?}");
}

/// Playtest 4: far claims migrate first, rivals' ground and bad radii are
/// refused, and orders say what they replace.
#[test]
fn far_claims_migrate_first_and_claims_say_what_they_replace() {
    let (mut g, _) = game_with(GameConfig { claim_cooldown_min_minutes: 0.0, ..Default::default() });
    let home = g.world.group_middle(1, 0).unwrap();
    assert_eq!(
        run(&mut g, ClientMsg::ClaimTerritory { group: 0, x: home.x, y: home.y, radius: -2.0 }),
        Err("bad_radius".into())
    );
    assert_eq!(g.world.territory(1, 0), None, "nothing committed");
    // A far claim: a migration now, the territory on arrival.
    let far = land_away(&g, home, 35.0);
    let d = run(&mut g, ClientMsg::ClaimTerritory { group: 0, x: far.x, y: far.y, radius: 4.0 }).unwrap();
    assert_eq!(d["pending"], true, "{d}");
    assert!(
        d["migrating_to"]["x"].is_number()
            && d["note"].as_str().unwrap().contains("claims the territory when it arrives"),
        "{d}"
    );
    assert_eq!(g.world.territory(1, 0), None);
    assert!(g.world.migration_order(1, 0).is_some());
    // A rally says it replaces that migration.
    let d = run(&mut g, ClientMsg::Rally { group: 0, x: home.x, y: home.y }).unwrap();
    assert_eq!(d["replaced"], serde_json::json!(["migration"]), "{d}");
    let d = run(&mut g, ClientMsg::CancelRally { group: 0 }).unwrap();
    assert!(d["note"].is_string(), "{d}");
    // Another player's territory can't be claimed over.
    let world = build_world(&WorldSettings { seed: 7, width: 64, height: 64, wild: false, ..Default::default() });
    let mut two = Game::new(world, GameConfig::default());
    two.found(1, &design("A")).unwrap();
    two.found(2, &design("B")).unwrap();
    two.connect(10, 1, "ann");
    let theirs = two.world.group_middle(2, 0).unwrap();
    two.world.claim_territory(2, 0, theirs, 5.0).unwrap();
    let out =
        two.command(10, 1, Some(1), ClientMsg::ClaimTerritory { group: 0, x: theirs.x, y: theirs.y, radius: 4.0 });
    let ServerMsg::Result { error: Some(e), .. } = &out[0].1 else { panic!("refused") };
    assert_eq!(e.code, "overlaps");
}

#[test]
fn a_migration_that_never_moves_says_so_on_the_group_card() {
    let (mut g, _) = game();
    let home = g.world.group_middle(1, 0).unwrap();
    let far = land_away(&g, home, 25.0);
    for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == 1) {
        o.pos = home;
    }
    run(&mut g, ClientMsg::Migrate { x: far.x, y: far.y, group: None }).unwrap();
    let ends = g.world.migration_plan(1, 0).unwrap().deadline_tick;
    while g.world.migration_order(1, 0).is_some() && g.world.tick() <= ends {
        for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == 1) {
            o.pos = home;
            (o.needs.thirst, o.needs.hunger, o.health) = (0.0, 0.0, 100.0);
        }
        g.world.step();
        g.after_tick();
    }
    let v = g.state_for(10).unwrap().groups.into_iter().find(|v| v.id == 0).unwrap();
    assert!(v.last_outcome.as_deref().is_some_and(|o| o.contains("never got going")), "{:?}", v.last_outcome);
    assert!(alerts(&g, AlertKind::Group).iter().any(|t| t.contains("never got going")));
    // A new order clears it.
    run(&mut g, ClientMsg::Rally { group: 0, x: home.x, y: home.y }).unwrap();
    let v = g.state_for(10).unwrap().groups.into_iter().find(|v| v.id == 0).unwrap();
    assert_eq!(v.last_outcome, None);
}

/// Playtest 5: group outcomes survive a flood of daily summaries.
#[test]
fn group_alerts_outlive_daily_summaries() {
    let (mut g, _) = game();
    let mut out = Outbox::new();
    g.alert(1, AlertKind::Group, "Group A (main) reached its migration target (1, 1)".into(), None, &mut out);
    for k in 0..(MAX_ALERTS + 20) {
        g.alert(1, AlertKind::DailySummary, format!("day {k}"), None, &mut out);
    }
    assert!(alerts(&g, AlertKind::Group).iter().any(|t| t.contains("reached")), "evicted");
    assert_eq!(g.alerts(1).len(), MAX_ALERTS);
}

#[test]
fn far_claims_and_rallies_share_the_travel_warnings_and_migrate_checks_the_destination() {
    let (mut g, _) = game_with(GameConfig { claim_cooldown_min_minutes: 0.0, ..Default::default() });
    let home = g.world.group_middle(1, 0).unwrap();
    for o in g.world.organisms.iter_mut().filter(|o| o.owner == 1) {
        o.dormancy = evo_sim::organism::Dormancy::Winter;
    }
    let far = land_away(&g, home, 35.0);
    let d = run(&mut g, ClientMsg::ClaimTerritory { group: 0, x: far.x, y: far.y, radius: 4.0 }).unwrap();
    assert!(d["warning"].as_str().unwrap().contains("dormant"), "far claim: {d}");
    let d = run(&mut g, ClientMsg::Rally { group: 0, x: home.x, y: home.y }).unwrap();
    assert!(d["warning"].as_str().unwrap().contains("dormant"), "rally: {d}");
    // A destination with no water: said in the migrate reply.
    for t in &mut g.world.map.tiles {
        (t.water_depth, t.snow) = (0.0, 0.0);
    }
    let d = run(&mut g, ClientMsg::Migrate { x: far.x, y: far.y, group: None }).unwrap();
    assert!(d["destination"]["drinkable_tiles"].is_number(), "{d}");
    assert!(d["warning"].as_str().unwrap().contains("Destination: Little or no drinking water"), "{d}");
    // A half-tile nudge onto a tile's centre is no news.
    let p = g.world.map.clamp(home + Vec2::new(0.3, 0.2));
    let d = run(&mut g, ClientMsg::Migrate { x: p.x, y: p.y, group: None }).unwrap();
    assert!(d.get("moved").is_none(), "{d}");
}

#[test]
fn the_daily_check_covers_homes_outgrown_ground_and_says_less_than_a_day() {
    assert_eq!(days_text(0.2), "less than a day");
    assert_eq!(days_text(1.2), "about 1 day");
    assert_eq!(days_text(6.4), "about 6 days");
    let (mut g, _) = game();
    let home = g.world.group_middle(1, 0).unwrap();
    // A territory a herd has outgrown (food for fewer than it has).
    run(&mut g, ClientMsg::ClaimTerritory { group: 0, x: home.x, y: home.y, radius: 3.0 }).unwrap();
    let f = g.world.food_check(1, 0, home, 3.0).unwrap();
    let record = g.daily_record();
    let mut out = Outbox::new();
    g.daily_group_checks(&record, &mut out);
    let texts = alerts(&g, AlertKind::Group);
    if f.supports < f.members && f.standing_days >= BARE_DAYS {
        assert!(texts.iter().any(|t| t.contains("has outgrown its territory")), "{texts:?}");
    }
    // A home ground gone dry is checked too, and a rally elsewhere is no drift.
    run(&mut g, ClientMsg::AbandonTerritory { group: 0 }).unwrap();
    let target = land_away(&g, home, 10.0);
    run(&mut g, ClientMsg::Migrate { x: target.x, y: target.y, group: None }).unwrap();
    while g.world.migration_order(1, 0).is_some() {
        for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == 1) {
            o.pos = target;
        }
        g.world.step();
        g.after_tick();
    }
    assert!(g.world.settle_circle(1, 0).is_some());
    for t in &mut g.world.map.tiles {
        (t.water_depth, t.snow) = (0.0, 0.0);
    }
    g.daily_group_checks(&record, &mut out);
    assert!(alerts(&g, AlertKind::Group).iter().any(|t| t.contains("home ground") && t.contains("no drinking water")));
    let far = land_away(&g, target, 30.0);
    g.world.rally_group(1, 0, far).ok();
    for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == 1) {
        o.pos = far;
    }
    g.daily_group_checks(&record, &mut out);
    assert!(
        !alerts(&g, AlertKind::Group).iter().any(|t| t.contains("drifted")),
        "a rally elsewhere is the player's doing"
    );
}

/// Kill about a sixth of `player`'s animals (a catastrophe's hit, past
/// `SURVIVAL_EXPOSED`) and record the low.
fn hit(g: &mut Game, player: PlayerId) {
    let n = g.world.organisms.iter().filter(|o| o.alive && o.owner == player).count();
    for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == player).take(n.div_ceil(6)) {
        o.alive = false;
    }
    g.note_catastrophe_lows();
}

/// A catastrophe that ends as its season ends counts for that season, not
/// the next (playtest 5: the severe winter of days 210-240).
#[test]
fn survival_counts_in_the_season_the_catastrophe_fell_in() {
    let (mut g, _) = game();
    // Bob is alive through it too, but never hit: no survival, not in the public list.
    g.set_name(2, "bob");
    g.found(2, &design("B")).unwrap();
    g.season = crate::league::SeasonTracker::new(1, 0, 4);
    g.cfg.season_days = 4;
    g.world.schedule_catastrophe(evo_sim::chronicle::CatastropheKind::SevereWinter, 1, Some(3));
    run_days(&mut g, 2);
    hit(&mut g, 1);
    assert!(g.chronicle().catastrophes.active.expect("under way").survivors.is_empty(), "judged when it ends");
    run_days(&mut g, 4);
    let last = g.last_season_record.clone().expect("season 1 finished");
    let survived: Vec<PlayerId> = last.standings[4].rows.iter().map(|r| r.player).collect();
    assert_eq!(survived, vec![1], "{:?}", last.standings[4]);
    let ch = g.chronicle();
    assert_eq!(ch.catastrophes.history[0].survivors, vec![1], "public list: bob wasn't hit");
    assert!(ch.achievements.players.iter().all(|t| t.player != 2 || t.catastrophes_survived == 0));
    let now = g.season.standings(|p| p.to_string());
    assert!(now[4].rows.is_empty(), "not credited again in season 2: {:?}", now[4]);
}

/// A survivor is the lineage alive when the catastrophe began, keeping at
/// least half its size then (playtest 7, M15; a quarter until playtest 8),
/// through at least a quarter of a season of it: one that lost most isn't,
/// nor is a lineage re-founded during it, nor one counted after a day.
#[test]
fn survivors_keep_half_the_lineage_they_started_with() {
    let (mut g, _) = game();
    // A 4-day season: a quarter of it is a day.
    g.cfg.season_days = 4;
    g.world.schedule_catastrophe(evo_sim::chronicle::CatastropheKind::SevereWinter, 1, Some(4));
    run_days(&mut g, 2);
    let c = g.world.chronicle.active_catastrophe().expect("under way").id;
    let (l, start) = *g.survival_starts.get(&c).and_then(|m| m.get(&1)).expect("player 1's size at the start");
    assert_eq!(Some(l), g.lineage_of(1));
    assert!(start > 4, "{start}");
    // Untouched by it: no survival (playtest 9: a benign year made everyone a survivor).
    assert!(!g.survived(1, c), "not hit");
    hit(&mut g, 1);
    assert!(g.survived(1, c));
    // Down to under half: not a survivor.
    let keep = (start as f64 * crate::game::SURVIVAL_KEEP).ceil() as usize - 1;
    for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == 1).skip(keep) {
        o.alive = false;
    }
    assert!(!g.survived(1, c));
    // A lineage founded during it (player 2, absent at the start): not a survivor.
    g.set_name(2, "bo");
    g.found(2, &design("B")).unwrap();
    assert!(!g.survived(2, c));
}

/// A grace carries over through the whole re-founding window, not only
/// twice the grace (playtest 8: small re-foundings chained just past that
/// got a fresh grace each).
#[test]
fn grace_carries_over_through_the_refound_window() {
    let (g, _) = game();
    let window = g.refound_window_days() * g.world.config.ticks_per_day as u64;
    assert!(g.world.grace_carry_ticks() >= window);
    assert!(g.world.grace_carry_ticks() >= 2 * g.world.grace_length());
}

/// The found reply (and an alert) says whether a lineage has a newcomer
/// grace and why: a fresh one, or one continued from an earlier lineage,
/// even when that one has run out (playtest 5).
#[test]
fn founding_says_whether_grace_applies_and_why() {
    let (mut g, _) = game();
    g.world.config.balance.attack_grace = 0.5;
    g.cfg.grace_min_minutes = 0.0;
    g.world.set_attack_grace_min_ticks(0);
    tick(&mut g);
    g.set_name(2, "bo");
    let ok = g.found(2, &design("B")).unwrap();
    assert!(ok.grace.as_deref().unwrap().starts_with("Newcomer grace: "), "{:?}", ok.grace);
    assert!(ok.grace.as_deref().unwrap().contains("40 or fewer"), "{:?}", ok.grace);
    g.connect(20, 2, "bo");
    tick(&mut g);
    assert!(g.alerts.get(&2).is_some_and(|l| l.iter().any(|a| a.text.starts_with("Newcomer grace: "))));
    // Past the grace, retire and found again at once: the old grace carries on, run out.
    for _ in 0..g.world.grace_length() + 2 {
        tick(&mut g);
    }
    let lineage = g.lineage_of(2).unwrap();
    g.command(20, 2, Some(1), ClientMsg::RetireLineage { lineage });
    // The designer says so before founding (playtest 7: it promised the winter rule).
    let preview = g.grace_preview(2, 20).unwrap();
    assert!(preview.starts_with("If you found now: No newcomer grace: it continues the grace"), "{preview}");
    let again = g.found(2, &design("B2")).unwrap();
    let text = again.grace.unwrap();
    assert!(text.starts_with("No newcomer grace: it continues the grace of your earlier lineage"), "{text}");
}

/// A small founding whose grace would end late in autumn or in its first
/// winter keeps it to that winter's end (playtest 5), and with
/// `Balance::founding_drought` through the spring after it, while its
/// first young grow up (playtest 6); a big one doesn't.
#[test]
fn a_small_foundings_grace_lasts_through_its_first_winter() {
    let (mut g, a) = game();
    let w = &mut g.world;
    let tpd = w.config.ticks_per_day as u64;
    let dpy = w.config.days_per_year as u64;
    let l = w.organism(a[0]).unwrap().lineage;
    let len = w.grace_length();
    let winter = (3 * dpy).div_ceil(4) * tpd;
    let start = winter.saturating_sub(len) + tpd;
    if start + len < dpy * tpd && start < winter {
        assert_eq!(w.grace_end(l, start), (dpy + dpy / 4) * tpd, "through the spring after");
    }
    w.config.balance.founding_drought = 0.0;
    let w = &g.world;
    let tpd = w.config.ticks_per_day as u64;
    let dpy = w.config.days_per_year as u64;
    let l = w.organism(a[0]).unwrap().lineage;
    let len = w.grace_length();
    // A start whose plain grace ends on the first day of winter.
    let winter = (3 * dpy).div_ceil(4) * tpd;
    let start = winter.saturating_sub(len) + tpd;
    if start + len < dpy * tpd && start < winter {
        assert_eq!(w.grace_end(l, start), dpy * tpd, "to the winter's end");
    }
    // A start whose grace ends mid-summer: unchanged.
    let summer = dpy / 4 * tpd + tpd;
    assert_eq!(w.grace_end(l, summer.saturating_sub(len).max(1)), summer.saturating_sub(len).max(1) + len);
    assert!(w.lineages[l as usize].founders <= evo_sim::world::commands::GRACE_WINTER_FOUNDERS);
}

/// Playtest 6: long walks of big groups warn of the journey; the
/// destination is rated for the group's size, without a distance warning.
#[test]
fn a_long_migration_warns_of_the_journey() {
    let (mut g, _) = game();
    let home = g.world.group_middle(1, 0).unwrap();
    let far = land_away(&g, home, 45.0);
    // Bare land all along: no food on the way.
    for t in &mut g.world.map.tiles {
        t.veg = 0.0;
    }
    let d = run(&mut g, ClientMsg::Migrate { x: far.x, y: far.y, group: None }).unwrap();
    let j = &d["journey"];
    assert!(j["days"].as_f64().unwrap() > 0.0 && j["tiles"].as_f64().unwrap() > 30.0, "{d}");
    let warning = d["warning"].as_str().unwrap_or("");
    if j["days"].as_f64().unwrap() >= JOURNEY_WARN_DAYS as f64 {
        assert!(warning.contains("days on the way") && warning.contains("heavy losses"), "{d}");
    }
    assert!(!warning.contains("Destination: It is"), "no distance warning for the destination: {d}");
}

#[test]
fn other_players_territories_show_in_the_state() {
    let world = build_world(&WorldSettings { seed: 7, width: 64, height: 64, wild: false, ..Default::default() });
    let mut g = Game::new(world, GameConfig::default());
    g.found(1, &design("A")).unwrap();
    g.found(2, &design("B")).unwrap();
    g.connect(10, 1, "ann");
    let mine = g.world.group_middle(1, 0).unwrap();
    // Bob's territory right by ann's herd: in her view.
    let near = g.world.map.clamp(mine + Vec2::new(4.0, 0.0));
    g.world.claim_territory(2, 0, near, 3.0).unwrap();
    let s = g.state_for(10).unwrap();
    assert!(s.territories.iter().any(|t| (t.x - near.x).abs() < 0.01 && t.radius == 3.0), "{:?}", s.territories);
    // Her own territory isn't in that list (it is in her groups).
    let theirs_only = s.territories.len();
    g.world.claim_territory(1, 0, mine, 3.0).ok();
    assert_eq!(g.state_for(10).unwrap().territories.len(), theirs_only);
}

#[test]
fn a_stalled_migration_says_how_far_it_got_and_bare_alerts_dont_repeat() {
    assert_eq!(timeout_reason(0.0, 0.0, 0.0, false, 12.4).1, "stalled after 12 tiles");
    assert_eq!(timeout_reason(0.0, 0.0, 0.0, true, 0.2).1, "never got going");
    let (mut g, _) = game();
    let home = g.world.group_middle(1, 0).unwrap();
    run(&mut g, ClientMsg::ClaimTerritory { group: 0, x: home.x, y: home.y, radius: 100.0 }).unwrap();
    let t = g.world.territory(1, 0).unwrap();
    let record = g.daily_record();
    let mut out = Outbox::new();
    let set_veg = |g: &mut Game, f: f32| {
        for k in 0..g.world.map.tiles.len() {
            let max = g.world.map.ground()[k].veg_max;
            g.world.map.tiles[k].veg = f * max;
        }
    };
    let bare = |g: &Game| alerts(g, AlertKind::Group).iter().filter(|a| a.contains("grazed bare")).count();
    set_veg(&mut g, 0.0);
    g.daily_group_checks(&record, &mut out);
    assert_eq!(bare(&g), 1);
    // A little regrowth (not recovered): no new alert when it dips again.
    set_veg(&mut g, 0.02);
    let f = g.world.food_check(1, 0, t.center, t.radius).unwrap();
    // Plants for about 4 days: above bare (3), below recovered (6).
    set_veg(&mut g, 0.02 * 4.0 / f.standing_days);
    assert!((3.0..6.0).contains(&g.world.food_check(1, 0, t.center, t.radius).unwrap().standing_days));
    g.daily_group_checks(&record, &mut out);
    set_veg(&mut g, 0.0);
    g.daily_group_checks(&record, &mut out);
    assert_eq!(bare(&g), 1, "said once until it really recovers");
}

/// Playtest 7: nothing told the player of the drought provision. In a
/// drought the found reply (and an alert) says the founders start watered
/// and lose water more slowly; outside one it says nothing.
#[test]
fn a_drought_founding_says_its_founders_were_watered() {
    let (mut g, _) = game();
    tick(&mut g);
    g.set_name(2, "bo");
    g.world.map.drought = 0.5;
    let ok = g.found(2, &design("B")).unwrap();
    let text = ok.watered.expect("the provision applies");
    assert!(text.starts_with("Drought provision: 50% of the land is dry"), "{text}");
    assert!(text.contains("50% of the usual rate") && text.contains("lasts a drought"), "{text}");
    g.connect(20, 2, "bo");
    tick(&mut g);
    assert!(g.alerts.get(&2).is_some_and(|l| l.iter().any(|a| a.text.starts_with("Drought provision: "))));
    g.set_name(3, "cy");
    g.world.map.drought = 0.1;
    assert_eq!(g.found(3, &design("C")).unwrap().watered, None);
}

/// Alerts carry a stable id: two with the same tick and text are told
/// apart, and reading the feed again (or after a restart) gives the same
/// ids and ticks (playtest 7: a watcher keyed by tick and text saw
/// "duplicates").
#[test]
fn alerts_have_stable_ids() {
    let (mut g, _) = game();
    // The founding's own notes (grace, landing) go out on the first tick.
    tick(&mut g);
    let mut out = Outbox::new();
    g.alert(1, AlertKind::World, "same".into(), None, &mut out);
    g.alert(1, AlertKind::World, "same".into(), None, &mut out);
    let a = g.alerts(1);
    let ids: Vec<u64> = a.iter().rev().take(2).map(|x| x.id).collect();
    assert!(ids[0] != ids[1] && ids.iter().all(|&i| i > 0), "{a:?}");
    tick(&mut g);
    assert_eq!(g.alerts(1), a, "unchanged by later reads and ticks");
    let r = Game::resume(g.world.clone(), GameConfig::default(), g.saved(), Vec::new());
    assert_eq!(r.alerts(1), a, "kept over a restart");
}

/// Playtest 7: a short trip over bare land warns too, and the sentence
/// starts with a capital.
#[test]
fn a_short_trip_over_bare_land_warns() {
    let (mut g, _) = game();
    let home = g.world.group_middle(1, 0).unwrap();
    let near = land_away(&g, home, 14.0);
    for t in &mut g.world.map.tiles {
        t.veg = 0.0;
    }
    let d = run(&mut g, ClientMsg::Migrate { x: near.x, y: near.y, group: None }).unwrap();
    let days = d["journey"]["days"].as_f64().unwrap();
    assert!(days < JOURNEY_WARN_DAYS as f64, "{d}");
    assert!(d["journey"]["food_share"].as_f64().unwrap() < JOURNEY_BARE_SHARE as f64, "{d}");
    let warning = d["warning"].as_str().unwrap_or("");
    assert!(warning.contains("days on the way") && warning.contains("The land along the way holds about"), "{d}");
}

/// Playtest 7: a split group far from the view still sees its rivals'
/// territories (the refusal says they show on the map), and a split's
/// reply counts every group as the state does.
#[test]
fn rival_outlines_follow_each_group_and_splits_count_like_the_state() {
    let world = build_world(&WorldSettings { seed: 7, width: 128, height: 128, wild: false, ..Default::default() });
    let mut g = Game::new(world, GameConfig::default());
    let a = g.found(1, &design("A")).unwrap().founders;
    g.found(2, &design("B")).unwrap();
    g.connect(10, 1, "ann");
    let mine = g.world.group_middle(1, 0).unwrap();
    // Half of ann's founders walk far off and split.
    let far = land_away(&g, mine, g.cfg.default_view_radius + 40.0);
    let half: Vec<OrgId> = a[..3].to_vec();
    for o in g.world.organisms.iter_mut().filter(|o| half.contains(&o.id)) {
        o.pos = far;
    }
    let d = run(&mut g, ClientMsg::SplitGroup { group: 0, organisms: Some(half) }).unwrap();
    let groups = d["groups"].as_array().unwrap();
    let total: u64 = groups.iter().map(|x| x["members"].as_u64().unwrap()).sum();
    assert_eq!(total, d["population"].as_u64().unwrap(), "{d}");
    let state_total: usize = g.state_for(10).unwrap().groups.iter().map(|x| x.members).sum();
    assert_eq!(total as usize, state_total);
    // Bob claims beside the far group, out of ann's view.
    let by = g.world.map.clamp(far + Vec2::new(6.0, 0.0));
    g.world.claim_territory(2, 0, by, 3.0).unwrap();
    let s = g.state_for(10).unwrap();
    assert!((s.view.x - by.x).abs() > s.view.radius + 3.0 || (s.view.y - by.y).abs() > s.view.radius + 3.0);
    assert!(s.territories.iter().any(|t| (t.x - by.x).abs() < 0.01), "{:?}", s.territories);
}

/// Playtest 7: a far claim's reply rates the circle it will get, and a
/// starving territory's hold loosens with an alert.
#[test]
fn far_claims_rate_the_clamped_circle_and_a_starving_hold_loosens() {
    let (mut g, _) = game();
    let home = g.world.group_middle(1, 0).unwrap();
    let far = land_away(&g, home, evo_sim::world::groups::FAR_CLAIM_TILES + 4.0);
    let limit = World::territory_radius_limit(g.world.groups(1)[0].adults);
    let d = run(&mut g, ClientMsg::ClaimTerritory { group: 0, x: far.x, y: far.y, radius: 300.0 }).unwrap();
    assert_eq!(d["pending"], true, "{d}");
    assert!((d["radius"].as_f64().unwrap() - limit as f64).abs() < 1e-3, "{d}");
    assert!(d["note"].as_str().unwrap().contains("radius cut to the group's limit"), "{d}");
    // A territory at home whose holders all starve.
    let (mut g, _) = game();
    g.world.claim_territory(1, 0, home, 4.0).unwrap();
    for _ in 0..3 * g.world.config.ticks_per_hour().max(1) {
        for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == 1) {
            o.needs.hunger = o.needs.hunger.max(90.0);
        }
        tick(&mut g);
    }
    let texts = alerts(&g, AlertKind::Group);
    assert!(texts.iter().any(|t| t.contains("the hold is loosened")), "{texts:?}");
}

/// Playtest 8 Lows in the shop: no mating slot for a player with no living
/// lineage; a blocked reinforcement quotes what the next will cost (not the
/// price with every recent purchase counted).
#[test]
fn the_shop_refuses_slots_without_a_lineage_and_quotes_the_next_reinforcement() {
    let (mut g, _) = game();
    let _ = g.post(1, 5000, LedgerKind::Income, "test".into(), None, None);
    let today = g.world.day();
    // Two bought on different days: the first frees its slot first.
    g.economy.reinforce_days.insert(1, vec![today, today + 1]);
    let shop = g.shop_view(1);
    assert!(shop.reinforce_blocked.is_some() && shop.reinforce_next_day.is_some(), "{shop:?}");
    assert_eq!(shop.reinforce_price, economy::reinforce_price(1), "the price once the old one lapses");
    // Retired: no slots for sale.
    let lineage = g.lineage_of(1).unwrap();
    g.retire(1, lineage).unwrap();
    assert_eq!(run(&mut g, ClientMsg::BuyMatingSlot), Err("no_lineage".to_string()));
}

/// Playtest 8: two alerts with one id, after the alert holding the highest
/// id was dropped. Ids never repeat.
#[test]
fn alert_ids_never_repeat_when_alerts_are_dropped() {
    let (mut g, _) = game();
    let mut out = Outbox::new();
    g.alert(1, AlertKind::DailySummary, "summary".into(), None, &mut out);
    let first = g.alerts(1).last().unwrap().id;
    g.alerts.get_mut(&1).unwrap().pop_back();
    g.alert(1, AlertKind::World, "next".into(), None, &mut out);
    assert!(g.alerts(1).last().unwrap().id > first);
}

/// Playtest 8: "spent in the shop 0" after real purchases, which had
/// scrolled out of the entries sent. The ledger reply carries every entry's
/// totals.
#[test]
fn the_ledger_reply_totals_every_entry() {
    let (mut g, _) = game();
    let _ = g.post(1, 1000, LedgerKind::Income, "test".into(), None, None);
    let shop = g.shop_view(1);
    run(&mut g, ClientMsg::BuyMatingSlot).unwrap();
    for _ in 0..economy::LEDGER_KEPT {
        let _ = g.post(1, 1, LedgerKind::Income, "filler".into(), None, None);
    }
    let d = run(&mut g, ClientMsg::Ledger { limit: Some(5) }).unwrap();
    assert_eq!(d["totals"]["shop"].as_u64(), shop.slot_price, "{d}");
    assert!(d["entries"].as_array().unwrap().iter().all(|e| e["kind"] != "purchase"));
}

/// Playtest 8: the survey left out a territory held while its group was
/// away, and refused while the main group was empty.
#[test]
fn the_survey_shows_territories_held_away_and_works_without_a_main_group() {
    let (mut g, a) = game();
    let _ = g.post(1, 5000, LedgerKind::Income, "test".into(), None, None);
    g.set_name(2, "bo");
    g.found(2, &design("B")).unwrap();
    let mine = g.world.group_middle(1, 0).unwrap();
    // Bo's group 0 holds a territory near ann, then migrates off keeping it.
    let near = land_away(&g, mine, 6.0);
    for o in g.world.organisms.iter_mut().filter(|o| o.owner == 2) {
        o.pos = near;
    }
    g.world.claim_territory(2, 0, near, 4.0).unwrap();
    // Migrating keeps the territory, held while away.
    let far = land_away(&g, near, 12.0);
    g.world.command_migrate_group(2, 0, far).unwrap();
    assert!(g.world.territory(2, 0).is_none() && g.world.territory_away(2, 0).is_some());
    // Ann's main group empties into group 1.
    for o in g.world.organisms.iter_mut().filter(|o| a.contains(&o.id)) {
        o.group = 1;
    }
    let s = g.survey(1).expect("from the largest group");
    let bo = s.groups.iter().find(|x| x.player == 2).expect("bo surveyed");
    assert!(bo.territory.is_some(), "{s:?}");
}

/// Playtest 8 (M9): a group about to bed down, a destination on a rival's
/// territory, water that won't last a drought, a journey's food shared
/// with the grazers along the way, and where founders landed.
#[test]
fn migrate_warns_of_sleep_rival_ground_and_drying_water() {
    let world = build_world(&WorldSettings { seed: 7, width: 64, height: 64, wild: false, ..Default::default() });
    let mut g = Game::new(world, GameConfig { claim_cooldown_min_minutes: 0.0, ..Default::default() });
    g.set_name(1, "ann");
    g.set_name(2, "bob");
    g.found(1, &design("A")).unwrap();
    g.connect(10, 1, "ann");
    let home = g.world.group_middle(1, 0).unwrap();
    // Bob lands later: told how far ann is.
    let bob = g.found(2, &design("B")).unwrap();
    let landing = bob.landing.expect("landing");
    assert!(landing.starts_with("Your founders landed around (") && landing.contains("ann's"), "{landing}");
    // Bob's territory beside ann's herd: a migration into it says so.
    let m = &g.world.map;
    let near = (0..m.height)
        .flat_map(|y| (0..m.width).map(move |x| evo_sim::terrain::Map::tile_center(x, y)))
        .find(|&p| {
            let t = m.tile_at(p);
            (12.0..20.0).contains(&p.dist(home)) && t.water_depth == 0.0 && t.ice == 0.0 && m.ground_at(p).slope < 0.4
        })
        .expect("land near");
    g.world.claim_territory(2, 0, near, 5.0).unwrap();
    let d = run(&mut g, ClientMsg::Migrate { x: near.x, y: near.y, group: None }).unwrap();
    let w = d["warning"].as_str().unwrap_or_default().to_string();
    assert!(w.contains("Destination: it is inside bob's territory"), "{d}");
    assert_eq!(w.matches("Destination:").count(), 1, "said once: {w}");
    // Estivators in a parched land bed down within days (spring drought
    // estivation): warned, though none sleeps yet.
    for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == 1) {
        o.traits.seasonal.estivation = 0.8;
    }
    for t in &mut g.world.map.tiles {
        t.wetness = 0.04;
    }
    assert!(g.world.dormant_share(1, 0) < DORMANT_WARN_SHARE);
    let far = land_away(&g, home, 20.0);
    let d = run(&mut g, ClientMsg::Migrate { x: far.x, y: far.y, group: None }).unwrap();
    assert!(d["warning"].as_str().unwrap_or_default().contains("will bed down within about"), "{d}");
    // In the drought, drinking spots beside no lake don't count as lasting.
    let m = &g.world.map;
    let spot =
        (0..m.height).flat_map(|y| (0..m.width).map(move |x| evo_sim::terrain::Map::tile_center(x, y))).find(|&p| {
            let t = m.tile_at(p);
            t.water_depth == 0.0
                && g.world.water_check(1, 0, p, 3.0).drinkable >= LOW_WATER_TILES
                && g.lasting_water(p, 3.0) == 0
        });
    if let Some(p) = spot {
        assert!(g.lasting_water(p, 3.0) < g.world.water_check(1, 0, p, 3.0).drinkable);
    }
    // The journey's food is shared with the grazers along the way.
    let way = [home, far];
    let days = 3.0;
    for k in 0..g.world.map.tiles.len() {
        let max = g.world.map.ground()[k].veg_max;
        g.world.map.tiles[k].veg = 0.01 * max;
    }
    let alone = g.journey_food(1, 0, &way, days, None).expect("plant eaters");
    let bob_ids: Vec<usize> = (0..g.world.organisms.len()).filter(|&i| g.world.organisms[i].owner == 2).collect();
    for (k, &i) in bob_ids.iter().enumerate() {
        g.world.organisms[i].pos = home + (far - home) * (k as f32 / bob_ids.len() as f32);
    }
    let shared = g.journey_food(1, 0, &way, days, Some(1)).expect("plant eaters");
    assert!(shared.others > 0 && shared.route < alone.route, "{alone:?} {shared:?}");
    // And a crowded destination rates the trip.
    assert!(shared.share() <= shared.destination.unwrap() + 1e-6, "{shared:?}");
}

/// Playtest 9 (M15): a satellite group dwindling or dying says so; a claim
/// for a starving group says the hold loosens (and the sim's alert just
/// after doesn't repeat it).
#[test]
fn a_dwindling_group_and_a_starving_claim_say_so() {
    let (mut g, _) = game_with(GameConfig { claim_cooldown_min_minutes: 0.0, ..Default::default() });
    let d = run(&mut g, ClientMsg::SplitGroup { group: 0, organisms: None }).unwrap();
    let b = d["group"].as_u64().unwrap() as u8;
    let record = g.daily_record();
    let mut out = Outbox::new();
    g.daily_group_checks(&record, &mut out); // the baseline
    let members: Vec<usize> = (0..g.world.organisms.len())
        .filter(|&i| g.world.organisms[i].alive && g.world.organisms[i].group == b)
        .collect();
    for &i in &members[..members.len().div_ceil(2)] {
        g.world.organisms[i].alive = false;
    }
    g.daily_group_checks(&record, &mut out);
    let fell = |g: &Game| alerts(g, AlertKind::Group).into_iter().filter(|t| t.contains("has fallen from")).count();
    assert_eq!(fell(&g), 1, "{:?}", alerts(&g, AlertKind::Group));
    g.daily_group_checks(&record, &mut out);
    assert_eq!(fell(&g), 1, "said once");
    for &i in &members {
        g.world.organisms[i].alive = false;
    }
    g.daily_group_checks(&record, &mut out);
    assert!(
        alerts(&g, AlertKind::Group).iter().any(|t| t.contains("has died out")),
        "{:?}",
        alerts(&g, AlertKind::Group)
    );
    // A claim for the main group while a fifth of it is starving.
    let n = g.world.organisms.iter().filter(|o| o.alive && o.owner == 1 && o.group == 0).count();
    for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == 1 && o.group == 0).take(n.div_ceil(5)) {
        o.needs.hunger = 95.0;
    }
    let home = g.world.group_middle(1, 0).unwrap();
    let d = run(&mut g, ClientMsg::ClaimTerritory { group: 0, x: home.x, y: home.y, radius: 6.0 }).unwrap();
    assert!(d["warning"].as_str().unwrap_or_default().contains("starving or parched now"), "{d}");
    let before = alerts(&g, AlertKind::Group).len();
    for _ in 0..g.world.config.ticks_per_day / 4 {
        for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == 1 && o.group == 0).take(n.div_ceil(5)) {
            o.needs.hunger = 95.0;
        }
        tick(&mut g);
    }
    let after = alerts(&g, AlertKind::Group);
    assert!(!after[before..].iter().any(|t| t.contains("no longer feeds it")), "not again: {:?}", &after[before..]);
}

/// Playtest 9 (M11): the journey counts the hunger the group sets out with,
/// and a dry way warns.
#[test]
fn the_journey_counts_hunger_and_dry_ways() {
    let (mut g, _) = game();
    let home = g.world.group_middle(1, 0).unwrap();
    let far = land_away(&g, home, 25.0);
    let way = [home, far];
    let fed = g.journey_food(1, 0, &way, 3.0, None).expect("plant eaters").route;
    for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == 1) {
        o.needs.hunger = 90.0;
    }
    let hungry = g.journey_food(1, 0, &way, 3.0, None).expect("plant eaters").route;
    assert!(hungry < fed || fed >= 10.0, "{fed} {hungry}");
    // No water anywhere: the whole way is dry, and thirsty walkers are told.
    for t in &mut g.world.map.tiles {
        (t.water_depth, t.snow) = (0.0, 0.0);
    }
    for o in g.world.organisms.iter_mut().filter(|o| o.alive && o.owner == 1) {
        o.needs.thirst = 60.0;
    }
    let w = g.route_water(1, 0, &way);
    assert!(w.dry_tiles >= home.dist(far) - 1.0 && w.thirsty > 0.9, "{w:?}");
    let d = run(&mut g, ClientMsg::Migrate { x: far.x, y: far.y, group: None }).unwrap();
    assert!(d["warning"].as_str().unwrap_or_default().contains("with no drinking water"), "{d}");
}

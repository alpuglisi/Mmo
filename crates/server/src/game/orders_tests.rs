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
    let now = g.world.tick();
    let r: RallyView =
        serde_json::from_value(run(&mut g, ClientMsg::Rally { group: 0, x: 20.0, y: 20.0 }).unwrap()).unwrap();
    assert!(r.expires_tick - now >= 300, "{r:?}");
    let (mut fast, _) = game_with(GameConfig { tick_ms: 100.0, rally_min_minutes: 5.0, ..Default::default() });
    let now = fast.world.tick();
    let r: RallyView =
        serde_json::from_value(run(&mut fast, ClientMsg::Rally { group: 0, x: 20.0, y: 20.0 }).unwrap()).unwrap();
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
    // A newcomer's grace, with the days left.
    g.world.config.balance.attack_grace = 1e6;
    let out = g.command(10, 1, Some(2), msg.clone());
    match &out[0].1 {
        ServerMsg::Result { error: Some(e), .. } if e.code == "newcomer_grace" => {
            assert!(e.detail.as_ref().is_some_and(|d| d["days_left"].as_f64().is_some_and(|x| x > 1.0)), "{e:?}");
        }
        m => panic!("expected newcomer_grace, got {m:?}"),
    }
    g.world.config.balance.attack_grace = 0.0;
    let d = run(&mut g, msg).unwrap();
    assert_eq!((d["organism"].as_u64(), d["target"].as_u64()), (Some(a[0]), Some(b[0])));
    assert_eq!(g.world.attack_target(a[0]), Some(b[0]));
    let told: Vec<String> = g
        .alerts
        .get(&2)
        .map_or(Vec::new(), |l| l.iter().filter(|x| x.kind == AlertKind::Attack).map(|x| x.text.clone()).collect());
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains(&format!("#{}", b[0])) && told[0].contains("hunted"), "{told:?}");
    assert!(alerts(&g, AlertKind::Attack).is_empty(), "the attacker isn't alerted");
}

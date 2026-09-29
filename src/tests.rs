use macroquad::math::{vec3, Vec3};

use crate::map;
use crate::pmove::*;

fn step(world: &crate::collision::CollisionWorld, vars: &MoveVars, s: &mut PmState, cmd: UserCmd) {
    let mut pm = PlayerMove::new(world, vars, s, cmd);
    pm.run();
}

#[test]
fn stands_on_floor() {
    let m = map::load("surf_wars");
    let vars = MoveVars::surf_server();
    let mut s = PmState::new(vec3(-4000.0, 0.0, 2400.0 + 40.0));
    for _ in 0..200 {
        step(&m.world, &vars, &mut s, UserCmd { msec: 10, ..Default::default() });
    }
    assert!(s.onground, "not on ground: {:?}", s.origin);
    assert!((s.origin.z - 2436.0).abs() < 0.2, "z = {}", s.origin.z);
}

#[test]
fn walks_and_jumps() {
    let m = map::load("surf_wars");
    let vars = MoveVars::surf_server();
    let mut s = PmState::new(vec3(-4300.0, 200.0, 2440.0));
    let mut maxz: f32 = 0.0;
    for i in 0..150 {
        let buttons = if i == 100 { IN_JUMP } else { 0 };
        step(&m.world, &vars, &mut s, UserCmd { msec: 10, forwardmove: 400.0, buttons, ..Default::default() });
        maxz = maxz.max(s.origin.z);
    }
    let speed = vec3(s.velocity.x, s.velocity.y, 0.0).length();
    assert!(speed > 240.0 && speed <= 250.5, "speed {speed}");
    assert!(maxz > 2436.0 + 40.0 && maxz < 2436.0 + 50.0, "jump apex {}", maxz - 2436.0);
}

/// Drop a player on the north lane inner face and hold into the ramp.
#[test]
fn surfs_a_ramp() {
    let m = map::load("surf_wars");
    let vars = MoveVars::surf_server();
    // inner face of the north lane faces -y, ridge at y=1100
    let mut s = PmState::new(vec3(-3300.0, 850.0, 1850.0));
    s.velocity = vec3(400.0, 0.0, 0.0);
    let mut surf_ticks = 0;
    let mut min_x_speed = f32::MAX;
    for i in 0..500 {
        // Looking along +x, holding D (moveright) pushes toward -y... we want +y (into the ramp),
        // so hold A (moveleft = -sidemove) which is +y when looking +x.
        let cmd = UserCmd { msec: 10, sidemove: -400.0, viewangles: vec3(0.0, 0.0, 0.0), ..Default::default() };
        step(&m.world, &vars, &mut s, cmd);
        if s.is_surfing() { surf_ticks += 1; }
        if i > 50 { min_x_speed = min_x_speed.min(s.velocity.x); }
        if i % 50 == 0 {
            println!("t={:.2} pos={:?} vel={:?} speed={:.0} surf={} ridge={:.0}", i as f32 * 0.01, s.origin, s.velocity, s.velocity.length(), s.is_surfing(), map::sw_ridge(s.origin.x));
        }
    }
    println!("final pos={:?} vel={:?}", s.origin, s.velocity);
    assert!(surf_ticks > 400, "only surfed {surf_ticks} ticks");
    assert!(s.origin.x > 0.0, "did not travel along the lane: {:?}", s.origin);
    assert!(min_x_speed > 300.0);
}

#[test]
fn ray_hits_ramp() {
    let m = map::load("surf_wars");
    let tr = m.world.trace_ray(vec3(-2000.0, 0.0, 1500.0), vec3(-2000.0, 1200.0, 1500.0));
    assert!(tr.hit());
    assert!(tr.normal.y < -0.7 && tr.normal.z > 0.5, "{:?}", tr.normal);
    let _ = Vec3::ZERO;
}

#[test]
fn debug_ramp_contact() {
    let m = map::load("surf_wars");
    let vars = MoveVars::surf_server();
    let mut s = PmState::new(vec3(-3300.0, 850.0, 1850.0));
    s.velocity = vec3(400.0, 0.0, 0.0);
    for i in 0..110 {
        let cmd = UserCmd { msec: 10, sidemove: -400.0, ..Default::default() };
        let before = s;
        step(&m.world, &vars, &mut s, cmd);
        if i >= 85 {
            let tr = m.world.trace(before.origin, before.origin + before.velocity * 0.01, s.mins(), s.maxs());
            println!("i={} pos={:?} vel={:?} tr.frac={} n={:?} ss={} as={} brush={:?}", i, s.origin, s.velocity, tr.fraction, tr.normal, tr.startsolid, tr.allsolid, tr.brush);
        }
    }
}

/// Runs one bot on every T route and reports how far it gets.
#[test]
fn bot_routes() {
    use crate::game::*;
    for mapname in crate::map::map_names() {
        let settings = Settings { map: mapname.to_string(), player_team: None, bots_t: 1, bots_ct: 0, mode: Mode::Deathmatch, ..Default::default() };
        let probe = Game::new(settings.clone(), 1);
        let nroutes = probe.map.routes.len();
        for ri in 0..nroutes {
            if probe.map.routes[ri].team != crate::map::Team::T { continue; }
            let mut g = Game::new(settings.clone(), 7 + ri as u64);
            g.players[0].bot.as_mut().unwrap().force_route = Some(ri);
            g.players[0].bot.as_mut().unwrap().on_spawn();
            let mut max_wp = 0;
            let mut first_tp: Option<f64> = None;
            let mut max_speed: f32 = 0.0;
            let mut surf_ticks = 0;
            let mut log = String::new();
            for tick in 0..3000 {
                g.step(None);
                for e in g.events.drain(..) {
                    if let Event::Teleport { .. } = e { if first_tp.is_none() { first_tp = Some(g.time); } }
                }
                let p = &g.players[0];
                let b = p.bot.as_ref().unwrap();
                if first_tp.is_none() {
                    max_wp = max_wp.max(b.wp);
                    max_speed = max_speed.max(p.pm.velocity.length());
                    if p.pm.is_surfing() { surf_ticks += 1; }
                }
                if tick % 50 == 0 && first_tp.is_none() {
                    log += &format!("  t={:.1} wp={} pos=({:.0},{:.0},{:.0}) v=({:.0},{:.0},{:.0}) ground={} surf={}\n", g.time, b.wp, p.pm.origin.x, p.pm.origin.y, p.pm.origin.z, p.pm.velocity.x, p.pm.velocity.y, p.pm.velocity.z, p.pm.onground, p.pm.is_surfing());
                }
            }
            let r = &g.map.routes[ri];
            println!("{} / {:<24} reached wp {}/{} first_teleport={:?} max_speed={:.0} surf={:.1}s", mapname, r.name, max_wp, r.points.len(), first_tp.map(|t| (t*10.0).round()/10.0), max_speed, surf_ticks as f32 * 0.01);
            if std::env::var("BOTLOG").map_or(false, |v| r.name.contains(&v)) { println!("{log}"); }
        }
    }
}

#[test]
fn debug_slowdown() {
    use crate::game::*;
    let settings = Settings { map: "surf_wars".into(), player_team: None, bots_t: 1, bots_ct: 0, mode: Mode::Deathmatch, ..Default::default() };
    let mut g = Game::new(settings, 7);
    g.players[0].bot.as_mut().unwrap().force_route = Some(0);
    g.players[0].bot.as_mut().unwrap().on_spawn();
    let mut last_speed = 0.0;
    for _ in 0..1300 {
        g.step(None);
        let p = &g.players[0];
        let sp = p.pm.velocity.length();
        if last_speed - sp > 40.0 {
            println!("t={:.2} pos={:?} v={:?} sp {} -> {} surfn={:?}", g.time, p.pm.origin, p.pm.velocity, last_speed, sp, p.pm.surf_normal);
        }
        last_speed = sp;
    }
}

#[test]
fn debug_seam() {
    use crate::game::*;
    let settings = Settings { map: "surf_wars".into(), player_team: None, bots_t: 1, bots_ct: 0, mode: Mode::Deathmatch, ..Default::default() };
    let mut g = Game::new(settings, 8);
    let ri = g.map.routes.iter().position(|r| r.name == "lane north outer").unwrap();
    g.players[0].bot.as_mut().unwrap().force_route = Some(ri);
    g.players[0].bot.as_mut().unwrap().on_spawn();
    let mut last_speed = 0.0;
    for _ in 0..1600 {
        let before = g.players[0].pm;
        g.step(None);
        let p = &g.players[0];
        let sp = p.pm.velocity.length();
        if last_speed - sp > 100.0 {
            let tr = g.map.world.trace(before.origin, before.origin + before.velocity * 0.01, before.mins(), before.maxs());
            println!("t={:.2} pos={:?} v={:?} sp {} -> {} | before v={:?} tr frac={} n={:?} brush={:?} ss={}", g.time, p.pm.origin, p.pm.velocity, last_speed, sp, before.velocity, tr.fraction, tr.normal, tr.brush, tr.startsolid);
            if let Some(bi) = tr.brush { let b = &g.map.world.brushes[bi]; println!("   brush mins={:?} maxs={:?}", b.mins, b.maxs); }
        }
        last_speed = sp;
    }
}

/// Full bot match: checks that fights happen and rounds end.
#[test]
fn bot_match() {
    use crate::game::*;
    for mapname in crate::map::map_names() {
        let settings = Settings { map: mapname.to_string(), player_team: None, bots_t: 5, bots_ct: 5, mode: Mode::Rounds, ..Default::default() };
        let mut g = Game::new(settings, 42);
        let mut shots = 0;
        let mut hits = 0;
        let mut kills = 0;
        let mut teleports = 0;
        let mut rounds = 0;
        let start = std::time::Instant::now();
        let secs = 300.0;
        while g.time < secs {
            g.step(None);
            for e in g.events.drain(..) {
                match e {
                    Event::Shot { .. } => shots += 1,
                    Event::Hit { .. } => hits += 1,
                    Event::Kill { .. } => kills += 1,
                    Event::Teleport { .. } => teleports += 1,
                    Event::RoundEnd { .. } => rounds += 1,
                    _ => {}
                }
            }
        }
        let el = start.elapsed().as_secs_f64();
        println!("{mapname}: {secs}s sim in {el:.2}s ({:.0}x realtime) rounds={rounds} score={:?} shots={shots} hits={hits} kills={kills} teleports={teleports}", secs / el, g.score);
        for p in &g.players { println!("   {:<16} {:?} k={} d={}", p.name, p.team, p.kills, p.deaths); }
        assert!(rounds >= 2 && kills > 5);
    }
}

/// A human player that just stands there, in both game modes.
#[test]
fn idle_human_long_run() {
    use crate::game::*;
    for mode in [Mode::Rounds, Mode::Deathmatch] {
        for mapname in crate::map::map_names() {
            let settings = Settings { map: mapname.to_string(), player_team: Some(crate::map::Team::CT), bots_t: 5, bots_ct: 4, mode, ..Default::default() };
            let mut g = Game::new(settings, 3);
            let mut rounds = 0;
            let mut local_deaths = 0;
            while g.time < 600.0 {
                let angles = g.players[0].angles;
                g.step(Some(crate::pmove::UserCmd { msec: 10, viewangles: angles, ..Default::default() }));
                for e in g.events.drain(..) {
                    match e {
                        Event::RoundEnd { .. } => rounds += 1,
                        Event::Kill { victim: 0, .. } => local_deaths += 1,
                        _ => {}
                    }
                }
            }
            let kills: i32 = g.players.iter().map(|p| p.kills).sum();
            println!("{mapname} {:?}: rounds={rounds} score={:?} total kills={kills} local deaths={local_deaths}", mode, g.score);
            assert!(kills > 10);
        }
    }
}

#[test]
fn shot_stats() {
    use crate::game::*;
    use std::collections::HashMap;
    for mapname in crate::map::map_names() {
        let settings = Settings { map: mapname.to_string(), player_team: None, bots_t: 5, bots_ct: 5, mode: Mode::Deathmatch, ..Default::default() };
        let mut g = Game::new(settings, 11);
        let mut stats: HashMap<(String, bool), (u32, u32, f32)> = HashMap::new();
        let mut kills_by: HashMap<String, u32> = HashMap::new();
        let mut hold_time = 0.0;
        let mut surf_time = 0.0;
        let mut ground_time = 0.0;
        let mut air_time = 0.0;
        while g.time < 600.0 {
            // who is shooting from where
            let before: Vec<(bool, u32)> = g.players.iter().map(|p| (p.pm.onground, p.weapon().map_or(0, |w| w.clip))).collect();
            g.step(None);
            for p in &g.players { if !p.alive { continue; } if p.pm.onground { ground_time += 0.01 } else if p.pm.is_surfing() { surf_time += 0.01 } else { air_time += 0.01 } }
            let _ = &mut hold_time;
            let mut shots_this_tick: Vec<usize> = vec![];
            for e in g.events.drain(..) {
                match e {
                    Event::Shot { player, weapon, .. } => { shots_this_tick.push(player); let k = (format!("{:?}", weapon), before[player].0); let s = stats.entry(k).or_default(); s.0 += 1; }
                    Event::Hit { attacker, .. } => { let p = &g.players[attacker]; let k = (format!("{:?}", p.active_id()), before[attacker].0); let s = stats.entry(k).or_default(); s.1 += 1; }
                    Event::Kill { weapon: Some(w), .. } => { *kills_by.entry(format!("{:?}", w)).or_default() += 1; }
                    _ => {}
                }
            }
        }
        println!("== {mapname}: ground {:.0}s surf {:.0}s air {:.0}s (player-seconds)", ground_time, surf_time, air_time);
        let mut v: Vec<_> = stats.into_iter().collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        for ((w, ground), (shots, hits, _)) in v { println!("  {:<6} {:<7} shots {:>5} hits {:>4} ({:.0}%)", w, if ground {"ground"} else {"air"}, shots, hits, hits as f32 * 100.0 / shots.max(1) as f32); }
        println!("  kills by weapon: {:?}", kills_by);
    }
}

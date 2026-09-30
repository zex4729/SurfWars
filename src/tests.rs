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
    // stock ramp behaviour: with a raised ramp climb, holding into the ramp
    // climbs over the ridge (see ramp_climb_lifts_surfers)
    let vars = MoveVars { ramp_climb: 30.0, ..MoveVars::surf_server() };
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
        if s.is_surfing() {
            surf_ticks += 1;
        }
        if i > 50 {
            min_x_speed = min_x_speed.min(s.velocity.x);
        }
        if i % 50 == 0 {
            println!(
                "t={:.2} pos={:?} vel={:?} speed={:.0} surf={} ridge={:.0}",
                i as f32 * 0.01,
                s.origin,
                s.velocity,
                s.velocity.length(),
                s.is_surfing(),
                map::sw_ridge(s.origin.x)
            );
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

/// Runs one bot on every T route and reports how far it gets.
#[test]
fn bot_routes() {
    use crate::game::*;
    for mapname in crate::map::map_names() {
        let settings = Settings {
            map: mapname.to_string(),
            player_team: None,
            bots_t: 1,
            bots_ct: 0,
            mode: Mode::Deathmatch,
            ..Default::default()
        };
        let probe = Game::new(settings.clone(), 1);
        let nroutes = probe.map.routes.len();
        for ri in 0..nroutes {
            if probe.map.routes[ri].team != crate::map::Team::T {
                continue;
            }
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
                    if let Event::Teleport { .. } = e {
                        if first_tp.is_none() {
                            first_tp = Some(g.time);
                        }
                    }
                }
                let p = &g.players[0];
                let b = p.bot.as_ref().unwrap();
                if first_tp.is_none() {
                    max_wp = max_wp.max(b.wp);
                    max_speed = max_speed.max(p.pm.velocity.length());
                    if p.pm.is_surfing() {
                        surf_ticks += 1;
                    }
                }
                if tick % 50 == 0 && first_tp.is_none() {
                    log += &format!(
                        "  t={:.1} wp={} pos=({:.0},{:.0},{:.0}) v=({:.0},{:.0},{:.0}) ground={} surf={}\n",
                        g.time,
                        b.wp,
                        p.pm.origin.x,
                        p.pm.origin.y,
                        p.pm.origin.z,
                        p.pm.velocity.x,
                        p.pm.velocity.y,
                        p.pm.velocity.z,
                        p.pm.onground,
                        p.pm.is_surfing()
                    );
                }
            }
            let r = &g.map.routes[ri];
            println!(
                "{} / {:<24} reached wp {}/{} first_teleport={:?} max_speed={:.0} surf={:.1}s",
                mapname,
                r.name,
                max_wp,
                r.points.len(),
                first_tp.map(|t| (t * 10.0).round() / 10.0),
                max_speed,
                surf_ticks as f32 * 0.01
            );
            // The bot must get through the route (the last point may be a
            // hold spot it is still camping at).
            assert!(max_wp + 1 >= r.points.len(), "{} / {}: stopped at waypoint {}", mapname, r.name, max_wp);
            if std::env::var("BOTLOG").map_or(false, |v| r.name.contains(&v)) {
                println!("{log}");
            }
        }
    }
}

/// Regression test: crossing the seam between two ramp segments must not
/// act like a wall (the entry plane has to be picked by its exact crossing).
#[test]
fn ramp_seams_do_not_stop_surfers() {
    use crate::game::*;
    let settings = Settings {
        map: "surf_wars".into(),
        player_team: None,
        bots_t: 1,
        bots_ct: 0,
        mode: Mode::Deathmatch,
        ..Default::default()
    };
    for name in ["lane north outer", "lane north inner", "lane south outer", "lane south inner"] {
        let mut g = Game::new(settings.clone(), 8);
        let ri = g.map.routes.iter().position(|r| r.name == name && r.team == crate::map::Team::T).unwrap();
        g.players[0].bot.as_mut().unwrap().force_route = Some(ri);
        g.players[0].bot.as_mut().unwrap().on_spawn();
        let mut last = 0.0f32;
        for _ in 0..1600 {
            g.step(None);
            let p = &g.players[0];
            let sp = p.pm.velocity.length();
            if p.pm.is_surfing() {
                assert!(last - sp < 300.0, "{name}: speed dropped {last} -> {sp} at {:?}", p.pm.origin);
            }
            last = sp;
        }
    }
}

/// Full bot match: checks that fights happen and rounds end.
#[test]
fn bot_match() {
    use crate::game::*;
    for mapname in crate::map::map_names() {
        let settings = Settings {
            map: mapname.to_string(),
            player_team: None,
            bots_t: 5,
            bots_ct: 5,
            mode: Mode::Rounds,
            ..Default::default()
        };
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
        for p in &g.players {
            println!("   {:<16} {:?} k={} d={}", p.name, p.team, p.kills, p.deaths);
        }
        assert!(rounds >= 1 && kills > 5);
    }
}

/// A human player that just stands there, in both game modes.
#[test]
fn idle_human_long_run() {
    use crate::game::*;
    for mode in [Mode::Rounds, Mode::Deathmatch] {
        for mapname in crate::map::map_names() {
            let settings = Settings {
                map: mapname.to_string(),
                player_team: Some(crate::map::Team::CT),
                bots_t: 5,
                bots_ct: 4,
                mode,
                ..Default::default()
            };
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
            println!(
                "{mapname} {:?}: rounds={rounds} score={:?} total kills={kills} local deaths={local_deaths}",
                mode, g.score
            );
            assert!(kills > 10);
        }
    }
}

#[test]
fn shot_stats() {
    use crate::game::*;
    use std::collections::HashMap;
    for mapname in crate::map::map_names() {
        let settings = Settings {
            map: mapname.to_string(),
            player_team: None,
            bots_t: 5,
            bots_ct: 5,
            mode: Mode::Deathmatch,
            ..Default::default()
        };
        let mut g = Game::new(settings, 11);
        let mut stats: HashMap<(String, bool), (u32, u32, f32)> = HashMap::new();
        let mut kills_by: HashMap<String, u32> = HashMap::new();
        let mut hold_time = 0.0;
        let mut surf_time = 0.0;
        let mut ground_time = 0.0;
        let mut air_time = 0.0;
        while g.time < 600.0 {
            // who is shooting from where
            let before: Vec<(bool, u32)> =
                g.players.iter().map(|p| (p.pm.onground, p.weapon().map_or(0, |w| w.clip))).collect();
            g.step(None);
            for p in &g.players {
                if !p.alive {
                    continue;
                }
                if p.pm.onground {
                    ground_time += 0.01
                } else if p.pm.is_surfing() {
                    surf_time += 0.01
                } else {
                    air_time += 0.01
                }
            }
            let _ = &mut hold_time;
            let mut shots_this_tick: Vec<usize> = vec![];
            for e in g.events.drain(..) {
                match e {
                    Event::Shot { player, weapon, .. } => {
                        shots_this_tick.push(player);
                        let k = (format!("{:?}", weapon), before[player].0);
                        let s = stats.entry(k).or_default();
                        s.0 += 1;
                    }
                    Event::Hit { attacker, .. } => {
                        let p = &g.players[attacker];
                        let k = (format!("{:?}", p.active_id()), before[attacker].0);
                        let s = stats.entry(k).or_default();
                        s.1 += 1;
                    }
                    Event::Kill { weapon: Some(w), .. } => {
                        *kills_by.entry(format!("{:?}", w)).or_default() += 1;
                    }
                    _ => {}
                }
            }
        }
        println!(
            "== {mapname}: ground {:.0}s surf {:.0}s air {:.0}s (player-seconds)",
            ground_time, surf_time, air_time
        );
        let mut v: Vec<_> = stats.into_iter().collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        for ((w, ground), (shots, hits, _)) in v {
            println!(
                "  {:<6} {:<7} shots {:>5} hits {:>4} ({:.0}%)",
                w,
                if ground { "ground" } else { "air" },
                shots,
                hits,
                hits as f32 * 100.0 / shots.max(1) as f32
            );
        }
        println!("  kills by weapon: {:?}", kills_by);
    }
}

mod weapon_tests {
    use crate::game::*;
    use crate::map::Team;
    use crate::pmove::*;
    use crate::weapons::*;
    use macroquad::math::vec3;

    /// Human (T) and one CT bot standing on the T spawn, 300 units apart.
    fn duel() -> Game {
        let s = Settings {
            player_team: Some(Team::T),
            bots_t: 0,
            bots_ct: 1,
            mode: Mode::Deathmatch,
            ..Default::default()
        };
        let mut g = Game::new(s, 5);
        // let the freeze time pass and put both on the T spawn platform
        g.players[0].pm = PmState::new(vec3(-4200.0, -300.0, 2436.0));
        g.players[1].pm = PmState::new(vec3(-3900.0, -300.0, 2436.0));
        g.players[1].bot = None; // make the target a dummy
        g
    }

    fn cmd(g: &Game, buttons: u32) -> UserCmd {
        // aim at the dummy's chest
        let me = g.players[0].pm.eye();
        let target = g.players[1].pm.origin + vec3(0.0, 0.0, 10.0);
        let (pitch, yaw) = crate::util::vec_to_angles(target - me);
        UserCmd { msec: 10, viewangles: vec3(pitch, yaw, 0.0), buttons, ..Default::default() }
    }

    #[test]
    fn ak_kills_and_reloads() {
        let mut g = duel();
        g.give(0, WeaponId::Ak47);
        let mut kills = 0;
        let mut shots = 0;
        for _ in 0..400 {
            let c = cmd(&g, IN_ATTACK);
            g.step(Some(c));
            for e in g.events.drain(..) {
                match e {
                    Event::Shot { player: 0, .. } => shots += 1,
                    Event::Kill { victim: 1, .. } => kills += 1,
                    _ => {}
                }
            }
            if kills > 0 {
                break;
            }
        }
        assert_eq!(kills, 1, "AK did not kill a stationary target at 300 units ({shots} shots)");
        assert!(shots <= 12, "took {shots} shots");
        // empty the magazine, then let go and check the automatic reload
        let mut emptied = false;
        for _ in 0..600 {
            let mut c = cmd(&g, IN_ATTACK);
            if g.players[0].weapon().unwrap().clip == 0 {
                emptied = true;
            }
            if emptied {
                c.buttons = 0;
            }
            g.step(Some(c));
        }
        let w = g.players[0].weapon().unwrap();
        assert_eq!(w.clip, 30);
        assert!(w.reserve < 90);
    }

    #[test]
    fn usp_is_semi_auto() {
        let mut g = duel();
        let mut shots = 0;
        for _ in 0..200 {
            let c = cmd(&g, IN_ATTACK);
            g.step(Some(c));
            shots += g.events.drain(..).filter(|e| matches!(e, Event::Shot { player: 0, .. })).count();
        }
        assert_eq!(shots, 1, "holding the trigger fired {shots} USP shots");
    }

    #[test]
    fn awp_scope_resumes_after_shot() {
        let mut g = duel();
        g.players[1].health = 1000.0; // keep the dummy alive
        g.give(0, WeaponId::Awp);
        for _ in 0..150 {
            let c = cmd(&g, 0);
            g.step(Some(c));
        }
        let c = cmd(&g, IN_ATTACK2);
        g.step(Some(c));
        assert_eq!(g.players[0].zoom, 1);
        assert_eq!(g.players[0].fov(), 40.0);
        let c = cmd(&g, IN_ATTACK);
        g.step(Some(c));
        assert_eq!(g.players[0].zoom, 0, "the AWP unzooms while the bolt cycles");
        for _ in 0..160 {
            let c = cmd(&g, 0);
            g.step(Some(c));
        }
        assert_eq!(g.players[0].zoom, 1, "the zoom comes back after the bolt");
    }

    #[test]
    fn m3_reloads_shell_by_shell() {
        let mut g = duel();
        g.players[1].health = 10000.0;
        assert!(g.buy(0, WeaponId::M3));
        for _ in 0..150 {
            let c = cmd(&g, 0);
            g.step(Some(c));
        }
        let mut pellets_hit = 0;
        let c = cmd(&g, IN_ATTACK);
        g.step(Some(c));
        pellets_hit += g.events.drain(..).filter(|e| matches!(e, Event::Hit { .. })).count();
        assert!(pellets_hit >= 3, "only {pellets_hit} pellets hit at 300 units");
        assert_eq!(g.players[0].weapon().unwrap().clip, 7);
        let c = cmd(&g, IN_RELOAD);
        for _ in 0..200 {
            g.step(Some(UserCmd { buttons: IN_RELOAD, ..c }));
        }
        let clip = g.players[0].weapon().unwrap().clip;
        assert_eq!(clip, 8);
    }

    #[test]
    fn headshot_multiplier() {
        let mut g = duel();
        g.give(0, WeaponId::Scout);
        for _ in 0..150 {
            let c = cmd(&g, 0);
            g.step(Some(c));
        }
        // aim at the head
        let me = g.players[0].pm.eye();
        let head = g.players[1].pm.origin + vec3(0.0, 0.0, 27.0);
        let (pitch, yaw) = crate::util::vec_to_angles(head - me);
        g.step(Some(UserCmd {
            msec: 10,
            viewangles: vec3(pitch, yaw, 0.0),
            buttons: IN_ATTACK2,
            ..Default::default()
        }));
        g.step(Some(UserCmd { msec: 10, viewangles: vec3(pitch, yaw, 0.0), ..Default::default() }));
        g.step(Some(UserCmd { msec: 10, viewangles: vec3(pitch, yaw, 0.0), buttons: IN_ATTACK, ..Default::default() }));
        let hs = g.events.drain(..).any(|e| matches!(e, Event::Kill { victim: 1, headshot: true, .. }));
        assert!(hs, "a scoped scout headshot should kill through a helmet");
    }
}

/// Stock CS caps bunny hop speed on jump (PM_PreventMegaBunnyJumping); surf
/// servers remove that.
#[test]
fn bhop_cap_only_on_stock_settings() {
    let m = map::load("surf_wars");
    for (vars, capped) in [(MoveVars::stock(), true), (MoveVars::surf_server(), false)] {
        let mut s = PmState::new(vec3(-4300.0, 200.0, 2436.0));
        s.velocity = vec3(500.0, 0.0, 0.0);
        step(&m.world, &vars, &mut s, UserCmd { msec: 10, buttons: IN_JUMP, ..Default::default() });
        let speed = vec3(s.velocity.x, s.velocity.y, 0.0).length();
        if capped {
            // 500 > 1.2 * 250, so the velocity is scaled by 300 / 500 * 0.65
            assert!((speed - 500.0 * 300.0 / 500.0 * 0.65).abs() < 5.0, "stock speed {speed}");
        } else {
            assert!(speed > 495.0, "surf speed {speed}");
        }
    }
}

#[test]
fn only_basic_guns_are_for_sale() {
    use crate::game::*;
    use crate::weapons::WeaponId;
    let s = Settings { player_team: Some(crate::map::Team::T), bots_t: 0, bots_ct: 0, ..Default::default() };
    let mut g = Game::new(s, 1);
    for id in [WeaponId::Usp, WeaponId::M3, WeaponId::Mp5] {
        assert!(g.buy(0, id));
    }
    for id in [WeaponId::Ak47, WeaponId::Awp, WeaponId::Scout, WeaponId::Laser, WeaponId::Rocket] {
        assert!(!g.buy(0, id));
    }
}

#[test]
fn map_files_round_trip() {
    for name in crate::map::BUILTIN_MAPS {
        let m = map::load(name);
        let text = crate::mapfile::to_text(&m);
        let back = crate::mapfile::from_text(&text).unwrap();
        assert_eq!(back.world.brushes.len(), m.world.brushes.len(), "{name}");
        assert_eq!(back.pickups.len(), m.pickups.len());
        assert_eq!(back.spawns[0].len(), m.spawns[0].len());
        for (a, b) in m.world.brushes.iter().zip(back.world.brushes.iter()) {
            assert!((a.mins - b.mins).length() < 0.1 && (a.maxs - b.maxs).length() < 0.1);
        }
    }
}

/// Rockets explode, hurt, and knock players around (rocket jumps).
#[test]
fn rocket_splash_and_knockback() {
    use crate::game::*;
    use crate::weapons::WeaponId;
    let s = Settings {
        player_team: Some(crate::map::Team::T),
        bots_t: 0,
        bots_ct: 1,
        mode: Mode::Deathmatch,
        ..Default::default()
    };
    let mut g = Game::new(s, 5);
    g.players[0].pm = PmState::new(vec3(-4200.0, -300.0, 2436.0));
    g.players[1].pm = PmState::new(vec3(-3900.0, -300.0, 2436.0));
    g.players[1].bot = None;
    g.give(0, WeaponId::Rocket);
    for _ in 0..150 {
        g.step(Some(UserCmd { msec: 10, viewangles: g.players[0].angles, ..Default::default() }));
    }
    // aim at the dummy's feet
    let (p, y) = crate::util::vec_to_angles(g.players[1].pm.origin - vec3(0.0, 0.0, 30.0) - g.players[0].pm.eye());
    let mut exploded = false;
    let hp0 = g.players[1].health;
    for i in 0..100 {
        let buttons = if i == 0 { IN_ATTACK } else { 0 };
        g.step(Some(UserCmd { msec: 10, viewangles: vec3(p, y, 0.0), buttons, ..Default::default() }));
        exploded |= g.events.drain(..).any(|e| matches!(e, Event::Explosion { .. }));
    }
    assert!(exploded);
    assert!(g.players[1].health < hp0 || !g.players[1].alive);
    // rocket jump: fire at the floor under our own feet
    g.players[0].pm = PmState::new(vec3(-4200.0, 300.0, 2436.0));
    for _ in 0..100 {
        g.step(Some(UserCmd { msec: 10, viewangles: vec3(89.0, 0.0, 0.0), ..Default::default() }));
    }
    let mut max_vz: f32 = 0.0;
    for i in 0..60 {
        let buttons = if i == 0 { IN_ATTACK } else { 0 };
        g.step(Some(UserCmd { msec: 10, viewangles: vec3(89.0, 0.0, 0.0), buttons, ..Default::default() }));
        max_vz = max_vz.max(g.players[0].pm.velocity.z);
    }
    assert!(max_vz > 400.0, "rocket jump only reached vz {max_vz}");
}

/// Attachments change weapon behaviour the way their descriptions say.
#[test]
fn attachments_modify_weapons() {
    use crate::weapons::*;
    let heavy = Attachments { stock: Stock::Heavy, grip: Grip::Vertical, ..Default::default() }.mods();
    assert!(heavy.recoil < 0.7 && heavy.speed < 0.0);
    let acog = Weapon::with(WeaponId::Mp5, Attachments { sight: Sight::Acog, ..Default::default() });
    assert_eq!(acog.zoom_levels(), 1);
    assert!(acog.is_ads(1) && acog.zoom_fov(1) < 50.0);
    let supp = Attachments { muzzle: Muzzle::Suppressor, ..Default::default() }.mods();
    assert!(supp.silenced && supp.damage < 1.0);
    // an AWP keeps its own scope levels whatever sight is chosen
    let awp = Weapon::with(WeaponId::Awp, Attachments { sight: Sight::RedDot, ..Default::default() });
    assert_eq!(awp.zoom_levels(), 2);
    assert!(!awp.is_ads(1));
}

/// Health packs heal and then respawn later.
#[test]
fn health_pickup() {
    use crate::game::*;
    let s = Settings {
        player_team: Some(crate::map::Team::T),
        bots_t: 0,
        bots_ct: 0,
        mode: Mode::Deathmatch,
        ..Default::default()
    };
    let mut g = Game::new(s, 5);
    let k = g.pickups.iter().position(|p| p.def.kind == crate::map::PickupKind::Health).unwrap();
    let pos = g.pickups[k].def.pos;
    g.players[0].health = 30.0;
    g.players[0].pm = PmState::new(pos + vec3(0.0, 0.0, 20.0));
    g.step(Some(UserCmd { msec: 10, ..Default::default() }));
    assert!(g.players[0].health >= 79.0, "health {}", g.players[0].health);
    assert!(g.pickups[k].available_at > g.time);
}

/// Builds a map with every editor tool, saves it, loads it back and plays
/// a bot match on it (bots roam since editor maps have no routes).
#[test]
fn editor_build_save_play() {
    use crate::editor::Editor;
    use crate::game::*;
    let mut ed = Editor::new(None);
    let n0 = ed.game.map.world.brushes.len();
    ed.look_from(vec3(0.0, -1500.0, 2600.0), vec3(35.0, 90.0, 0.0));
    for k in ["box", "slope", "ramp", "pillar", "turn90", "turn180"] {
        ed.add_shape(k);
    }
    assert!(ed.game.map.world.brushes.len() > n0 + 5 + 24);
    let b = ed.selected_brush().unwrap();
    let before = ed.game.map.world.brushes[b].mins;
    ed.move_sel(vec3(64.0, 0.0, 0.0));
    assert!((ed.game.map.world.brushes[b].mins.x - before.x - 64.0).abs() < 0.5);
    ed.resize_sel(2, 64.0);
    ed.rotate_sel(15.0);
    ed.duplicate_sel();
    ed.delete_sel();
    ed.restyle_sel(true);
    ed.add_spawn(crate::map::Team::T);
    ed.add_spawn(crate::map::Team::CT);
    for k in 0..9 {
        ed.set_pickup_kind(k);
        ed.add_pickup();
    }
    ed.game.map.name = "test_editor_map".into();
    let text = crate::mapfile::to_text(&ed.game.map);
    let map = crate::mapfile::from_text(&text).unwrap();
    assert_eq!(map.world.brushes.len(), ed.game.map.world.brushes.len());
    assert!(map.pickups.iter().any(|p| p.kind == crate::map::PickupKind::Weapon(crate::weapons::WeaponId::Rocket)));
    assert!(map.pickups.iter().any(|p| p.kind == crate::map::PickupKind::Weapon(crate::weapons::WeaponId::Laser)));
    let s = Settings { player_team: None, bots_t: 3, bots_ct: 3, mode: Mode::Deathmatch, ..Default::default() };
    let mut g = Game::with_map(map, s, 9);
    let mut moved = 0.0;
    for _ in 0..6000 {
        let before: Vec<Vec3> = g.players.iter().map(|p| p.pm.origin).collect();
        g.step(None);
        moved += g.players.iter().zip(before).map(|(p, b)| (p.pm.origin - b).length().min(100.0)).sum::<f32>();
        g.events.clear();
    }
    assert!(moved > 10000.0, "bots barely moved on an editor map: {moved}");
}

/// Holding into a ramp climbs it only with a raised ramp climb cap.
#[test]
fn ramp_climb_lifts_surfers() {
    let m = map::load("surf_wars");
    let mut gains = Vec::new();
    for climb in [30.0, 100.0, 160.0] {
        let vars = MoveVars { ramp_climb: climb, ..MoveVars::surf_server() };
        let mut s = PmState::new(vec3(-3300.0, 850.0, 1850.0));
        s.velocity = vec3(900.0, 0.0, 0.0);
        for _ in 0..250 {
            let cmd = UserCmd { msec: 10, sidemove: -400.0, ..Default::default() };
            step(&m.world, &vars, &mut s, cmd);
        }
        // height relative to the ridge line after gliding down the lane
        let h = s.origin.z - map::sw_ridge(s.origin.x);
        println!("ramp_climb {climb}: height over ridge {h:.0}, final {:?} vel {:?}", s.origin, s.velocity);
        gains.push(h);
    }
    assert!(gains[1] > gains[0] + 60.0, "{gains:?}");
    assert!(gains[2] > gains[1], "{gains:?}");
}

/// Scripted surfer: look along `yaw` and hold into whatever ramp we touch.
fn surf_cmd(s: &PmState, yaw: f32) -> UserCmd {
    let mut cmd = UserCmd { msec: 10, viewangles: vec3(0.0, yaw, 0.0), ..Default::default() };
    if s.surf_time < 0.3 {
        let n = s.surf_normal;
        let wish = -vec3(n.x, n.y, 0.0).normalize_or_zero();
        let (_, r, _) = angle_vectors(cmd.viewangles);
        cmd.sidemove = 400.0 * wish.dot(r).signum();
    }
    cmd
}

fn solo_game(map: &str) -> crate::game::Game {
    use crate::game::*;
    let s = Settings {
        map: map.into(),
        player_team: Some(crate::map::Team::T),
        bots_t: 0,
        bots_ct: 0,
        mode: Mode::Deathmatch,
        ..Default::default()
    };
    let mut s = s;
    if let Some(c) = std::env::var("RAMP_CLIMB").ok().and_then(|v| v.parse().ok()) {
        s.vars.ramp_climb = c;
    }
    Game::new(s, 3)
}

/// Every sky ramp: the launch pad throws you onto the ramp, the boosters
/// carry you up, and you land on the sky platform.
#[test]
fn sky_ramps_reach_platforms() {
    for name in map::BUILTIN_MAPS {
        let g0 = solo_game(name);
        let skies = g0.map.sky.clone();
        assert!(skies.len() == 2, "{name} has {} sky ramps", skies.len());
        for (k, sky) in skies.iter().enumerate() {
            let mut g = solo_game(name);
            g.players[0].pm = PmState::new(sky.pad + vec3(0.0, 0.0, 38.0));
            let yaw = if sky.dir > 0.0 { 0.0 } else { 180.0 };
            let mut reached = None;
            let mut maxz = 0.0f32;
            let mut max_speed = 0.0f32;
            let mut last = Vec3::ZERO;
            for t in 0..3000 {
                let cmd = surf_cmd(&g.players[0].pm, yaw);
                g.step(Some(cmd));
                g.events.clear();
                let p = &g.players[0].pm;
                maxz = maxz.max(p.origin.z);
                max_speed = max_speed.max(p.velocity.length());
                let feet = p.origin + vec3(0.0, 0.0, p.mins().z);
                let pl = sky.platform;
                if p.onground
                    && feet.x >= pl.mins.x - 16.0
                    && feet.x <= pl.maxs.x + 16.0
                    && feet.y >= pl.mins.y - 16.0
                    && feet.y <= pl.maxs.y + 16.0
                    && (feet.z - pl.maxs.z).abs() < 4.0
                {
                    reached = Some(t as f32 * 0.01);
                    break;
                }
                if t % 100 == 0 && std::env::var("SKYLOG").is_ok() {
                    println!(
                        "  t={:.1} pos={:.0?} vel={:.0?} surf={}",
                        t as f32 * 0.01,
                        p.origin,
                        p.velocity,
                        p.is_surfing()
                    );
                }
                last = p.origin;
            }
            println!(
                "{name} sky {k}: reached={reached:?} max_z={maxz:.0} max_speed={max_speed:.0} platform_top={:.0} last={last:.0?}",
                sky.platform.maxs.z
            );
            assert!(reached.is_some(), "{name} sky ramp {k} does not reach its platform");
        }
    }
}

/// Spawn launch pads throw you onto a ramp.
#[test]
fn spawn_launchers_land_on_ramps() {
    for name in map::BUILTIN_MAPS {
        let g0 = solo_game(name);
        let sky_pads: Vec<Vec3> = g0.map.sky.iter().map(|s| s.pad).collect();
        let pads: Vec<Vec3> = g0
            .map
            .boosters
            .iter()
            .filter(|b| matches!(b.push, crate::map::Push::Launch { .. }))
            .map(|b| b.pad())
            .filter(|p| !sky_pads.iter().any(|q| q.distance(*p) < 1.0))
            .collect();
        assert!(!pads.is_empty(), "{name} has no spawn launch pads");
        for pad in pads {
            let mut g = solo_game(name);
            g.players[0].pm = PmState::new(pad + vec3(0.0, 0.0, 38.0));
            let mut surfed = 0;
            let mut teleported = false;
            for _ in 0..400 {
                let pm = g.players[0].pm;
                let yaw = pm.velocity.y.atan2(pm.velocity.x).to_degrees();
                g.step(Some(surf_cmd(&pm, yaw)));
                if g.events.iter().any(|e| matches!(e, crate::game::Event::Teleport { .. })) {
                    teleported = true;
                }
                g.events.clear();
                if g.players[0].pm.is_surfing() {
                    surfed += 1;
                }
                if std::env::var("PADLOG").is_ok() && g.tick % 25 == 0 {
                    let p = &g.players[0].pm;
                    println!("  pos={:.0?} vel={:.0?} surf={}", p.origin, p.velocity, p.is_surfing());
                }
            }
            println!("{name} pad {pad:.0?}: surfed {surfed} ticks, teleported {teleported}");
            assert!(surfed > 30 && !teleported, "{name} pad {pad:?}");
        }
    }
}

/// Gizmo and 2D view drags move and resize on the grid; boosters and launch
/// pads survive a save and load.
#[test]
fn editor_drags_and_boosters() {
    use crate::editor::Editor;
    use macroquad::math::vec2;
    let mut ed = Editor::new(None);
    ed.look_from(vec3(0.0, -1500.0, 2600.0), vec3(35.0, 90.0, 0.0));
    ed.add_shape("box");
    let (a0, b0) = ed.bounds().unwrap();
    // gizmo drag along Z by 70 units snaps to 64 (grid 32)
    ed.drag_axis(2, 70.0);
    let (a1, _) = ed.bounds().unwrap();
    assert!((a1.z - a0.z - 64.0).abs() < 0.5, "{a0} -> {a1}");
    // drag the +X / +Y corner handle of the top view by (100, 50)
    ed.drag_top_handle(1, 1, vec2(100.0, 50.0));
    let (a2, b2) = ed.bounds().unwrap();
    assert!((a2.x - a0.x).abs() < 0.5 && (a2.y - a0.y).abs() < 0.5, "min corner moved");
    assert!((b2.x % 32.0).abs() < 0.5 && (b2.y % 32.0).abs() < 0.5, "not on the grid: {b2}");
    assert!(b2.x > b0.x + 60.0 && b2.y > b0.y + 30.0, "{b0} -> {b2}");
    // dragging the min edge past the max edge keeps one grid step
    ed.drag_top_handle(-1, 0, vec2(5000.0, 0.0));
    let (a3, b3) = ed.bounds().unwrap();
    assert!((b3.x - a3.x - 32.0).abs() < 0.5, "{a3} {b3}");

    ed.add_boost(false);
    ed.resize_sel(0, 64.0);
    ed.rotate_sel(90.0);
    ed.restyle_sel(false);
    ed.add_boost(true);
    ed.resize_sel(0, 100.0);
    ed.drag_axis(0, 128.0);
    let text = crate::mapfile::to_text(&ed.game.map);
    let m = crate::mapfile::from_text(&text).unwrap();
    assert_eq!(m.boosters.len(), 2);
    for (x, y) in m.boosters.iter().zip(ed.game.map.boosters.iter()) {
        assert!((x.zone.mins - y.zone.mins).length() < 0.5);
        assert_eq!(format!("{:?}", x.push).len() > 0, true);
    }
    assert!(matches!(m.boosters[0].push, crate::map::Push::Boost { two_way: true, .. }));
    assert!(matches!(m.boosters[1].push, crate::map::Push::Launch { .. }));
}

/// Attachments come from map pickups into the inventory, get fitted and
/// come back to the inventory when the gun is lost.
#[test]
fn attachment_inventory() {
    use crate::game::*;
    use crate::map::PickupKind;
    use crate::weapons::*;
    let mut g = solo_game("surf_wars");
    g.give(0, WeaponId::Mp5);
    let k = g
        .pickups
        .iter()
        .position(|p| matches!(p.def.kind, PickupKind::Attachment(AttItem::Sight(Sight::RedDot))))
        .expect("red dot on the map");
    let pos = g.pickups[k].def.pos;
    g.players[0].pm = PmState::new(pos - vec3(0.0, 0.0, 30.0));
    g.step(Some(UserCmd { msec: 10, ..Default::default() }));
    let p = &g.players[0];
    // fitted straight onto the MP5 in hand
    assert_eq!(p.primary.unwrap().att.sight, Sight::RedDot, "inventory {:?}", p.inventory);
    assert!(p.inventory.is_empty());
    assert!(g.pickups[k].available_at > g.time);
    // ADS instead of a scope
    let w = p.primary.unwrap();
    assert!(w.ads_view(1) && !w.scope_view(1));

    // can't fit what you don't have
    assert!(!g.mount(0, Slot::Primary, 0, Some(AttItem::Sight(Sight::Acog))));
    g.players[0].inventory.push(AttItem::Sight(Sight::Acog));
    assert!(g.mount(0, Slot::Primary, 0, Some(AttItem::Sight(Sight::Acog))));
    let p = &g.players[0];
    assert_eq!(p.inventory, vec![AttItem::Sight(Sight::RedDot)]);
    assert!(p.primary.unwrap().scope_view(1), "the ACOG is a scope");
    // back to iron sights
    assert!(g.mount(0, Slot::Primary, 0, None));
    assert_eq!(g.players[0].inventory.len(), 2);
    assert!(g.mount(0, Slot::Primary, 0, Some(AttItem::Sight(Sight::Holo))) == false);

    // dropping the gun returns its parts; buying again refits them
    assert!(g.mount(0, Slot::Primary, 0, Some(AttItem::Sight(Sight::RedDot))));
    g.players[0].active = Slot::Primary;
    g.drop_weapon(0);
    assert_eq!(g.players[0].inventory.len(), 2);
    assert!(g.dropped.last().unwrap().weapon.att == Attachments::default());
    g.give(0, WeaponId::Mp5);
    assert_eq!(g.players[0].primary.unwrap().att.sight, Sight::RedDot);
    assert_eq!(g.players[0].inventory, vec![AttItem::Sight(Sight::Acog)]);
}

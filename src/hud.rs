//! 2D overlay in the style of the Counter-Strike 1.6 HUD.

use macroquad::prelude::*;

use crate::game::{Game, Mode, Phase, Player};
use crate::map::Team;
use crate::pmove::angle_vectors;
use crate::util::{horizontal, vec_to_angles};
use crate::weapons::{WeaponId, ALL_BUYABLE};

pub const HUD_COLOR: Color = Color::new(1.0, 0.69, 0.1, 0.9);
const CROSS_COLOR: Color = Color::new(0.2, 1.0, 0.2, 0.9);

pub fn team_ui_color(t: Team) -> Color {
    match t {
        Team::T => Color::new(1.0, 0.35, 0.25, 1.0),
        Team::CT => Color::new(0.45, 0.7, 1.0, 1.0),
    }
}

pub fn text(s: &str, x: f32, y: f32, size: f32, c: Color) {
    draw_text_ex(s, x, y, TextParams { font_size: size.round().max(4.0) as u16, color: c, ..Default::default() });
}

pub fn text_shadow(s: &str, x: f32, y: f32, size: f32, c: Color) {
    text(s, x + 2.0, y + 2.0, size, Color::new(0.0, 0.0, 0.0, c.a * 0.6));
    text(s, x, y, size, c);
}

pub fn text_width(s: &str, size: f32) -> f32 {
    measure_text(s, None, size.round().max(4.0) as u16, 1.0).width
}

pub fn text_centered(s: &str, cx: f32, y: f32, size: f32, c: Color) {
    let w = text_width(s, size);
    text_shadow(s, cx - w * 0.5, y, size, c);
}

fn panel(x: f32, y: f32, w: f32, h: f32) {
    draw_rectangle(x, y, w, h, Color::new(0.0, 0.0, 0.0, 0.55));
    draw_rectangle_lines(x, y, w, h, 2.0, Color::new(1.0, 0.69, 0.1, 0.35));
}

pub struct HudState<'a> {
    pub game: &'a Game,
    /// Player whose HUD we show (local or spectated).
    pub pov: Option<usize>,
    pub spectating: bool,
    pub show_scores: bool,
    pub buy_menu: bool,
    pub buy_page: u8,
    pub view_angles: Vec3,
    pub third_person: bool,
    pub fps: i32,
}

pub fn draw(h: &HudState) {
    let g = h.game;
    let sw = screen_width();
    let sh = screen_height();
    let s = sh / 720.0;

    let pov = h.pov.map(|i| &g.players[i]);

    if let Some(p) = pov {
        if p.alive {
            if p.zoom > 0 && !h.third_person {
                draw_scope(sw, sh);
            } else if !h.third_person {
                draw_crosshair(p, sw, sh, s);
            }
            draw_vitals(p, sw, sh, s);
            draw_ammo(p, sw, sh, s);
            draw_speed(p, sw, sh, s);
            draw_damage_indicator(g, p, h.view_angles, sw, sh, s);
            draw_status_text(g, p, h.view_angles, sw, sh, s);
        }
    }

    if let Some(p) = pov {
        draw_radar(g, p, h.view_angles, s);
    }
    draw_round_info(g, sw, sh, s);
    draw_killfeed(g, sw, s);

    if let Some((msg, _)) = &g.center_msg {
        let c = match g.last_winner {
            Some(t) => team_ui_color(t),
            None => WHITE,
        };
        text_centered(msg, sw * 0.5, sh * 0.3, 44.0 * s, c);
    }
    if g.phase == Phase::Freeze {
        let left = (g.phase_end - g.time).max(0.0).ceil();
        text_centered(&format!("Round starts in {left}"), sw * 0.5, sh * 0.36, 26.0 * s, WHITE);
        if let Some(p) = pov {
            if p.primary.is_none() && !h.buy_menu {
                text_centered("Press B to buy a weapon", sw * 0.5, sh * 0.41, 20.0 * s, HUD_COLOR);
            }
        }
    }

    if h.spectating {
        let name = pov.map(|p| p.name.as_str()).unwrap_or("-");
        let w = 560.0 * s;
        panel(sw * 0.5 - w * 0.5, 52.0 * s, w, 56.0 * s);
        text_centered(&format!("Spectating: {name}"), sw * 0.5, 78.0 * s, 24.0 * s, WHITE);
        text_centered(
            "MOUSE1 / MOUSE2: next / previous player    SPACE: first / third person",
            sw * 0.5,
            100.0 * s,
            15.0 * s,
            Color::new(0.8, 0.8, 0.8, 1.0),
        );
    }

    if h.buy_menu {
        if h.buy_page == 1 {
            draw_attachment_menu(g, h.pov, sw, sh, s);
        } else {
            draw_buy_menu(g, h.pov, sw, sh, s);
        }
    }
    if let Some(i) = h.pov.filter(|i| Some(*i) == g.local && g.players[*i].alive) {
        if let Some(id) = g.pickup_near(i) {
            text_centered(
                &format!("Press E to swap for the {}", id.def().name),
                sw * 0.5,
                sh * 0.62,
                20.0 * s,
                HUD_COLOR,
            );
        }
    }
    if h.show_scores {
        draw_scoreboard(g, sw, sh, s);
    }
    text(&format!("{} fps", h.fps), 6.0, sh - 4.0 * s, 13.0 * s, Color::new(1.0, 1.0, 1.0, 0.35));
}

/// CS style radar: teammates only, rotates with the view.
fn draw_radar(g: &Game, p: &Player, view: Vec3, s: f32) {
    let r = 78.0 * s;
    let cx = 20.0 * s + r;
    let cy = 30.0 * s + r;
    let range = 3500.0;
    draw_circle(cx, cy, r, Color::new(0.0, 0.0, 0.0, 0.45));
    draw_circle_lines(cx, cy, r, 2.0, Color::new(1.0, 0.69, 0.1, 0.4));
    draw_line(cx - r, cy, cx + r, cy, 1.0, Color::new(1.0, 1.0, 1.0, 0.12));
    draw_line(cx, cy - r, cx, cy + r, 1.0, Color::new(1.0, 1.0, 1.0, 0.12));
    let yaw = view.y.to_radians();
    let (sn, cs) = yaw.sin_cos();
    let me = p.pm.origin;
    for o in g.players.iter() {
        if !o.alive || o.team != p.team || std::ptr::eq(o, p) {
            continue;
        }
        let d = o.pm.origin - me;
        // rotate so that our view direction points up on the radar
        let fwd = d.x * cs + d.y * sn;
        let left = -d.x * sn + d.y * cs;
        let mut v = vec2(-left, -fwd) / range * r;
        if v.length() > r - 4.0 * s {
            v = v.normalize() * (r - 4.0 * s);
        }
        let c = team_ui_color(o.team);
        let dz = o.pm.origin.z - me.z;
        let size = 3.5 * s;
        if dz > 150.0 {
            draw_triangle(
                vec2(cx + v.x, cy + v.y - size * 1.4),
                vec2(cx + v.x - size, cy + v.y + size * 0.8),
                vec2(cx + v.x + size, cy + v.y + size * 0.8),
                c,
            );
        } else if dz < -150.0 {
            draw_triangle(
                vec2(cx + v.x, cy + v.y + size * 1.4),
                vec2(cx + v.x - size, cy + v.y - size * 0.8),
                vec2(cx + v.x + size, cy + v.y - size * 0.8),
                c,
            );
        } else {
            draw_rectangle(cx + v.x - size, cy + v.y - size, size * 2.0, size * 2.0, c);
        }
    }
    draw_triangle(vec2(cx, cy - 6.0 * s), vec2(cx - 4.0 * s, cy + 4.0 * s), vec2(cx + 4.0 * s, cy + 4.0 * s), WHITE);
}

fn draw_crosshair(p: &Player, sw: f32, sh: f32, s: f32) {
    let id = p.active_id();
    if matches!(id, WeaponId::Awp | WeaponId::Scout) {
        // CS 1.6 snipers have no crosshair without the scope.
        return;
    }
    let base = match id {
        WeaponId::Usp => 8.0,
        WeaponId::Ak47 => 4.0,
        WeaponId::Mp5 => 3.0,
        WeaponId::M3 => 8.0,
        _ => 5.0,
    };
    let speed = horizontal(p.pm.velocity).length();
    let mut gap = base;
    if !p.pm.onground {
        gap *= 2.4;
    } else if speed > 140.0 {
        gap *= 1.6;
    } else if p.pm.ducking {
        gap *= 0.8;
    }
    if let Some(w) = p.weapon() {
        gap += (w.shots_fired as f32).min(10.0) * 1.2;
    }
    let gap = gap * s;
    let len = 7.0 * s;
    let th = (1.5 * s).max(1.0);
    let cx = (sw * 0.5).floor();
    let cy = (sh * 0.5).floor();
    draw_rectangle(cx - gap - len, cy - th * 0.5, len, th, CROSS_COLOR);
    draw_rectangle(cx + gap, cy - th * 0.5, len, th, CROSS_COLOR);
    draw_rectangle(cx - th * 0.5, cy - gap - len, th, len, CROSS_COLOR);
    draw_rectangle(cx - th * 0.5, cy + gap, th, len, CROSS_COLOR);
}

fn draw_scope(sw: f32, sh: f32) {
    let cx = sw * 0.5;
    let cy = sh * 0.5;
    let r = sh * 0.5;
    let big = r * 1.6;
    let segs = 72;
    for i in 0..segs {
        let a0 = i as f32 / segs as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / segs as f32 * std::f32::consts::TAU;
        let p0 = vec2(cx + a0.cos() * r, cy + a0.sin() * r);
        let p1 = vec2(cx + a1.cos() * r, cy + a1.sin() * r);
        let q0 = vec2(cx + a0.cos() * big, cy + a0.sin() * big);
        let q1 = vec2(cx + a1.cos() * big, cy + a1.sin() * big);
        draw_triangle(p0, p1, q1, BLACK);
        draw_triangle(p0, q1, q0, BLACK);
    }
    draw_rectangle(0.0, 0.0, (cx - big * 0.7).max(0.0), sh, BLACK);
    draw_rectangle(cx + big * 0.7, 0.0, sw, sh, BLACK);
    draw_line(0.0, cy, sw, cy, 1.0, BLACK);
    draw_line(cx, 0.0, cx, sh, 1.0, BLACK);
}

fn draw_vitals(p: &Player, _sw: f32, sh: f32, s: f32) {
    let y = sh - 22.0 * s;
    let size = 46.0 * s;
    let x = 22.0 * s;
    let hc = if p.health <= 25.0 { Color::new(1.0, 0.2, 0.1, 0.95) } else { HUD_COLOR };
    // health cross
    let c = 16.0 * s;
    draw_rectangle(x + c * 0.35, y - c * 1.6, c * 0.3, c, hc);
    draw_rectangle(x, y - c * 1.25, c, c * 0.3, hc);
    text_shadow(&format!("{:>3}", p.health.ceil().max(0.0) as i32), x + 22.0 * s, y, size, hc);
    // armor shield
    let ax = x + 125.0 * s;
    let top = y - c * 1.65;
    draw_triangle(vec2(ax, top), vec2(ax + c, top), vec2(ax + c * 0.5, top + c * 1.2), HUD_COLOR);
    draw_rectangle(ax, top, c, c * 0.45, HUD_COLOR);
    if p.helmet {
        draw_circle(ax + c * 0.5, top - c * 0.2, c * 0.28, HUD_COLOR);
    }
    text_shadow(&format!("{:>3}", p.armor.ceil() as i32), ax + 22.0 * s, y, size, HUD_COLOR);
}

fn draw_ammo(p: &Player, sw: f32, sh: f32, s: f32) {
    let y = sh - 22.0 * s;
    let size = 46.0 * s;
    let name = p.active_id().def().name;
    if let Some(w) = p.weapon() {
        let t = format!("{:>2} | {:<3}", w.clip, w.reserve);
        let tw = text_width(&t, size);
        let x = sw - tw - 30.0 * s;
        let c = if w.clip == 0 { Color::new(1.0, 0.2, 0.1, 0.95) } else { HUD_COLOR };
        text_shadow(&t, x, y, size, c);
        // bullet icon
        let bx = x - 18.0 * s;
        draw_rectangle(bx, y - 30.0 * s, 7.0 * s, 20.0 * s, HUD_COLOR);
        draw_circle(bx + 3.5 * s, y - 30.0 * s, 3.5 * s, HUD_COLOR);
        if p.reloading() {
            text_shadow("RELOADING", x, y - 44.0 * s, 18.0 * s, HUD_COLOR);
        }
    }
    let tw = text_width(name, 18.0 * s);
    text_shadow(name, sw - tw - 30.0 * s, y - 64.0 * s, 18.0 * s, Color::new(1.0, 1.0, 1.0, 0.7));
}

fn draw_speed(p: &Player, sw: f32, sh: f32, s: f32) {
    let speed = horizontal(p.pm.velocity).length();
    let t = format!("{:.0}", speed);
    let c = if speed > 1500.0 {
        Color::new(1.0, 0.35, 0.9, 0.95)
    } else if speed > 800.0 {
        Color::new(0.3, 0.9, 1.0, 0.95)
    } else {
        Color::new(1.0, 1.0, 1.0, 0.85)
    };
    text_centered(&t, sw * 0.5, sh - 70.0 * s, 34.0 * s, c);
    text_centered("u/s", sw * 0.5, sh - 52.0 * s, 14.0 * s, Color::new(1.0, 1.0, 1.0, 0.6));
    if p.board > 0.5 {
        text_centered("SURFING", sw * 0.5, sh - 104.0 * s, 16.0 * s, Color::new(0.4, 1.0, 0.9, 0.8 * p.board));
    }
}

fn draw_round_info(g: &Game, sw: f32, sh: f32, s: f32) {
    // Timer at the bottom center like CS.
    if g.settings.mode == Mode::Rounds {
        let left = g.round_time_left().ceil() as i32;
        let t = format!("{}:{:02}", left / 60, left % 60);
        let c = if left <= 10 && g.phase == Phase::Live { Color::new(1.0, 0.2, 0.1, 0.95) } else { HUD_COLOR };
        text_centered(&t, sw * 0.5, sh - 14.0 * s, 34.0 * s, c);
    }
    // Scores at the top.
    let w = 280.0 * s;
    let x = sw * 0.5 - w * 0.5;
    panel(x, 6.0 * s, w, 40.0 * s);
    let tc = team_ui_color(Team::T);
    let cc = team_ui_color(Team::CT);
    let ta = g.alive_count(Team::T);
    let ca = g.alive_count(Team::CT);
    text_shadow("T", x + 12.0 * s, 34.0 * s, 26.0 * s, tc);
    text_shadow(&format!("{}", g.score[0]), x + 44.0 * s, 35.0 * s, 30.0 * s, WHITE);
    text_centered(
        &format!("{ta} alive  |  {ca} alive"),
        sw * 0.5,
        31.0 * s,
        15.0 * s,
        Color::new(0.85, 0.85, 0.85, 1.0),
    );
    let sc = format!("{}", g.score[1]);
    text_shadow(&sc, x + w - 60.0 * s - text_width(&sc, 30.0 * s), 35.0 * s, 30.0 * s, WHITE);
    text_shadow("CT", x + w - 44.0 * s, 34.0 * s, 26.0 * s, cc);
    if g.settings.mode == Mode::Deathmatch {
        text_centered("DEATHMATCH", sw * 0.5, 62.0 * s, 14.0 * s, Color::new(1.0, 1.0, 1.0, 0.6));
    }
}

fn draw_killfeed(g: &Game, sw: f32, s: f32) {
    let size = 18.0 * s;
    let mut y = 70.0 * s;
    for k in g.killfeed.iter() {
        let age = (g.time - k.time) as f32;
        let a = (1.0 - (age - 5.0).max(0.0)).clamp(0.0, 1.0);
        let weapon = format!(" [{}{}] ", k.weapon, if k.headshot { " HS" } else { "" });
        let killer = k.killer.as_ref().map(|(n, _)| n.as_str()).unwrap_or("");
        let total = text_width(killer, size) + text_width(&weapon, size) + text_width(&k.victim.0, size);
        let mut x = sw - total - 16.0 * s;
        draw_rectangle(x - 6.0 * s, y - size, total + 12.0 * s, size * 1.35, Color::new(0.0, 0.0, 0.0, 0.45 * a));
        if let Some((n, t)) = &k.killer {
            let mut c = team_ui_color(*t);
            c.a = a;
            text(n, x, y, size, c);
            x += text_width(n, size);
        }
        let mut wc = WHITE;
        wc.a = a;
        if k.headshot {
            wc = Color::new(1.0, 0.85, 0.3, a);
        }
        text(&weapon, x, y, size, wc);
        x += text_width(&weapon, size);
        let mut c = team_ui_color(k.victim.1);
        c.a = a;
        text(&k.victim.0, x, y, size, c);
        y += size * 1.5;
    }
}

fn draw_damage_indicator(g: &Game, p: &Player, view: Vec3, sw: f32, sh: f32, s: f32) {
    let age = (g.time - p.last_hurt) as f32;
    if age > 1.2 || p.last_attacker.is_none() {
        return;
    }
    let a = (1.0 - age / 1.2).clamp(0.0, 1.0);
    let (_, yaw) = vec_to_angles(p.hurt_from - p.pm.origin);
    let rel = (view.y - yaw).to_radians();
    let cx = sw * 0.5;
    let cy = sh * 0.5;
    let r = 90.0 * s;
    let dir = vec2(rel.sin(), -rel.cos());
    let perp = vec2(-dir.y, dir.x);
    let tip = vec2(cx, cy) + dir * (r + 22.0 * s);
    let b0 = vec2(cx, cy) + dir * r + perp * 18.0 * s;
    let b1 = vec2(cx, cy) + dir * r - perp * 18.0 * s;
    draw_triangle(tip, b0, b1, Color::new(1.0, 0.1, 0.05, 0.75 * a));
}

fn draw_status_text(g: &Game, p: &Player, view: Vec3, sw: f32, sh: f32, s: f32) {
    let (f, _, _) = angle_vectors(view);
    let eye = p.pm.eye();
    let wall = g.map.world.trace_ray(eye, eye + f * 6000.0);
    let max = wall.fraction * 6000.0;
    let mut best: Option<(usize, f32)> = None;
    for (j, o) in g.players.iter().enumerate() {
        if !o.alive || std::ptr::eq(o, p) {
            continue;
        }
        if let Some((t, _)) = g.ray_player(j, eye, f, max) {
            if best.is_none_or(|b| t < b.1) {
                best = Some((j, t));
            }
        }
    }
    if let Some((j, _)) = best {
        let o = &g.players[j];
        let friend = o.team == p.team;
        let t = if friend {
            format!("Friend: {}  Health: {}%", o.name, o.health.ceil() as i32)
        } else {
            format!("Enemy: {}", o.name)
        };
        let c = if friend { Color::new(0.4, 1.0, 0.4, 0.95) } else { Color::new(1.0, 0.35, 0.3, 0.95) };
        text_shadow(&t, sw * 0.5 - text_width(&t, 18.0 * s) * 0.5, sh * 0.5 + 60.0 * s, 18.0 * s, c);
    }
}

fn draw_buy_menu(g: &Game, pov: Option<usize>, _sw: f32, sh: f32, s: f32) {
    let x = 30.0 * s;
    let y = sh * 0.3;
    let w = 330.0 * s;
    let line = 30.0 * s;
    let h = line * (ALL_BUYABLE.len() as f32 + 4.4);
    panel(x, y, w, h);
    text_shadow("Buy weapon (free)", x + 14.0 * s, y + 30.0 * s, 24.0 * s, HUD_COLOR);
    let can_buy = pov.is_some_and(|i| g.in_buyzone(i));
    for (i, id) in ALL_BUYABLE.iter().enumerate() {
        let d = id.def();
        let c = if can_buy { WHITE } else { GRAY };
        text_shadow(
            &format!("{}. {}", i + 1, d.name),
            x + 20.0 * s,
            y + 30.0 * s + line * (i as f32 + 1.2),
            20.0 * s,
            c,
        );
        let info = format!("{}/{}", d.clip, d.reserve);
        text(&info, x + w - 80.0 * s, y + 30.0 * s + line * (i as f32 + 1.2), 16.0 * s, GRAY);
    }
    let n = ALL_BUYABLE.len() as f32;
    text_shadow(
        "4. Attachments for the gun in hand",
        x + 20.0 * s,
        y + 30.0 * s + line * (n + 1.4),
        20.0 * s,
        if can_buy { WHITE } else { GRAY },
    );
    text("AK, AWP, Scout: find them on the map", x + 20.0 * s, y + 30.0 * s + line * (n + 2.2), 15.0 * s, GRAY);
    let hint = if can_buy { "0. Close" } else { "You are not in a buy zone" };
    text_shadow(hint, x + 20.0 * s, y + h - 14.0 * s, 18.0 * s, if can_buy { WHITE } else { RED });
}

fn draw_attachment_menu(g: &Game, pov: Option<usize>, _sw: f32, sh: f32, s: f32) {
    let x = 30.0 * s;
    let y = sh * 0.26;
    let w = 560.0 * s;
    let line = 44.0 * s;
    let h = line * 7.2;
    panel(x, y, w, h);
    let Some(p) = pov.map(|i| &g.players[i]) else { return };
    let Some(wp) = p.weapon() else {
        text_shadow("Take out a gun to customize it", x + 14.0 * s, y + 30.0 * s, 22.0 * s, HUD_COLOR);
        return;
    };
    let can = pov.is_some_and(|i| g.in_buyzone(i));
    text_shadow(&format!("Attachments: {}", wp.def().name), x + 14.0 * s, y + 30.0 * s, 24.0 * s, HUD_COLOR);
    let a = wp.att;
    let rows: [(&str, &str, &str); 4] = [
        ("Sight", a.sight.name(), a.sight.desc()),
        ("Muzzle", a.muzzle.name(), a.muzzle.desc()),
        ("Stock", a.stock.name(), a.stock.desc()),
        ("Grip", a.grip.name(), a.grip.desc()),
    ];
    for (i, (cat, name, desc)) in rows.iter().enumerate() {
        let yy = y + 30.0 * s + line * (i as f32 + 1.0);
        text_shadow(&format!("{}. {cat}: {name}", i + 1), x + 20.0 * s, yy, 21.0 * s, if can { WHITE } else { GRAY });
        text(desc, x + 44.0 * s, yy + 18.0 * s, 15.0 * s, Color::new(0.75, 0.75, 0.75, 1.0));
    }
    let m = wp.mods();
    let stats = format!(
        "damage x{:.2}  recoil x{:.2}  spread x{:.2}  speed {:+.0}{}",
        m.damage,
        m.recoil,
        m.spread,
        m.speed,
        if m.silenced { "  silenced" } else { "" }
    );
    text(&stats, x + 20.0 * s, y + 30.0 * s + line * 5.1, 16.0 * s, HUD_COLOR);
    let hint = if can { "1-4 next option (SHIFT: previous)    0. Back" } else { "You are not in a buy zone" };
    text_shadow(hint, x + 20.0 * s, y + h - 14.0 * s, 17.0 * s, if can { WHITE } else { RED });
}

fn draw_scoreboard(g: &Game, sw: f32, sh: f32, s: f32) {
    let w = (760.0 * s).min(sw - 20.0);
    let x = sw * 0.5 - w * 0.5;
    let y = sh * 0.14;
    let row = 24.0 * s;
    let rows = g.players.len() as f32 + 6.0;
    panel(x, y, w, row * rows);
    text_shadow(&format!("{}   -   Round {}", g.map.name, g.round), x + 16.0 * s, y + 28.0 * s, 22.0 * s, HUD_COLOR);
    let mut yy = y + 64.0 * s;
    for team in [Team::T, Team::CT] {
        let c = team_ui_color(team);
        text_shadow(&format!("{}  -  {}", team.name(), g.score[team.index()]), x + 16.0 * s, yy, 20.0 * s, c);
        text("Kills", x + w - 220.0 * s, yy, 16.0 * s, GRAY);
        text("Deaths", x + w - 140.0 * s, yy, 16.0 * s, GRAY);
        yy += row;
        let mut list: Vec<&Player> = g.players.iter().filter(|p| p.team == team).collect();
        list.sort_by(|a, b| b.kills.cmp(&a.kills));
        for p in list {
            let mut pc = if p.alive { WHITE } else { Color::new(0.6, 0.6, 0.6, 1.0) };
            if !p.is_bot() {
                pc = Color::new(1.0, 1.0, 0.6, 1.0);
            }
            let name = if p.alive { p.name.clone() } else { format!("{}  (dead)", p.name) };
            text(&name, x + 30.0 * s, yy, 18.0 * s, pc);
            text(&format!("{}", p.kills), x + w - 210.0 * s, yy, 18.0 * s, pc);
            text(&format!("{}", p.deaths), x + w - 125.0 * s, yy, 18.0 * s, pc);
            yy += row;
        }
        yy += row * 0.6;
    }
}

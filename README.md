# SurfWars

A Counter-Strike 1.6 style surf combat game written in Rust (with
[macroquad](https://github.com/not-fl3/macroquad)). Two teams of players and
bots surf ramps, bunny hop across pillars and shoot each other with the
classic CS arsenal.

![Surfing](docs/surfing.png)

| | |
|---|---|
| ![First person](docs/first_person.png) | ![surf_canyon](docs/canyon.png) |
| ![Overview of surf_wars](docs/overview.png) | |

## Features

- **GoldSrc / CS 1.6 movement, ported line by line** from `pm_shared.c`:
  `PM_AirAccelerate` with the 30 u/s wish speed cap, `PM_FlyMove` clipping
  (this is what makes surfing work), ground friction and edge friction, the
  18 unit step-up, split gravity integration, duck timing and duck jumping,
  the CS jump stamina (`fuser2`) and landing slowdown, weapon based max
  speeds and the 100 Hz tick of a `fps_max 100` client.
- **Surf server settings**: everything is stock CS except what surf servers
  change: `sv_airaccelerate 100`, `sv_maxvelocity 3500` and no bunny hop
  speed cap. Auto bunny hop is available as an option (off by default).
- **Hover boards**: every player gets a glowing board under their feet
  while they touch a surf ramp, with a team coloured trail at speed.
- **Weapons**: USP, MP5 Navy, M3 shotgun, AK-47, Scout and AWP (plus the
  knife) with CS 1.6 damage, fire rate, magazine sizes, reload times,
  movement speeds, spread formulas, recoil (`KickBack`), armor penetration,
  range falloff and hitbox multipliers. Scopes have two zoom levels.
- **Two teams, rounds or deathmatch**: CS style elimination rounds with
  freeze time, round timer, scores and a free buy menu in spawn; or
  deathmatch with instant respawns.
- **Bots that actually surf**: bots use the same movement code as you. They
  pick routes (surf lanes, bunny hop pillars or sniper towers), steer on
  ramps with perpendicular air strafes, bunny hop pillars with landing
  prediction, and fight with reaction times, tracking, recoil control and
  burst fire. Four difficulty levels.
- **Two maps** made of ramps with bunny hop sections:
  - `surf_wars`: two long V shaped surf lanes between the spawns, a middle
    island reached by a descending pillar line, and two sniper towers at the
    end of long bunny hop paths.
  - `surf_canyon`: three parallel lanes, pillar lines between them leading
    to mid platforms, and cross ramps below.
  Falling into the water teleports you back to your spawn, like on surf
  servers. The spawn floor is tinted with the colour of the ramp below it, so
  you can see where to drop in.
- CS 1.6 style HUD (health, armor, ammo, timer, scores, kill feed, dynamic
  crosshair, radar, scoreboard) plus a speedometer, and procedurally
  synthesized sounds.

## Building and running

Install Rust (<https://rustup.rs>), then:

```sh
cargo run --release
```

On Linux the sound backend needs the ALSA development package
(`sudo apt install libasound2-dev` on Debian/Ubuntu, `alsa-lib-devel` on
Fedora). To build without sound:

```sh
cargo run --release --no-default-features
```

Settings from the menu are stored in `surfwars.cfg` next to where you run
the game. On a machine without a sound device, start it with `--no-audio`.

## Controls

| Key | Action |
|---|---|
| W A S D | move |
| Space / mouse wheel | jump (the wheel is the classic bunny hop bind) |
| Ctrl / C | duck |
| Shift | walk |
| Mouse 1 | fire |
| Mouse 2 | scope (Scout, AWP) / stab (knife) |
| 1 2 3 | primary / pistol / knife |
| Q | last weapon |
| R | reload |
| G | drop weapon |
| B | buy menu (in your spawn) |
| Tab | scoreboard |
| V | third person camera, to see your own board |
| Esc | pause menu (sensitivity, team switch, restart) |

While dead: Mouse 1 / Mouse 2 cycle through players, Space toggles first and
third person.

### How to surf

Drop onto a ramp, look along it and hold the strafe key that pushes you *into*
the ramp (A if the ramp is on your left, D if it is on your right). Steer with
the mouse. Don't hold W on a ramp: the forward key only slows you down in the
air, exactly like in CS.

## Options

- **Ramp accuracy** (menu): in stock CS a player on a surf ramp is "in the
  air", which makes rifles almost useless while surfing. With the default
  *Surf* setting, riding a ramp uses the running accuracy instead, so fights
  between surfers are possible. *Classic CS* keeps the stock behaviour.
  Jumping and falling always use the in air spread.
- **Auto bunny hop**, **bot count per team**, **bot skill**, **mode**,
  **mouse sensitivity** (CS scale, `m_yaw 0.022`), **volume**.

## Code layout

| File | Purpose |
|---|---|
| `src/pmove.rs` | CS 1.6 player movement port and movement variables |
| `src/collision.rs` | convex brushes with bevel planes, box sweeps (GoldSrc hull semantics) |
| `src/map.rs` | map definitions, spawns, teleport and buy zones, bot routes |
| `src/weapons.rs` | weapon stats, CS accuracy and recoil formulas, armor |
| `src/game.rs` | game state, shooting, damage, rounds, triggers |
| `src/bot.rs` | bot navigation, surfing and bunny hop control, combat |
| `src/render.rs` | world, player models, boards, trails, effects, view model |
| `src/hud.rs` | HUD, radar, scoreboard, buy menu |
| `src/audio.rs` | sound synthesis and playback |
| `src/main.rs` | window, input, camera, menus |

### Tests

```sh
cargo test --release -- --nocapture
```

The tests check the movement (standing, jumping, surfing a ramp, ramp seams,
the stock bunny hop cap), weapon behaviour (auto fire and reloads, semi-auto
pistols, scope resume, shotgun shell reloads, headshots), run a bot along
every route of every map and report how far it gets, and simulate full bot
matches to make sure fights happen and rounds end.

The binary can also render screenshots without user input, which is how the
images above were made:

```sh
cargo run --release -- --no-audio --shot out.png --cam spectate --after 11
```

`--cam` accepts `spectate`, `third`, `eye`, `overview`, `possess` and
`possess3`; `--ui buy|scores|pause|scope`, `--map`, `--team t|ct`,
`--follow N` and `--look pitch,yaw` are also available.

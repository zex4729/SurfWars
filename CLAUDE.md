# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

SurfWars is a Counter-Strike 1.6 style surf combat game in Rust on macroquad 0.4. It is a single binary crate with no asset files: the maps are built in code, and textures and sounds are generated procedurally at startup. See README.md for features and controls.

## Commands

```sh
cargo build --release
cargo run --release                       # needs a display; on Linux also libasound2-dev
cargo run --release --no-default-features # build without the `audio` feature (no ALSA)
cargo test --release                      # all tests (the bot and match simulations want --release)
cargo test --release bot_routes -- --nocapture   # one test, with its printed report
cargo clippy --release
cargo fmt                                  # rustfmt.toml: max_width 120
```

All tests are in `src/tests.rs`, which is a `#[cfg(test)]` module of the binary:
- `bot_routes` runs one bot along every route of every map and prints how far it got. Setting `BOTLOG=<route name substring>` dumps that route's trajectory.
- `bot_match`, `idle_human_long_run` and `shot_stats` simulate full matches headlessly. `shot_stats` prints hit rates by weapon and by ground/air, which is useful when tuning balance.

### Headless screenshots

The binary can render one frame and exit, which is the way to check visual changes. It works under Xvfb with Mesa:

```sh
xvfb-run -a -s "-screen 0 1280x720x24" env LIBGL_ALWAYS_SOFTWARE=1 \
  ./target/release/surfwars --no-audio --shot out.png --cam spectate --after 11
```

Options:
- `--cam`: `spectate`, `third`, `eye`, `overview`, `possess`, `possess3`
- `--ui`: `buy`, `scores`, `pause`, `scope`
- also `--menu`, `--map`, `--team t|ct`, `--follow N`, `--look pitch,yaw`, `--after SECONDS`

The possess modes copy a surfing bot's movement state onto the local player. Without a sound device, ALSA prints errors and the audio thread panics; this is harmless.

## Architecture

**Coordinates and units.** The world is Quake/GoldSrc style: Z-up, right-handed, units in inches. Angles are `(pitch, yaw, roll)` in degrees and positive pitch looks down. `pmove::angle_vectors` is the Quake `AngleVectors`. The renderer uses Z-up Camera3D directly, so there is no axis conversion.

**Simulation loop.** `game::Game::step` advances one fixed 10 ms tick (`TICK_MSEC`), like CS at `fps_max 100`. `main.rs` accumulates frame time, runs ticks, and renders with interpolation between `prev_origin` and `pm.origin`. Mouse look updates the view angles every frame, and each tick's `UserCmd` samples them. The game is deterministic given the seed (`util::Rng`), with no rand crate.

**Everything moves through `UserCmd`.** The human (`App::build_cmd`) and the bots (`bot::think`) both produce a `pmove::UserCmd`. `Game::run_player` then runs movement (`pmove::PlayerMove::run`), triggers (teleport zones), weapon pickup and `weapon_frame`. Bots have no movement shortcuts: they send analog `forwardmove`/`sidemove` relative to their aim yaw. Bot actions that need `&mut Game`, such as buying or switching weapons, go into `BotBrain.actions` and are applied in `Game::step`.

**Movement (`pmove.rs`)** is a faithful port of CS 1.6's `pm_shared.c`. Keep it that way. Only the surf-server values in `MoveVars::surf_server()` differ from stock.

**Collision (`collision.rs`).** Brushes are convex half-space sets. Each brush gets axial and edge bevel planes so that Minkowski expansion by the player AABB stays tight. Two deliberate choices matter for surfing; don't "fix" them back to Quake 3 behaviour:
- GoldSrc semantics: a plane only blocks a move that actually crosses it. The Q3 epsilon check makes players stick to ramps.
- The entering plane is chosen by its exact crossing fraction, and the `DIST_EPSILON` backoff is applied afterwards. Choosing by the backed-off fraction turns ramp seams into invisible walls. `ramp_seams_do_not_stop_surfers` guards this.

**Events.** Game logic pushes `game::Event`s: shots, hits, kills, teleports and so on. `main.rs` drains them after each tick and hands them to `render::Renderer::handle_events` (effects) and `audio::Audio::handle_events`. New feedback should go through an event, not through calls from the game into the renderer or audio.

**Maps (`map.rs`)** are functions that return a `Map`: brushes built with `Brush::cuboid` or `Brush::hull(points)`, spawns, teleport and buy zones, fog and sky colours, and bot `Route`s. Routes are written for the T side and mirrored to CT with `mirror_route`, which relies on the maps being symmetric in x. Each waypoint has a mode (`Walk`, `Drop`, `Hop`, `Surf`, `Hold`) that drives the bot controller. A new map needs routes, or bots won't move usefully. Register it in `map_names()` and `load()`.

**Bots (`bot.rs`).** The surf controller rotates velocity with air-accel pushes perpendicular to the horizontal velocity, toward a look-ahead point on the route line. Pushing straight into a tilted ramp brakes. The bhop controller predicts landing time. On the climbing half of a lane, route waypoints deliberately keep the valley height instead of following the ridge up.

**Weapons (`weapons.rs`)** hold CS 1.6 stats plus the spread (`compute_spread`) and recoil (`apply_recoil` / `KickBack`) formulas. `Settings::ramp_accuracy` (default on) is a deliberate deviation from stock CS: a player touching a surf ramp counts as grounded for spread.

**Rendering (`render.rs`).**
- World brushes become static meshes grouped by texture, capped at `MAX_VERTS` per mesh. macroquad meshes use u16 indices, and the draw-call capacities are set in `window_conf`.
- Dynamic geometry goes through `Batch`: players are box models, and boards, trails and effects are sprites. There are three materials: `world_mat` (fog and animated water via `normal.w`), additive `fx_mat` and alpha-blended `alpha_mat`.
- The view model is drawn after `clear_depth()`.
- Hover boards and the player lean are driven by `Player::board` and `board_normal`, which fade in from `PmState::surf_time` in `Game::run_player`.

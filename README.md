# on-sunday-i-will-kill-again

A standalone Rust library that connects to classic Quake II as an ordinary player. Build a deathmatch, team deathmatch or CTF bot by reading observations and submitting player input; the library handles the network connection. It has no external Rust dependencies and needs no game assets on the client.

## Included

- Protocol 34 challenge/connect, reliable commands, delta snapshots and movement checksums.
- Independent background networking at 20 input updates per second.
- Player/entity state, player names and teams, inventory, model/sound/item names and classic CTF HUD values.
- Movement, aiming, attack/respawn input, weapon selection, CTF team joining and classic skin/model team identification.
- Optional versioned human capability profiles, disabled by default.
- Bounded event history, expiring input, stale-observation rejection, map changes and server-requested reconnects.

Navigation, map geometry, line-of-sight calculations, target selection, combat policy, learning and team coordination belong in your bot. There is no built-in AI, renderer, JSON service or deployment integration.

## Start here

Use a stable Rust toolchain. From this checkout, with a compatible CTF server running:

```sh
cargo run --example passive -- 127.0.0.1:27910 ExamplePlayer red
cargo run --example controller -- 127.0.0.1:27910
```

`passive` reports state for 30 seconds and disconnects. `controller` is a deliberately inert policy scaffold: replace its `decide()` function with your bot. They occupy normal player slots.

For another Rust project, use this Git dependency (the crate is not published to crates.io):

```toml
[dependencies]
on-sunday-i-will-kill-again = { git = "https://github.com/bigdave/on-sunday-i-will-kill-again", tag = "v0.4.0" }
```

For a local checkout, use a path dependency instead (adjust the path):

```toml
[dependencies]
on-sunday-i-will-kill-again = { path = "../on-sunday-i-will-kill-again" }
```

```rust,no_run
use on_sunday_i_will_kill_again::{Action, Client, Config, Team};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::new("127.0.0.1:27910".parse()?, "MyBot");
    config.team = Some(Team::Red);
    let mut player = Client::connect(config)?;
    loop {
        if let Some(observation) = player.wait_for_update(Duration::from_millis(100))? {
            if !observation.connected { break; }
            if observation.ready {
                // Replace with your decisions. Neutral input is intentional here.
                player.submit(&observation, Action::default())?;
            }
        }
    }
    Ok(())
}
```

`connect()` returns after the network handshake; wait for `ready` before controlling the player. Submit fresh actions regularly. Dropping the client disconnects and joins its network worker. Create multiple clients for multiple players, in one process or separate processes.

## Optional human limitations

`HumanLimitationsV2` uses the same view, turning, and 200 ms world-perception
limits as V1. It refreshes the local player's movement and camera state from the
latest server snapshot. Entities, events, HUD, weapons, and inventory still
follow the delayed publication. `Observation.frame` and `snapshot_age` describe
that delayed world snapshot.

`HumanLimitationsV3` uses the V2 rules with a 100 ms world-perception delay.
Spatial sound events with a known origin also carry a coarse `SoundPan`
(`Left`, `Center`, or `Right`) relative to the listener's view when the cue
was first observed. As with ordinary stereo panning, a centered sound may be
in front of or behind the listener. Sounds outside the visual field of view
can still provide a stereo cue.

`HumanLimitationsV4` caps pitch and yaw independently at 360°/s, or 18°
per axis per 50 ms input tick. It publishes the same decoded observations as
unrestricted mode, including full sound metadata and camera kick. It adds no
acceleration ramp. Roll and non-view inputs use the ordinary transport rules.
Select it with `PlayerCapabilities::human_limitations_v4()`.

V1 was introduced in **v0.2.0**. To pin that release:

```toml
[dependencies]
on-sunday-i-will-kill-again = { git = "https://github.com/bigdave/on-sunday-i-will-kill-again", tag = "v0.2.0" }
```

All teams should use the same release tag (or an agreed immutable commit `rev`). Upgrading from 0.1.x: explicit `Config` struct literals must add `capabilities: None` or use `..Default::default()`. Existing constructor-based clients retain unrestricted behavior by default. Enable the profile explicitly:

```rust
use on_sunday_i_will_kill_again::{Config, PlayerCapabilities};
let mut config = Config::new("127.0.0.1:27910".parse().unwrap(), "MyBot");
config.capabilities = Some(PlayerCapabilities::human_limitations_v1());
```

Leave `capabilities = None` (the default) for the existing unrestricted behavior. The named v1 profile has fixed parameters, independent of the bot name, with no randomness:

| Property | v1 behavior |
| --- | --- |
| Perception delay | At least 200 ms for gameplay observations, including self state, HUD, inventory, and events |
| Field of view | 110° horizontal × 80° vertical; entity origins tested against the controlled camera, own entity retained |
| Turn speed | At most 360°/s combined pitch/yaw |
| Angular acceleration | At most 1440°/s² combined pitch/yaw controller acceleration; hard pitch stops at ±89° |
| Roll | Zero |
| Input / publication rate | Existing 20 Hz worker; extra calls cannot advance the limiter |
| Random aim error | None; prediction and accurate tracking remain useful |

For V1–V3, both `observation()` and `wait_for_update()` expose the delayed and filtered publication. During delay warmup or stale snapshots, `ready` is false and gameplay data is empty. Lifecycle invalidation is immediate: map/reconnection changes clear the delay queue, preventing old-map data from resurfacing. Snapshot and inventory ages include the intentional delay. All four profiles reset turn state for server delta-angle changes and respawn.

With any profile enabled, `Action.view` is a desired orientation. V1–V3 rotate movement into the limited camera's axes to preserve the intended world travel direction, subject to normal input clamps, and brake on `view=None`. V4 uses the submitted movement axes and holds its current limited pitch and yaw on `view=None`. Attack remains the bot's decision and uses the transmitted aim. Expired input releases movement and attack.

V1–V3 retain sound identity and volume but remove exact source position, entity and channel. V3 also supplies the stereo cue described above. Out-of-view muzzle flashes and opaque temporary-effect payloads are removed. Public roster/config strings, text and scoreboard layouts are retained but delayed.

**FOV is not wall occlusion.** The crate still has no map geometry: in-frustum network entities can be behind walls. Bots must supply geometry checks separately, and this profile does not model partial visibility, directional hearing or a human's complete perception. This is an opt-in convention, not enforcement against modified clients. New balancing parameters should use a new profile version rather than silently changing v1.

## Compatibility and observations

Validated locally with Q2PRO serving protocol 34:

| Mode | Verified behavior |
| --- | --- |
| Free-for-all deathmatch | Joining, movement, weapon switching, damage, kill scoring, respawn and map changes. |
| Classic skin-team deathmatch | Same core controls, skin team identification, enemy frags, friendly-fire protection and teamkill penalties. |
| Classic model-team deathmatch | Same checks with teams defined by player model. |
| Classic ThreeWave CTF | Opposite-team joining, inventory, movement, firing and map changes. |

For base-game team deathmatch, leave `Config.team=None` and choose `Config.skin` to join the appropriate skin/model team. `PlayerInfo::deathmatch_team(rule)` gives the classic team key. The caller supplies the server's rule; `DeathmatchTeamRule::from_dmflags()` decodes it. `Config.team` and `Observation::team()` are CTF-specific. See [mode setup](docs/server.md).

These checks use disposable fixture modules for positioning, loadouts and health; ordinary combat, scoring and team rules are unchanged. This is an initial 0.1 API, not a guarantee for every mod or server. Extended protocols and the 2023 rerelease are unsupported.

Snapshots are **network information, not human vision**. Potentially visible set (PVS) updates can contain entities behind walls or behind the player; absence does not prove an entity no longer exists. The optional profile supplies the timing and FOV restrictions described above; collision/occlusion tests must still be supplied separately. The crate is not a competition sandbox or anti-cheat system.

See [the interface contract](docs/interface.md), [server setup](docs/server.md), and [testing](docs/testing.md).

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo doc --no-deps
```

The live mode tests are opt-in and requires separately installed legal game data and a disposable test server. No maps or commercial assets are distributed here.

## License

GPL-2.0-or-later. Protocol code and checksum data derive from Quake II / Q2PRO; see [NOTICE](./NOTICE) and [LICENSE](./LICENSE). Preserve those notices and account for GPL obligations when distributing derivative programs. The game assets have separate licensing.

### Competition readiness (0.3.0)

`Observation::ctf_match_status` exposes classic CTF's public match HUD text,
including `SETUP: … not ready`, `… UNTIL START`, and `… MATCH`. It is empty
when unavailable and follows the selected capability profile's observation delay.
This field is separate from `Observation::ready`, which means a fresh playable
snapshot, **not** competition commitment.

Call `client.ready_up(&observation)?` to queue the ordinary player `ready`
command. The caller decides when the team is assembled. There is no automatic
readiness by default and no administrative start command. Requests coalesce,
expire with `Config::action_timeout`, cannot cross map/reconnect contexts, and
are sent at most once every two seconds. Server rules still decide whether the
player may ready up. `human_limitations_v1` is unchanged.

Version 0.3.0 adds a public observation field; users constructing complete
`Observation` literals should add the field or use `..Default::default()`.

`client.join_team(&observation, Team::Blue)?` queues the ordinary `team blue`
command (or `Team::Red` for red). It shares the bounded command slot and rate
limit with `ready_up`; the latest request wins. This lets a caller rejoin when
competition returns players to spectators between rounds. There is no automatic
team change: the caller explicitly requests it using a fresh observation.

For a spectator during an active match or countdown, `join_team` follows the
ordinary visible join menu instead: it opens the menu, advances its cursor, or
selects the requested team according to the latest delayed layout observation.
Call again after a fresh menu observation until assigned. It never selects a
menu marked `MATCH IS LOCKED`; server entry restrictions remain authoritative.

### Rate-limited team chat

`client.say_team(&observation, "I'm going for the flag.")?` queues an ordinary
`say_team` message. `true` means accepted into the single pending slot, not server
acknowledgement; `false` means busy or cooling down. Admission and actual sends
are limited to one message per five seconds per client. Messages expire with
`action_timeout`, cannot cross map/reconnect contexts, and never delay movement
or ready/menu commands. Received text remains ordinary delayed `Event::Message`.

Text is limited to 160 printable ASCII bytes. Empty/whitespace-only text, control
characters, quotes, backslashes, semicolons, and percent macros are rejected;
there is no console injection or implicit CTF location/inventory expansion.
No chat is sent unless the caller explicitly uses the API. Human capability
profiles are unchanged.

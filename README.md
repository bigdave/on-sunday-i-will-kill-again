# on-sunday-i-will-kill-again

A standalone Rust library that connects to classic Quake II as an ordinary player. Build a CTF bot by reading observations and submitting player input; the library handles the network connection. It has no external Rust dependencies and needs no game assets on the client.

## Included

- Protocol 34 challenge/connect, reliable commands, delta snapshots and movement checksums.
- Independent background networking at 20 input updates per second.
- Player/entity state, player names and teams, inventory, model/sound/item names and classic CTF HUD values.
- Movement, aiming, attack/respawn input, weapon selection and team selection.
- Bounded event history, expiring input, stale-observation rejection, map changes and server-requested reconnects.

Navigation, map geometry, line-of-sight calculations, target selection, combat policy, learning and team coordination belong in your bot. There is no built-in AI, renderer, JSON service or deployment integration.

## Start here

Use a stable Rust toolchain. From this checkout, with a compatible CTF server running:

```sh
cargo run --example passive -- 127.0.0.1:27910 ExamplePlayer red
cargo run --example controller -- 127.0.0.1:27910
```

`passive` reports state for 30 seconds and disconnects. `controller` is a deliberately inert policy scaffold: replace its `decide()` function with your bot. They occupy normal player slots.

For another local Rust project, add a path dependency (adjust the path):

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

## Compatibility and observations

Validated locally with Q2PRO serving protocol 34 and classic ThreeWave CTF, including two clients on opposite teams, inventory, movement, firing and a map transition. This is an initial 0.1 API, not a guarantee for every mod or server. Extended protocols and the 2023 rerelease are unsupported. Ordinary protocol-34 deathmatch uses the same transport; its HUD differs from CTF.

Snapshots are **network information, not human vision**. Potentially visible set (PVS) updates can contain entities behind walls or behind the player; absence does not prove an entity no longer exists. Collision tests and fair perception rules must be supplied separately. The crate is not a competition sandbox or anti-cheat system.

See [the interface contract](docs/interface.md), [server setup](docs/server.md), and [testing](docs/testing.md).

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo doc --no-deps
```

The live CTF test is opt-in and requires separately installed legal game data and a disposable test server. No maps or commercial assets are distributed here.

## License

GPL-2.0-or-later. Protocol code and checksum data derive from Quake II / Q2PRO; see [NOTICE](NOTICE) and [LICENSE](LICENSE). Preserve those notices and account for GPL obligations when distributing derivative programs. The game assets have separate licensing.

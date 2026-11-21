# Testing

The default suite needs no assets and opens no network sockets:

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

It covers packet checksums and redundancy, reliable acknowledgements/retransmission, player/entity deltas, truncated and malformed messages, bounded events, map resets, context rejection and input validation.

## Opt-in live CTF test

`tests/live_ctf.rs` starts a disposable loopback-only server, connects two ordinary clients, and always terminates its child server on exit. Supply absolute paths to:

- `Q2_TEST_ENGINE`: a Q2PRO dedicated server executable accepting classic protocol 34.
- `Q2_TEST_DATA`: your separately installed legal `baseq2` and classic `ctf` data (including `q2ctf6` and `q2ctf2`).
- `Q2_TEST_GAME`: a matching native CTF game library with the local test fixture below.

```sh
Q2_TEST_ENGINE=/path/to/q2proded \
Q2_TEST_DATA=/path/to/game-data \
Q2_TEST_GAME=/path/to/test-game-library \
cargo test --test live_ctf -- --ignored --nocapture
```

The test verifies opposite-team admission, independent player slots, server-observed movement, automatic input expiry while application polling stops, inventory and weapon state, firing events, map transitions, stale command rejection and explicit disconnect. It checks zero decoder errors in the successful session. It is not a long-duration, packet-loss, performance or security audit. The server log is left in the operating system's temporary directory for diagnosis.

### Local fixture module

`tests/fixtures/ctf_commands.inc` supplies console-only position, health, flag and inventory controls for a disposable classic CTF module. In a copy of the GPL classic CTF source, include this snippet inside `ServerCommand()` after `cmd = gi.argv(1);`, where the normal game API functions and types are in scope, then build that copy for your server architecture. The ordinary module is untouched. This fixture assumes the standard classic CTF game API and is not a universal patch for unrelated mods.

Never install the fixture in a public/production server: its commands intentionally bypass normal gameplay. The snippet and the client are source only; game assets remain external. The CI workflow runs asset-free tests; the opt-in test must be run separately.

## Opt-in base-game deathmatch tests

`tests/live_deathmatch.rs` tests free-for-all plus **both classic team variants**, with enemy and teammate cases for each variant (five local matches total). Use the same `Q2_TEST_ENGINE` and `Q2_TEST_DATA`, with `q2dm1` and `q2dm2` available. Set `Q2_TEST_DM_GAME` to a separately built native **base-game** module containing `tests/fixtures/dm_commands.inc` inside `ServerCommand()`, after `cmd = gi.argv(1);`. The snippet targets Q2PRO's classic base-game API; it does not require CTF code.

```sh
Q2_TEST_ENGINE=/path/to/q2proded \
Q2_TEST_DATA=/path/to/game-data \
Q2_TEST_DM_GAME=/path/to/test-base-game-library \
cargo test --test live_deathmatch -- --ignored --nocapture --test-threads=1
```

The fixture positions a pair on clear ground near a map spawn, supplies a controlled loadout and health, and suppresses unrelated map pickups during the combat assertions. It does not change damage, obituary/scoring or team logic. Keep it off public/production servers, just like the CTF fixture.

Checks include admission, raw names/skins, exact skin/model team keys, movement and input expiry, inventory, weapon switching, server-applied damage, enemy kill scoring, friendly-fire protection, teamkill penalties after enabling friendly damage, explicit attack-to-respawn, transition from `q2dm1` to `q2dm2`, stale-input rejection, zero decode errors and disconnect. The free-for-all case deliberately uses identical skins to confirm they do not create teams when team flags are off.

### Recorded validation

On 2026-11-21, all three test cases passed locally with Q2PRO protocol 34 and its classic base-game module plus the fixture controls: `deathmatch`, `skin_teams`, and `model_teams`. Both enemy and friendly cases passed for each team rule. This verifies classic base-game rules, not arbitrary third-party team mods or extended protocols. CTF has its separate successful lifecycle test above.

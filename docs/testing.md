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

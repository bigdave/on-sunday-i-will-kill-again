# Changelog

## Unreleased

Add opt-in `Config.capabilities = Some(PlayerCapabilities::human_limitations_v1())`: fixed observation delay, FOV/event filtering, and worker-timed angular speed/acceleration limits. Default `None` preserves unrestricted behavior. No new dependencies or map assets required. Config struct literals must include the new field or use `..Default::default()`.

## 0.1.1

Added configurable UDP source ports and a public version constant for consumer diagnostics. Inventory polling now waits for a living, active player so it does not toggle spectator/join menus.

## 0.1.0

Initial extraction of an independent protocol-34 player interface: background connection lifecycle, typed observations and timed input, classic CTF HUD/team plumbing, examples and tests. Navigation, combat policies and deployment configuration are intentionally outside this crate.

Added verified base-game deathmatch and skin/model team-deathmatch integration tests, explicit `DeathmatchTeamRule` / `PlayerInfo::deathmatch_team()` helpers, and mode-specific setup documentation.

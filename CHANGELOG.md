# Changelog

## 0.1.0

Initial extraction of an independent protocol-34 player interface: background connection lifecycle, typed observations and timed input, classic CTF HUD/team plumbing, examples and tests. Navigation, combat policies and deployment configuration are intentionally outside this crate.

Added verified base-game deathmatch and skin/model team-deathmatch integration tests, explicit `DeathmatchTeamRule` / `PlayerInfo::deathmatch_team()` helpers, and mode-specific setup documentation.

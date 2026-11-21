# Interface contract

## Lifecycle and timing

`Client::connect(Config)` performs a bounded synchronous handshake, then starts a private network thread. Each client uses its own UDP socket and qport. It needs no async runtime. Server addresses are resolved socket addresses; callers can use Rust's `ToSocketAddrs` for DNS.

`observation()` clones the latest publication without blocking on a packet. `wait_for_update()` waits for a newer publication, returning `None` on a wait timeout. Publication is approximately 20 Hz, so multiple observations can have the same server frame. Check `frame` if a policy should run only on fresh snapshots. The server remains authoritative; there is no client prediction.

`submit(&observation, action)` replaces the last input; there is no input backlog. A context contains a client ID, connection epoch and map epoch. A command referring to another client, a previous connection/map, a future sequence or an observation older than the action window is rejected. Context and sequence are coordination mechanisms, not a security boundary against malicious bot code.

Defaults: input period 50 ms, action lifetime 300 ms, snapshot freshness 500 ms, connect timeout 10 seconds, server timeout 15 seconds. Movement and attack become neutral when input expires or snapshots stop arriving. Weapon commands already sent reliably cannot be retracted. Observation age is estimated from publication sequence for submissions; the worker additionally checks elapsed time since submission. Actions should be based on a recently received observation.

`ready` means spawned with a recent valid snapshot, including while dead; it does not mean alive. The policy explicitly decides when to respawn (`attack`). A map change clears old world state and invalidates old commands. Server-requested reconnections are handled; server shutdown, kicks, handshake failures and prolonged silence are terminal. Recreate a client after a terminal failure. `wait_for_update()` first delivers the final disconnected observation, then returns the terminal error. `close()` and dropping the client stop and join its thread; a reconnect handshake may take up to its short socket polling interval to notice closure.

## Actions

| Field | Meaning |
| --- | --- |
| `view` | Optional world angles in degrees. Pitch positive looks down, yaw 0 faces +X and 90 faces +Y; roll is normally zero. Pitch is clamped to ±89°. Server delta-angle offsets are handled internally. `None` keeps the last server view. |
| `forward` | Forward/backward input, clamped to ±400. This is a movement command, not a guaranteed world velocity. |
| `side` | Positive moves right, negative left, clamped to ±400. |
| `up` | Positive jumps/swims up; negative crouches/swims down. ±200 is conventional. Release between successive jumps. |
| `attack` | Holds attack plus the ordinary any-button bit, also used for explicit respawn/intermission input. |
| `use_weapon` | Ordinary item name, e.g. `Rocket Launcher`; requires ownership and appropriate ammo. Repeated requests are rate limited. |

Actions do not automatically jump obstacles, aim, shoot, select targets, choose a weapon or respawn. Only initial requested team joining and optional inventory polling are automatic game commands. Set `inventory_interval=None` to disable polling. Arbitrary console commands and server-provided scripts are not executed.

## State

Positions and velocities are decoded into Quake world units and units/second. `PlayerState.view_angles` remain raw unsigned 16-bit angle units (65536 per turn); `delta_angles` are raw signed offsets. `Action.view` accepts degrees. `EntityState.angles` are degrees at the protocol's 8-bit precision. State structs expose the fields needed for control; they are not a complete rendering demo format.

Player slots are zero based; player entity IDs are `slot + 1`. `players` includes configured slots, some empty. `PlayerInfo.team` and `Observation::team()` recognize classic CTF `ctf_r` / `ctf_b` skins only. For base-game team deathmatch, use `PlayerInfo::deathmatch_team(DeathmatchTeamRule::Skin)` or `::Model` and compare exact keys. Set `Config.skin` to join that team and leave `Config.team=None`. `DeathmatchTeamRule::from_dmflags()` gives model teams (128) precedence over skin teams (64), matching the base game. Supply the actual server rule; it is not inferred from snapshots. An empty raw `game_dir` denotes the default base game. Entity model indices resolve through `models`; sound indices through `sounds`. Model 255 is the player-model convention, not an ordinary model path. Item inventory indices resolve through `item_names`.

`health()`, `ammo()`, `armor()` and `score()` read conventional HUD slots. `ctf_hud()` reads classic flag icons and capture totals. HUD definitions are mod-specific; these are not authoritative per-player capture events. Raw player stats remain available. There is no brittle parsing of kill announcements into an inferred scoreboard.

Inventory is a polled snapshot, not a promise of current ownership: inspect its `age` and `updates`. It starts as `None` and clears on map changes. Models, skins and other configstrings can arrive incrementally during setup.

Events include messages, center prints, layouts, sound, muzzle flashes and temporary effects. `events` is a rolling 256-record history repeated in observations. Deduplicate by `(connection_epoch, event.id)`; IDs increase within a connection across maps. If IDs skip beyond your last seen record, older events have been evicted. Map changes clear the history and emit a map event. Temporary effects retain the discriminator and remaining bytes rather than pretending to provide a complete semantic effects API. Muzzle flashes include non-weapon effects, so they are not inherently proof of damage or a kill.

`decode_errors` counts malformed or unsupported packets; `ProtocolWarning` events explain recent failures. Lost delta bases request a full snapshot. A single bad packet does not kill the connection. Unsupported protocol negotiation is terminal. Limits include 64 delta frames, 256 event records, 256 player/config lookup slots, entity IDs up to 8191, and 128 pending reliable commands. Pending duplicate commands are coalesced; excess commands are dropped if the queue is full. Snapshot clones cost memory proportional to currently known entities, so keep only the observations your policy needs.

## Competition boundary

This library supplies transport and normal client input, not a privileged server control channel. It loads no BSP, navigation file or game assets. It also does not restrict a contestant's other code, enforce line of sight, or provide hidden-state isolation. Agree separately on observation filtering, allowed map knowledge, computation budgets and team communication before comparing entrants.

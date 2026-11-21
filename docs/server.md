# Server setup

Use a classic Quake II server that accepts protocol 34 and a native CTF game module matching the server's operating system and CPU architecture. A server module is separate from this Rust client library. The library works as an ordinary network player and does not require a server-side AI plugin.

For a separately installed Q2PRO server with legal `baseq2` and `ctf` assets, an example local test launch is:

```sh
q2proded +set basedir /path/to/game-data +set game ctf \
  +set net_ip 127.0.0.1 +set net_port 27910 \
  +set deathmatch 1 +set ctf 1 +set competition 0 \
  +set maxclients 9 +set sv_iplimit 8 +set public 0 +map q2ctf1
```

This example intentionally listens only on loopback. Configure a separate listening address for players on other machines. Native module discovery/build details depend on your Q2PRO installation. No game data or server binary is included with this crate.

Allow enough player slots and enough clients from the same IP address for your teams. The server may impose password, rate, spectator, skin or anti-cheat policies; ordinary client admission rules still apply. Set `Config.password` only for the ordinary player password. The crate does not use RCON.

The supported protocol is 34. Do not force extended or rerelease-only protocols. Unsupported server messages are reported through protocol warnings; custom mods may need additional decoders or HUD mappings. Server-requested arbitrary console text, automatic downloads and shell commands are deliberately ignored.

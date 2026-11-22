use crate::{
    capabilities::Capabilities,
    game::Game,
    types::*,
    wire::{self, Channel, UserCmd},
    Error,
};
use std::{
    net::UdpSocket,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
const TICK: Duration = Duration::from_millis(50);
static CLIENT_ID: AtomicU64 = AtomicU64::new(1);
struct Input {
    context: Context,
    sequence: u64,
    at: Instant,
    action: Action,
}
struct ReadyRequest {
    context: Context,
    at: Instant,
}
struct State {
    observation: Observation,
    input: Option<Input>,
    ready_request: Option<ReadyRequest>,
    closed: bool,
    error: Option<Error>,
}
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    stop: AtomicBool,
}
/// One independent ordinary player, with a bounded background network worker.
/// Dropping it disconnects and joins the worker. It never runs a bot policy.
pub struct Client {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    seen: u64,
    config: Config,
}
impl Client {
    pub fn connect(config: Config) -> Result<Self, Error> {
        config.validate()?;
        let bind = if config.server.is_ipv4() {
            format!("0.0.0.0:{}", config.local_port)
        } else {
            format!("[::]:{}", config.local_port)
        };
        let socket = UdpSocket::bind(bind).map_err(io)?;
        socket.connect(config.server).map_err(io)?;
        let qport = socket.local_addr().map_err(io)?.port();
        let stop = AtomicBool::new(false);
        handshake(&socket, qport, &config, &stop)?;
        socket.set_nonblocking(true).map_err(io)?;
        let id = CLIENT_ID.fetch_add(1, Ordering::Relaxed);
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                observation: Observation {
                    context: Context {
                        client_id: id,
                        connection_epoch: 1,
                        map_epoch: 0,
                    },
                    connected: true,
                    ..Default::default()
                },
                input: None,
                ready_request: None,
                closed: false,
                error: None,
            }),
            changed: Condvar::new(),
            stop: AtomicBool::new(false),
        });
        let copy = shared.clone();
        let cfg = config.clone();
        let worker = thread::Builder::new()
            .name("on-sunday-i-will-kill-again-network".into())
            .spawn(move || {
                let result = run(socket, qport, &cfg, &copy, id);
                let mut state = copy.state.lock().unwrap_or_else(|e| e.into_inner());
                state.closed = true;
                state.observation.connected = false;
                state.observation.ready = false;
                if let Err(e) = result {
                    state.observation.error = Some(e.to_string());
                    state.error = Some(e);
                }
                state.observation.sequence += 1;
                copy.changed.notify_all();
            })
            .map_err(io)?;
        Ok(Self {
            shared,
            worker: Some(worker),
            seen: 0,
            config,
        })
    }
    pub fn observation(&self) -> Observation {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .observation
            .clone()
    }
    /// Wait for a new published observation. None means the wait timed out.
    pub fn wait_for_update(&mut self, timeout: Duration) -> Result<Option<Observation>, Error> {
        let state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        let (state, _) = self
            .shared
            .changed
            .wait_timeout_while(state, timeout, |s| {
                s.observation.sequence <= self.seen && !s.closed
            })
            .unwrap_or_else(|e| e.into_inner());
        if state.observation.sequence > self.seen {
            self.seen = state.observation.sequence;
            return Ok(Some(state.observation.clone()));
        }
        if state.closed {
            return Err(state.error.clone().unwrap_or(Error::Closed));
        }
        Ok(None)
    }
    /// Submit timed movement/attack against the observation that informed it.
    /// Inputs replace rather than queue. They expire unless refreshed.
    pub fn submit(&self, observation: &Observation, action: Action) -> Result<(), Error> {
        action.validate()?;
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed {
            return Err(state.error.clone().unwrap_or(Error::Closed));
        }
        if !valid_context(observation, &state.observation, self.config.action_timeout) {
            return Err(Error::StaleObservation);
        }
        state.input = Some(Input {
            context: observation.context,
            sequence: observation.sequence,
            at: Instant::now(),
            action,
        });
        Ok(())
    }
    /// Queue the ordinary classic CTF `ready` command against a fresh observation.
    /// This is explicit opt-in, not a toggle or an administrative match start.
    /// Repeated requests coalesce; the worker sends at most once every two seconds.
    /// Pending requests expire after action_timeout and never cross map/reconnect contexts.
    pub fn ready_up(&self, observation: &Observation) -> Result<(), Error> {
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed {
            return Err(state.error.clone().unwrap_or(Error::Closed));
        }
        if !observation.ready
            || !observation.connected
            || !valid_context(observation, &state.observation, self.config.action_timeout)
        {
            return Err(Error::StaleObservation);
        }
        state.ready_request = Some(ReadyRequest {
            context: observation.context,
            at: Instant::now(),
        });
        Ok(())
    }
    pub fn close(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.close();
    }
}
fn take_ready_request(
    request: &mut Option<ReadyRequest>,
    context: Context,
    now: Instant,
    last: Instant,
    ttl: Duration,
    fresh: bool,
) -> bool {
    let Some(pending) = request.as_ref() else {
        return false;
    };
    if pending.context != context || now.duration_since(pending.at) > ttl {
        *request = None;
        return false;
    }
    if !fresh || now.duration_since(last) < Duration::from_secs(2) {
        return false;
    }
    *request = None;
    true
}
fn io(e: std::io::Error) -> Error {
    Error::Io(e.to_string())
}
fn valid_context(old: &Observation, current: &Observation, ttl: Duration) -> bool {
    old.context == current.context
        && old.sequence <= current.sequence
        && Duration::from_millis(
            current
                .sequence
                .saturating_sub(old.sequence)
                .saturating_mul(50),
        ) <= ttl
}
fn oob(socket: &UdpSocket, s: &str) -> Result<(), Error> {
    let mut data = vec![255; 4];
    data.extend_from_slice(s.as_bytes());
    socket.send(&data).map_err(io)?;
    Ok(())
}
fn handshake(socket: &UdpSocket, qport: u16, cfg: &Config, stop: &AtomicBool) -> Result<(), Error> {
    socket.set_nonblocking(false).map_err(io)?;
    socket
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(io)?;
    let mut buf = [0u8; 65536];
    let mut challenge = None;
    let deadline = Instant::now() + cfg.connect_timeout;
    let mut last = Instant::now() - Duration::from_secs(1);
    while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
        if last.elapsed() >= Duration::from_millis(250) {
            if let Some(n) = challenge {
                let password = cfg
                    .password
                    .as_ref()
                    .map(|s| format!("\\password\\{s}"))
                    .unwrap_or_default();
                oob(socket,&format!("connect 34 {qport} {n} \"\\name\\{}\\skin\\{}\\rate\\25000\\msg\\1\\fov\\90\\hand\\2\\spectator\\0{password}\"\n",cfg.name,cfg.skin))?;
            } else {
                oob(socket, "getchallenge\n")?;
            }
            last = Instant::now();
        }
        match socket.recv(&mut buf) {
            Ok(n) if n >= 4 && buf[..4] == [255; 4] => {
                let text = String::from_utf8_lossy(&buf[4..n]);
                if text.starts_with("client_connect") {
                    return Ok(());
                }
                if let Some(s) = text.strip_prefix("challenge ") {
                    challenge = s
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse::<u32>().ok());
                } else if text.contains("Bad challenge") || text.contains("No challenge") {
                    challenge = None;
                } else if let Some(s) = text.strip_prefix("print\n") {
                    return Err(Error::Disconnected(s.trim().into()));
                }
            }
            Ok(_) => (),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(io(e)),
        }
    }
    if stop.load(Ordering::Relaxed) {
        Err(Error::Closed)
    } else {
        Err(Error::Timeout)
    }
}
fn publish(
    game: &Game,
    shared: &Shared,
    id: u64,
    connection: u64,
    errors: u64,
    cfg: &Config,
    limits: &mut Capabilities,
) {
    let age = game.last_snapshot.map(|t| t.elapsed());
    let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
    let sequence = state.observation.sequence + 1;
    let raw = Observation {
        context: Context {
            client_id: id,
            connection_epoch: connection,
            map_epoch: game.map_epoch,
        },
        sequence,
        connected: true,
        ready: game.spawned && age.map(|a| a <= cfg.snapshot_timeout).unwrap_or(false),
        frame: game.last_frame,
        game_dir: game.game_dir.clone(),
        ctf_match_status: game.match_status().into(),
        map: game.map.clone(),
        slot: usize::try_from(game.slot).ok(),
        player: game.player.clone(),
        entities: game.entities.clone(),
        players: (0..game.max_clients)
            .map(|slot| PlayerInfo {
                slot,
                entity_id: slot as u16 + 1,
                name: game.player_names[slot].clone(),
                skin: game.player_skins[slot].clone(),
                team: game.player_teams[slot],
            })
            .collect(),
        models: game.models.clone(),
        sounds: game.sounds.clone(),
        images: game.images.clone(),
        item_names: game.item_names.clone(),
        inventory: game.inventory_updated.map(|t| Inventory {
            counts: game.inventory,
            age: t.elapsed(),
            updates: game.inventory_updates,
        }),
        snapshot_age: age,
        events: game.events.iter().cloned().collect(),
        decode_errors: errors,
        error: None,
    };
    state.observation = limits.observation(raw, Instant::now());
    shared.changed.notify_all();
}
fn movement(action: &Action, game: &Game) -> UserCmd {
    let view = action.view.unwrap_or(ViewAngles {
        pitch: game.player.view_angles[0] as i16 as f32 * 360. / 65536.,
        yaw: game.player.view_angles[1] as f32 * 360. / 65536.,
        roll: game.player.view_angles[2] as f32 * 360. / 65536.,
    });
    let angles = [view.pitch.clamp(-89., 89.), view.yaw, view.roll];
    UserCmd {
        angles: std::array::from_fn(|i| {
            ((angles[i].rem_euclid(360.) * 65536. / 360.) as i32 as i16)
                .wrapping_sub(game.player.delta_angles[i])
        }),
        forward: action.forward.clamp(-400, 400),
        side: action.side.clamp(-400, 400),
        up: action.up.clamp(-400, 400),
        buttons: if action.attack { 129 } else { 0 },
        msec: 50,
    }
}
fn inventory_due(game: &Game, interval: Option<Duration>, elapsed: Duration) -> bool {
    let Some(interval) = interval else {
        return false;
    };
    game.spawned
        && game.player.pm_type == 0
        && game.player.stats[1] > 0
        && elapsed
            >= if game.inventory_pending {
                interval.max(Duration::from_secs(4))
            } else {
                interval
            }
}
fn run(socket: UdpSocket, qport: u16, cfg: &Config, shared: &Shared, id: u64) -> Result<(), Error> {
    let mut limits = Capabilities::new(cfg.capabilities);
    let mut connection = 1;
    let mut map_epoch = 0;
    let mut ch = Channel::new(qport);
    ch.command("new".into());
    let mut game = Game {
        assigned_team: cfg.team,
        ..Default::default()
    };
    let mut commands = [UserCmd::default(); 3];
    let mut next = Instant::now();
    let mut last_receive = Instant::now();
    let mut inventory = Instant::now() - Duration::from_secs(10);
    let mut weapon = String::new();
    let mut weapon_at = Instant::now() - Duration::from_secs(10);
    let mut ready_at = Instant::now() - Duration::from_secs(10);
    let mut errors = 0;
    let mut buf = [0u8; 65536];
    let result = (|| {
        while !shared.stop.load(Ordering::Relaxed) {
            let mut reconnect = false;
            for _ in 0..64 {
                match socket.recv(&mut buf) {
                    Ok(n) => {
                        if n >= 4 && buf[..4] == [255; 4] {
                            continue;
                        }
                        match ch.receive(&buf[..n]) {
                            Ok(Some(data)) => {
                                last_receive = Instant::now();
                                if let Err(e) = game.parse(data, &mut ch) {
                                    if e == "server requested reconnect" {
                                        reconnect = true;
                                        break;
                                    }
                                    if e == "server disconnected bot" {
                                        return Err(Error::Disconnected(
                                            "server closed the connection".into(),
                                        ));
                                    }
                                    if e.starts_with("unsupported protocol ") {
                                        return Err(Error::Protocol(e));
                                    }
                                    errors += 1;
                                    game.last_frame = -1;
                                    game.last_snapshot = None;
                                    game.event(Event::ProtocolWarning { message: e });
                                }
                            }
                            Ok(None) => (),
                            Err(e) => {
                                errors += 1;
                                game.event(Event::ProtocolWarning { message: e });
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(io(e)),
                }
            }
            if reconnect {
                inventory = Instant::now() - Duration::from_secs(10);
                weapon.clear();
                weapon_at = inventory;
                connection += 1;
                game = Game {
                    assigned_team: cfg.team,
                    ..Default::default()
                };
                commands = [UserCmd::default(); 3];
                publish(&game, shared, id, connection, errors, cfg, &mut limits);
                handshake(&socket, qport, cfg, &shared.stop)?;
                socket.set_nonblocking(true).map_err(io)?;
                ch = Channel::new(qport);
                ch.command("new".into());
                last_receive = Instant::now();
                next = Instant::now();
                continue;
            }
            if map_epoch != game.map_epoch {
                map_epoch = game.map_epoch;
                commands = [UserCmd::default(); 3];
                weapon.clear();
                inventory = Instant::now() - Duration::from_secs(10);
                weapon_at = inventory;
            }
            if last_receive.elapsed() > cfg.server_timeout {
                return Err(Error::Timeout);
            }
            let now = Instant::now();
            if now >= next {
                let context = Context {
                    client_id: id,
                    connection_epoch: connection,
                    map_epoch: game.map_epoch,
                };
                let fresh = game.spawned
                    && game
                        .last_snapshot
                        .map(|s| s.elapsed() <= cfg.snapshot_timeout)
                        .unwrap_or(false);
                let action = {
                    let state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                    state
                        .input
                        .as_ref()
                        .filter(|i| {
                            i.context == context
                                && i.at.elapsed() <= cfg.action_timeout
                                && state.observation.sequence.saturating_sub(i.sequence) * 50
                                    <= cfg.action_timeout.as_millis() as u64
                                && fresh
                        })
                        .map(|i| i.action.clone())
                        .unwrap_or_default()
                };
                {
                    let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
                    if take_ready_request(
                        &mut state.ready_request,
                        context,
                        now,
                        ready_at,
                        cfg.action_timeout,
                        fresh,
                    ) {
                        ch.command("ready".into());
                        ready_at = now;
                    }
                }
                let action = limits.action(action, &game.player, context);
                if fresh {
                    if inventory_due(&game, cfg.inventory_interval, inventory.elapsed()) {
                        ch.command("inven".into());
                        game.inventory_pending = true;
                        inventory = now;
                    }
                    if let Some(w) = &action.use_weapon {
                        if weapon_at.elapsed() >= Duration::from_millis(250)
                            && (w != &weapon || weapon_at.elapsed() > Duration::from_secs(2))
                        {
                            ch.command(format!("use {w}"));
                            weapon = w.clone();
                            weapon_at = now;
                        }
                    }
                }
                commands.rotate_left(1);
                commands[2] = if fresh {
                    movement(&action, &game)
                } else {
                    UserCmd {
                        msec: 50,
                        ..Default::default()
                    }
                };
                let payload = wire::movement(game.last_frame, ch.outgoing, &commands);
                socket.send(&ch.packet(&payload)).map_err(io)?;
                publish(&game, shared, id, connection, errors, cfg, &mut limits);
                next = now + TICK;
            }
            thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    })();
    let mut disconnect = vec![4];
    disconnect.extend_from_slice(b"disconnect\0");
    for _ in 0..3 {
        let _ = socket.send(&ch.packet(&disconnect));
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readiness_is_bounded_expires_and_cannot_cross_contexts() {
        let now = Instant::now();
        let context = Context::default();
        let ttl = Duration::from_millis(250);
        let mut request = Some(ReadyRequest { context, at: now });
        assert!(!take_ready_request(
            &mut request,
            context,
            now,
            now,
            ttl,
            true
        ));
        assert!(request.is_some());
        assert!(take_ready_request(
            &mut request,
            context,
            now,
            now - Duration::from_secs(2),
            ttl,
            true
        ));
        assert!(request.is_none());
        request = Some(ReadyRequest { context, at: now });
        assert!(!take_ready_request(
            &mut request,
            context,
            now + ttl + Duration::from_millis(1),
            now - Duration::from_secs(3),
            ttl,
            true
        ));
        assert!(request.is_none());
        for changed in [
            Context {
                map_epoch: 1,
                ..context
            },
            Context {
                connection_epoch: 1,
                ..context
            },
        ] {
            request = Some(ReadyRequest { context, at: now });
            assert!(!take_ready_request(
                &mut request,
                changed,
                now,
                now - Duration::from_secs(3),
                ttl,
                true
            ));
            assert!(request.is_none());
        }
    }
    #[test]
    fn public_read_apis_share_only_the_processed_publication() {
        let mut limits = Capabilities::new(Some(PlayerCapabilities::human_limitations_v1()));
        let now = Instant::now();
        let mut raw = Observation {
            connected: true,
            ready: true,
            sequence: 1,
            ctf_match_status: "SETUP: 10 not ready".into(),
            ..Default::default()
        };
        raw.entities.insert(
            2,
            crate::EntityState {
                origin: [-100., 0., 0.],
                ..Default::default()
            },
        );
        let warmup = limits.observation(raw.clone(), now);
        assert!(!warmup.ready);
        assert!(warmup.ctf_match_status.is_empty());
        raw.sequence = 2;
        let processed = limits.observation(raw, now + Duration::from_millis(200));
        let mut client = Client {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    observation: processed,
                    input: None,
                    ready_request: None,
                    closed: false,
                    error: None,
                }),
                changed: Condvar::new(),
                stop: AtomicBool::new(false),
            }),
            worker: None,
            seen: 0,
            config: Config {
                capabilities: Some(PlayerCapabilities::human_limitations_v1()),
                ..Default::default()
            },
        };
        let first = client.observation();
        assert!(first.ready && first.entities.is_empty());
        assert_eq!(first.ctf_match_status, "SETUP: 10 not ready");
        client.ready_up(&first).unwrap();
        let mut stale = first.clone();
        stale.context.map_epoch += 1;
        assert_eq!(client.ready_up(&stale), Err(Error::StaleObservation));
        assert_eq!(
            format!("{first:?}"),
            format!(
                "{:?}",
                client.wait_for_update(Duration::ZERO).unwrap().unwrap()
            )
        );
        for _ in 0..1000 {
            client.submit(&first, Action::default()).unwrap();
        }
        assert_eq!(client.observation().sequence, first.sequence);
        assert!(client.wait_for_update(Duration::ZERO).unwrap().is_none());
    }
    #[test]
    fn inventory_poll_waits_until_alive_and_retries_missing_replies() {
        let mut game = Game {
            spawned: true,
            ..Default::default()
        };
        let interval = Some(Duration::from_secs(2));
        assert!(!inventory_due(&game, interval, Duration::from_secs(10)));
        game.player.stats[1] = 100;
        assert!(inventory_due(&game, interval, Duration::from_secs(3)));
        game.inventory_pending = true;
        assert!(!inventory_due(&game, interval, Duration::from_secs(3)));
        assert!(inventory_due(&game, interval, Duration::from_secs(5)));
        game.player.pm_type = 2;
        assert!(!inventory_due(&game, interval, Duration::from_secs(5)));
        game.player.pm_type = 0;
        assert!(!inventory_due(&game, None, Duration::from_secs(5)));
        let mut packet = vec![5];
        for n in 0..256 {
            packet.extend((if n == 7 { 50i16 } else { 0i16 }).to_le_bytes());
        }
        let mut channel = Channel::new(123);
        game.parse(&packet, &mut channel).unwrap();
        assert_eq!(game.inventory[7], 50);
        assert!(!game.inventory_pending);
        assert_eq!(game.inventory_updates, 1);
        assert!(channel.packet(&[]).windows(7).any(|w| w == b"putaway"));
    }
    #[test]
    fn neutral_input_preserves_negative_pitch() {
        let mut game = Game::default();
        game.player.view_angles[0] = (-1820i16) as u16;
        assert_eq!(movement(&Action::default(), &game).angles[0], -1820);
    }
    #[test]
    fn stale_contexts_and_future_inputs_are_rejected() {
        let current = Observation {
            context: Context {
                client_id: 1,
                connection_epoch: 2,
                map_epoch: 3,
            },
            sequence: 30,
            ..Default::default()
        };
        let mut old = current.clone();
        assert!(valid_context(&old, &current, Duration::from_millis(300)));
        old.sequence = 20;
        assert!(!valid_context(&old, &current, Duration::from_millis(300)));
        old = current.clone();
        old.context.map_epoch = 2;
        assert!(!valid_context(&old, &current, Duration::from_secs(1)));
        old = current.clone();
        old.context.client_id = 2;
        assert!(!valid_context(&old, &current, Duration::from_secs(1)));
        old = current.clone();
        old.sequence = 31;
        assert!(!valid_context(&old, &current, Duration::from_secs(1)));
    }
    #[test]
    fn view_offsets_and_input_limits_are_transport_not_policy() {
        let mut game = Game::default();
        game.player.delta_angles = [100, 200, 300];
        let cmd = movement(
            &Action {
                view: Some(ViewAngles {
                    pitch: 0.,
                    yaw: 90.,
                    roll: 0.,
                }),
                forward: 999,
                side: -999,
                up: 200,
                attack: true,
                use_weapon: None,
            },
            &game,
        );
        assert_eq!(cmd.angles, [-100, 16384 - 200, -300]);
        assert_eq!(cmd.forward, 400);
        assert_eq!(cmd.side, -400);
        assert_eq!(cmd.buttons, 129);
        assert_eq!(cmd.up, 200);
    }
}

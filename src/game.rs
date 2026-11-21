use crate::{
    types::*,
    wire::{self, Channel, EntityState, PlayerState, Reader},
};
use std::{
    collections::{BTreeMap, VecDeque},
    time::Instant,
};
fn setup_command(command: &str) -> bool {
    let words: Vec<_> = command.split_whitespace().collect();
    match words.as_slice() {
        ["new"] => true,
        ["configstrings" | "baselines", a, b] => {
            a.parse::<i32>().is_ok() && b.parse::<u32>().is_ok()
        }
        _ => false,
    }
}
#[derive(Clone, Default)]
pub(crate) struct FrameState {
    pub(crate) player: PlayerState,
    entities: BTreeMap<u16, EntityState>,
}
pub(crate) struct Game {
    pub(crate) frames: BTreeMap<i32, FrameState>,
    pub(crate) baselines: BTreeMap<u16, EntityState>,
    pub(crate) entities: BTreeMap<u16, EntityState>,
    pub(crate) max_clients: usize,
    pub(crate) player_names: Vec<String>,
    pub(crate) player_skins: Vec<String>,
    pub(crate) player_teams: Vec<Option<Team>>,
    pub(crate) images: Vec<String>,
    pub(crate) sounds: Vec<String>,
    pub(crate) assigned_team: Option<Team>,
    pub(crate) models: Vec<String>,
    pub(crate) player: PlayerState,
    pub(crate) last_frame: i32,
    pub(crate) spawn: i32,
    pub(crate) slot: i16,
    pub(crate) spawned: bool,
    pub(crate) map: String,
    pub(crate) inventory: [i16; 256],
    pub(crate) item_names: Vec<String>,
    pub(crate) inventory_pending: bool,
    pub(crate) inventory_updated: Option<Instant>,
    pub(crate) inventory_updates: u64,
    pub(crate) last_snapshot: Option<Instant>,
    pub(crate) map_epoch: u64,
    pub(crate) game_dir: String,
    pub(crate) events: VecDeque<EventRecord>,
    pub(crate) event_sequence: u64,
}

impl Default for Game {
    fn default() -> Self {
        Self {
            frames: BTreeMap::new(),
            baselines: BTreeMap::new(),
            entities: BTreeMap::new(),
            max_clients: 9,
            player_names: vec![String::new(); 256],
            player_skins: vec![String::new(); 256],
            player_teams: vec![None; 256],
            images: vec![String::new(); 256],
            sounds: vec![String::new(); 256],
            assigned_team: None,
            models: vec![String::new(); 256],
            player: PlayerState::default(),
            last_frame: -1,
            spawn: 0,
            slot: -1,
            spawned: false,
            map: String::new(),
            inventory: [0; 256],
            item_names: vec![String::new(); 256],
            inventory_pending: false,
            inventory_updated: None,
            inventory_updates: 0,
            last_snapshot: None,
            map_epoch: 0,
            game_dir: String::new(),
            events: VecDeque::new(),
            event_sequence: 0,
        }
    }
}
impl Game {
    pub(crate) fn event(&mut self, event: Event) {
        self.event_sequence += 1;
        if self.events.len() == 256 {
            self.events.pop_front();
        }
        self.events.push_back(EventRecord {
            id: self.event_sequence,
            map_epoch: self.map_epoch,
            event,
        });
    }
    fn stuff(&mut self, s: &str, ch: &mut Channel) {
        // Whitelist protocol setup commands. Never execute shell or arbitrary console text.
        for line in s.split(['\n', ';']) {
            let line = line.trim();
            let words: Vec<_> = line.split_whitespace().collect();
            if words.first() == Some(&"precache") {
                if let Some(n) = words.get(1).and_then(|s| s.parse::<i32>().ok()) {
                    if n == self.spawn {
                        ch.command(format!("begin {}", n));
                        if let Some(team) = self.assigned_team {
                            ch.command(format!("team {}", team.as_str()));
                        }
                    }
                }
            } else if let Some(cmd) = line.strip_prefix("cmd ") {
                if setup_command(cmd) {
                    ch.command(cmd.to_string());
                } else if cmd.starts_with("\u{7f}c version") {
                    ch.command(format!(
                        "\u{7f}c version on-sunday-i-will-kill-again-{}",
                        env!("CARGO_PKG_VERSION")
                    ));
                }
            } else if line == "reconnect" {
                ch.command("new".into());
                self.spawned = false;
            }
        }
    }
    pub(crate) fn parse(&mut self, data: &[u8], ch: &mut Channel) -> wire::Result<()> {
        let mut r = Reader::new(data);
        while r.pos < data.len() {
            match r.u8()? {
                opcode @ (1 | 2) => {
                    let entity = r.u16()?;
                    let flash = r.u8()?;
                    self.event(Event::MuzzleFlash {
                        entity,
                        flash,
                        monster: opcode == 2,
                    });
                }
                3 => {
                    let start = r.pos;
                    wire::temp_entity(&mut r)?;
                    self.event(Event::TemporaryEntity {
                        kind: data[start],
                        data: data[start + 1..r.pos].to_vec(),
                    });
                }
                opcode @ (4 | 15) => {
                    let text = r.string()?.chars().take(8192).collect();
                    self.event(if opcode == 4 {
                        Event::Layout { text }
                    } else {
                        Event::CenterPrint { text }
                    });
                }
                5 => {
                    for n in &mut self.inventory {
                        *n = r.i16()?;
                    }
                    self.inventory_pending = false;
                    self.inventory_updated = Some(Instant::now());
                    self.inventory_updates += 1;
                    ch.command("putaway".into()); // idempotent close, never toggles another inventory request
                }
                6 => (),
                7 => return Err("server disconnected bot".into()),
                8 => return Err("server requested reconnect".into()),
                9 => {
                    let flags = r.u8()?;
                    let index = r.u8()?;
                    let volume = if flags & 1 != 0 {
                        r.u8()? as f32 / 255.
                    } else {
                        1.
                    };
                    let attenuation = if flags & 2 != 0 {
                        r.u8()? as f32 / 64.
                    } else {
                        1.
                    };
                    let offset = if flags & 16 != 0 {
                        r.u8()? as f32 / 1000.
                    } else {
                        0.
                    };
                    let channel = if flags & 8 != 0 { Some(r.u16()?) } else { None };
                    let position = if flags & 4 != 0 {
                        Some([
                            r.i16()? as f32 / 8.,
                            r.i16()? as f32 / 8.,
                            r.i16()? as f32 / 8.,
                        ])
                    } else {
                        None
                    };
                    self.event(Event::Sound {
                        index,
                        volume,
                        attenuation,
                        offset,
                        entity: channel.map(|c| c >> 3),
                        channel: channel.map(|c| (c & 7) as u8),
                        position,
                    });
                }
                10 => {
                    let level = r.u8()?;
                    let s = r.string()?;
                    self.event(Event::Message {
                        level,
                        text: s.chars().take(8192).collect(),
                    });
                }
                11 => {
                    let s = r.string()?;
                    self.stuff(&s, ch);
                }
                12 => {
                    let protocol = r.i32()?;
                    if protocol != 34 {
                        return Err(format!("unsupported protocol {}", protocol));
                    }
                    self.spawn = r.i32()?;
                    r.u8()?;
                    let game = r.string()?;
                    self.slot = r.i16()?;
                    self.map = r.string()?;
                    self.map_epoch += 1;
                    self.game_dir = game;
                    self.events.clear();
                    self.frames.clear();
                    self.last_snapshot = None;
                    self.baselines.clear();
                    self.entities.clear();
                    self.player_names.fill(String::new());
                    self.player_skins.fill(String::new());
                    self.player_teams.fill(None);
                    self.images.fill(String::new());
                    self.sounds.fill(String::new());
                    self.models.fill(String::new());
                    self.last_frame = -1;
                    self.player = PlayerState::default();
                    self.max_clients = 9;
                    self.spawned = false;
                    self.inventory = [0; 256];
                    self.inventory_updated = None;
                    self.item_names.fill(String::new());
                    self.inventory_pending = false;
                    self.event(Event::MapChanged {
                        map: self.map.clone(),
                    });
                }
                13 => {
                    let n = r.u16()?;
                    let s = r.string()?;
                    if s.len() > 1024 {
                        return Err("oversized configstring".into());
                    }
                    if n == 33 {
                        self.map = s.clone();
                        self.models[1] = s;
                    } else if (1056..1312).contains(&n) {
                        self.item_names[n as usize - 1056] = s;
                    } else if n == 30 {
                        self.max_clients = s.parse::<usize>().unwrap_or(9).min(256);
                    } else if (32..288).contains(&n) {
                        self.models[n as usize - 32] = s;
                    } else if (288..544).contains(&n) {
                        self.sounds[n as usize - 288] = s;
                    } else if (544..800).contains(&n) {
                        self.images[n as usize - 544] = s;
                    } else if (1312..1568).contains(&n) {
                        self.player_skins[n as usize - 1312] = s
                            .split_once('\\')
                            .map(|(_, skin)| skin)
                            .unwrap_or("")
                            .into();
                        self.player_teams[n as usize - 1312] = Team::from_skin(&s);
                        self.player_names[n as usize - 1312] =
                            s.split('\\').next().unwrap_or("").to_string();
                    }
                }
                14 => {
                    if let Some(delta) = wire::entity_delta(&mut r)? {
                        if delta.number > 8191 {
                            return Err("entity index exceeds limit".into());
                        }
                        let base = self
                            .baselines
                            .get(&delta.number)
                            .cloned()
                            .unwrap_or_default();
                        self.baselines.insert(delta.number, delta.apply(&base));
                    }
                }
                16 => {
                    let n = r.i16()?;
                    r.u8()?;
                    if n > 0 {
                        r.take(n as usize)?;
                    }
                }
                20 => {
                    let frame = r.i32()?;
                    let delta = r.i32()?;
                    r.u8()?;
                    let area = r.u8()?;
                    r.take(area as usize)?;
                    if r.u8()? != 17 {
                        return Err("frame missing playerinfo".into());
                    }
                    let valid = delta <= 0 || self.frames.contains_key(&delta);
                    let mut snapshot = self.frames.get(&delta).cloned().unwrap_or_default();
                    wire::player(&mut r, &mut snapshot.player)?;
                    if r.u8()? != 18 {
                        return Err("frame missing packetentities".into());
                    }
                    while let Some(d) = wire::entity_delta(&mut r)? {
                        if d.number > 8191 {
                            return Err("entity index exceeds limit".into());
                        }
                        if d.remove {
                            snapshot.entities.remove(&d.number);
                        } else {
                            let base = snapshot
                                .entities
                                .get(&d.number)
                                .or_else(|| self.baselines.get(&d.number))
                                .cloned()
                                .unwrap_or_default();
                            snapshot.entities.insert(d.number, d.apply(&base));
                        }
                    }
                    if valid {
                        self.player = snapshot.player.clone();
                        self.entities = snapshot.entities.clone();
                        self.last_snapshot = Some(Instant::now());
                        self.frames.insert(frame, snapshot);
                        self.last_frame = frame;
                        self.spawned = true;
                        while self.frames.len() > 64 {
                            let first = *self.frames.keys().next().unwrap();
                            self.frames.remove(&first);
                        }
                    } else {
                        self.last_frame = -1;
                    }
                }
                n => return Err(format!("unsupported server opcode {} at {}", n, r.pos - 1)),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn serverdata(spawn: i32) -> Vec<u8> {
        let mut p = vec![12];
        p.extend(34i32.to_le_bytes());
        p.extend(spawn.to_le_bytes());
        p.push(0);
        p.extend(b"ctf\0");
        p.extend(0i16.to_le_bytes());
        p.extend(b"Test Map\0");
        p
    }
    #[test]
    fn map_change_clears_snapshot_and_inventory() {
        let mut game = Game::default();
        let mut channel = Channel::new(7);
        game.parse(&serverdata(1), &mut channel).unwrap();
        game.inventory[3] = 42;
        game.models[5] = "old model".into();
        game.player.stats[1] = 100;
        game.spawned = true;
        game.parse(&serverdata(2), &mut channel).unwrap();
        assert_eq!(game.map_epoch, 2);
        assert!(!game.spawned);
        assert_eq!(game.player.stats[1], 0);
        assert_eq!(game.inventory[3], 0);
        assert!(game.models[5].is_empty());
        assert!(game.last_snapshot.is_none());
    }
    #[test]
    fn event_history_is_bounded_and_ids_increase() {
        let mut game = Game::default();
        for _ in 0..1000 {
            game.event(Event::Message {
                level: 0,
                text: "hello".into(),
            });
        }
        assert_eq!(game.events.len(), 256);
        assert_eq!(game.events.front().unwrap().id, 745);
        assert_eq!(game.events.back().unwrap().id, 1000);
    }
    #[test]
    fn truncated_and_random_messages_do_not_panic() {
        let valid = serverdata(42);
        for n in 1..valid.len() {
            assert!(Game::default()
                .parse(&valid[..n], &mut Channel::new(1))
                .is_err());
        }
        let mut seed = 12345u32;
        for length in 0..256 {
            let packet: Vec<u8> = (0..length)
                .map(|_| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    (seed >> 24) as u8
                })
                .collect();
            let _ = Game::default().parse(&packet, &mut Channel::new(1));
        }
    }
    #[test]
    fn setup_text_does_not_execute_arbitrary_console_commands() {
        assert!(setup_command("configstrings 12 0"));
        assert!(setup_command("baselines 12 100"));
        for s in [
            "quit",
            "exec autoexec.cfg",
            "connect example.com",
            "configstrings 1 0; quit",
            "new extra",
        ] {
            assert!(!setup_command(s));
        }
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    fn frame(number: i32, delta: i32, entity: &[u8]) -> Vec<u8> {
        let mut b = vec![20];
        b.extend(number.to_le_bytes());
        b.extend(delta.to_le_bytes());
        b.extend([0, 0, 17]);
        b.extend(0u16.to_le_bytes());
        b.extend(2u32.to_le_bytes());
        b.extend(100i16.to_le_bytes());
        b.push(18);
        b.extend(entity);
        b.extend([0, 0]);
        b
    }
    #[test]
    fn entities_inherit_baselines_and_leave_visibility() {
        let mut g = Game::default();
        let mut c = Channel::new(1);
        g.baselines.insert(
            2,
            EntityState {
                number: 2,
                model: 255,
                origin: [10., 20., 30.],
                solid: 1,
                ..Default::default()
            },
        );
        g.parse(&frame(1, -1, &[0, 2]), &mut c).unwrap();
        assert_eq!(g.entities[&2].origin, [10., 20., 30.]);
        let mut change = vec![1, 2];
        change.extend(800i16.to_le_bytes());
        g.parse(&frame(2, 1, &change), &mut c).unwrap();
        assert_eq!(g.entities[&2].origin, [100., 20., 30.]);
        assert_eq!(g.entities[&2].model, 255);
        g.parse(&frame(3, 2, &[64, 2]), &mut c).unwrap();
        assert!(!g.entities.contains_key(&2));
    }
    #[test]
    fn missing_delta_base_requests_full_state() {
        let mut g = Game::default();
        g.parse(&frame(20, 19, &[]), &mut Channel::new(1)).unwrap();
        assert_eq!(g.last_frame, -1);
        assert!(g.last_snapshot.is_none());
    }
}

//! Classic Quake II protocol 34. GPL-2.0-or-later; see NOTICE.
use crate::salt::SALT;
use std::collections::VecDeque;

pub type Result<T> = std::result::Result<T, String>;
pub struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}
impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.data.len().saturating_sub(self.pos) {
            return Err(format!("truncated packet at {}, need {}", self.pos, n));
        }
        let out = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
    pub fn string(&mut self) -> Result<String> {
        let n = self.data[self.pos..]
            .iter()
            .position(|b| *b == 0)
            .ok_or("unterminated string")?;
        Ok(String::from_utf8_lossy(&self.take(n + 1)?[..n]).into_owned())
    }
}

#[derive(Default)]
pub struct Channel {
    pub outgoing: u32,
    pub incoming: u32,
    pub qport: u16,
    ack: u32,
    incoming_reliable: u32,
    reliable_ack: u32,
    reliable_sequence: u32,
    last_reliable: u32,
    reliable: Vec<u8>,
    pending: VecDeque<String>,
}
impl Channel {
    pub fn new(qport: u16) -> Self {
        Self {
            outgoing: 1,
            qport,
            ..Self::default()
        }
    }
    pub fn command(&mut self, s: String) {
        if self.pending.len() < 128 && !self.pending.contains(&s) {
            self.pending.push_back(s);
        }
    }
    pub fn receive<'a>(&mut self, data: &'a [u8]) -> Result<Option<&'a [u8]>> {
        let mut r = Reader::new(data);
        let seq = r.u32()?;
        let ack = r.u32()?;
        let number = seq & 0x7fff_ffff;
        if number <= self.incoming {
            return Ok(None);
        }
        self.incoming = number;
        self.ack = ack & 0x7fff_ffff;
        self.reliable_ack = ack >> 31;
        if self.reliable_ack == self.reliable_sequence {
            self.reliable.clear();
        }
        if seq >> 31 != 0 {
            self.incoming_reliable ^= 1;
        }
        Ok(Some(&data[8..]))
    }
    pub fn packet(&mut self, unreliable: &[u8]) -> Vec<u8> {
        let mut send_reliable =
            self.ack > self.last_reliable && self.reliable_ack != self.reliable_sequence;
        if self.reliable.is_empty() {
            if let Some(s) = self.pending.pop_front() {
                self.reliable.push(4);
                self.reliable.extend(s.as_bytes());
                self.reliable.push(0);
                self.reliable_sequence ^= 1;
                send_reliable = true;
            }
        }
        let mut out = Vec::new();
        out.extend((self.outgoing | ((send_reliable as u32) << 31)).to_le_bytes());
        out.extend((self.incoming | (self.incoming_reliable << 31)).to_le_bytes());
        out.extend(self.qport.to_le_bytes());
        if send_reliable {
            out.extend(&self.reliable);
            self.last_reliable = self.outgoing;
        }
        out.extend(unreliable);
        self.outgoing += 1;
        out
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct UserCmd {
    pub angles: [i16; 3],
    pub forward: i16,
    pub side: i16,
    pub up: i16,
    pub buttons: u8,
    pub msec: u8,
}
fn delta(out: &mut Vec<u8>, a: &UserCmd, b: &UserCmd) {
    let mut bits = 0u8;
    for i in 0..3 {
        if a.angles[i] != b.angles[i] {
            bits |= 1 << i;
        }
    }
    if a.forward != b.forward {
        bits |= 8;
    }
    if a.side != b.side {
        bits |= 16;
    }
    if a.up != b.up {
        bits |= 32;
    }
    if a.buttons != b.buttons {
        bits |= 64;
    }
    out.push(bits);
    for (i, angle) in b.angles.iter().enumerate() {
        if bits & (1 << i) != 0 {
            out.extend(angle.to_le_bytes());
        }
    }
    for (bit, value) in [(8, b.forward), (16, b.side), (32, b.up)] {
        if bits & bit != 0 {
            out.extend(value.to_le_bytes());
        }
    }
    if bits & 64 != 0 {
        out.push(b.buttons);
    }
    out.push(b.msec);
    out.push(128); // light level
}
pub fn checksum(data: &[u8], sequence: u32) -> u8 {
    let mut bytes = data[..data.len().min(60)].to_vec();
    let offset = sequence as usize % 1020;
    bytes.extend(&SALT[offset..offset + 4]);
    let mut crc = 0xffffu16;
    let mut sum = 0u32;
    for b in bytes {
        sum += b as u32;
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    (crc as u32 ^ sum) as u8
}
pub fn movement(frame: i32, sequence: u32, commands: &[UserCmd; 3]) -> Vec<u8> {
    let mut data = vec![2, 0];
    data.extend(frame.to_le_bytes());
    delta(&mut data, &UserCmd::default(), &commands[0]);
    delta(&mut data, &commands[0], &commands[1]);
    delta(&mut data, &commands[1], &commands[2]);
    data[1] = checksum(&data[2..], sequence);
    data
}

/// Persistent entity data reconstructed from the bot's own protocol-34 snapshots.
#[derive(Clone, Default, Debug)]
pub struct EntityState {
    pub number: u16,
    pub origin: [f32; 3],
    pub angles: [f32; 3],
    pub model: u8,
    pub frame: u16,
    pub solid: u16,
    pub skin: u32,
    pub effects: u32,
}
#[derive(Clone, Default, Debug)]
pub struct EntityDelta {
    pub number: u16,
    pub remove: bool,
    pub origin: [Option<f32>; 3],
    pub angles: [Option<f32>; 3],
    pub model: Option<u8>,
    pub frame: Option<u16>,
    pub solid: Option<u16>,
    pub skin: Option<u32>,
    pub effects: Option<u32>,
}
impl EntityDelta {
    pub fn apply(&self, base: &EntityState) -> EntityState {
        let mut e = base.clone();
        e.number = self.number;
        for i in 0..3 {
            if let Some(v) = self.origin[i] {
                e.origin[i] = v;
            }
            if let Some(v) = self.angles[i] {
                e.angles[i] = v;
            }
        }
        if let Some(v) = self.model {
            e.model = v;
        }
        if let Some(v) = self.frame {
            e.frame = v;
        }
        if let Some(v) = self.solid {
            e.solid = v;
        }
        if let Some(v) = self.skin {
            e.skin = v;
        }
        if let Some(v) = self.effects {
            e.effects = v;
        }
        e
    }
}
pub fn entity_delta(r: &mut Reader) -> Result<Option<EntityDelta>> {
    let mut b = r.u8()? as u32;
    if b & 128 != 0 {
        b |= (r.u8()? as u32) << 8;
    }
    if b & 32768 != 0 {
        b |= (r.u8()? as u32) << 16;
    }
    if b & (1 << 23) != 0 {
        b |= (r.u8()? as u32) << 24;
    }
    let number = if b & 256 != 0 {
        r.u16()?
    } else {
        r.u8()? as u16
    };
    if number == 0 {
        return Ok(None);
    }
    if number >= 1024 {
        return Err("invalid classic entity number".into());
    }
    let mut e = EntityDelta {
        number,
        remove: b & 64 != 0,
        ..Default::default()
    };
    if b & (1 << 11) != 0 {
        e.model = Some(r.u8()?);
    }
    for bit in [20, 21, 22] {
        if b & (1 << bit) != 0 {
            r.u8()?;
        }
    }
    if b & (1 << 4) != 0 {
        e.frame = Some(r.u8()? as u16);
    }
    if b & (1 << 17) != 0 {
        e.frame = Some(r.u16()?);
    }
    let skin = b & ((1 << 16) | (1 << 25));
    if skin != 0 {
        e.skin = Some(if skin == ((1 << 16) | (1 << 25)) {
            r.u32()?
        } else if skin & (1 << 16) != 0 {
            r.u8()? as u32
        } else {
            r.u16()? as u32
        });
    }
    for (lo, hi) in [(14, 19), (12, 18)] {
        let m = b & ((1 << lo) | (1 << hi));
        if m != 0 {
            let value = if m == ((1 << lo) | (1 << hi)) {
                r.u32()?
            } else if m & (1 << lo) != 0 {
                r.u8()? as u32
            } else {
                r.u16()? as u32
            };
            if lo == 14 {
                e.effects = Some(value);
            }
        }
    }
    for (bit, i) in [(0, 0), (1, 1), (9, 2)] {
        if b & (1 << bit) != 0 {
            e.origin[i] = Some(r.i16()? as f32 / 8.);
        }
    }
    for (bit, i) in [(10, 0), (2, 1), (3, 2)] {
        if b & (1 << bit) != 0 {
            e.angles[i] = Some(r.u8()? as f32 * 360. / 256.);
        }
    }
    if b & (1 << 24) != 0 {
        r.take(6)?;
    }
    if b & (1 << 26) != 0 {
        r.u8()?;
    }
    if b & (1 << 5) != 0 {
        r.u8()?;
    }
    if b & (1 << 27) != 0 {
        e.solid = Some(r.u16()?);
    }
    Ok(Some(e))
}

#[derive(Clone, Default, Debug)]
pub struct PlayerState {
    pub origin: [f32; 3],
    pub view_offset: [f32; 3],
    pub gun_index: u16,
    pub gun_frame: u8,
    pub velocity: [f32; 3],
    pub delta_angles: [i16; 3],
    pub view_angles: [u16; 3],
    pub stats: [i16; 32],
    pub pm_type: u8,
    pub pm_flags: u8,
    pub gravity: i16,
}
pub fn player(r: &mut Reader, p: &mut PlayerState) -> Result<()> {
    let b = r.u16()?;
    if b & 1 != 0 {
        p.pm_type = r.u8()?;
    }
    if b & 2 != 0 {
        for value in &mut p.origin {
            *value = r.i16()? as f32 / 8.;
        }
    }
    if b & 4 != 0 {
        for value in &mut p.velocity {
            *value = r.i16()? as f32 / 8.;
        }
    }
    if b & 8 != 0 {
        r.take(1)?;
    }
    if b & 16 != 0 {
        p.pm_flags = r.u8()?;
    }
    if b & 32 != 0 {
        p.gravity = r.i16()?;
    }
    if b & 64 != 0 {
        for value in &mut p.delta_angles {
            *value = r.i16()?;
        }
    }
    if b & 128 != 0 {
        for value in &mut p.view_offset {
            *value = r.u8()? as i8 as f32 * 0.25;
        }
    }
    if b & 256 != 0 {
        for value in &mut p.view_angles {
            *value = r.u16()?;
        }
    }
    if b & 512 != 0 {
        r.take(3)?;
    }
    if b & 4096 != 0 {
        p.gun_index = r.u8()? as u16;
    }
    if b & 8192 != 0 {
        p.gun_frame = r.u8()?;
        r.take(6)?;
    }
    if b & 1024 != 0 {
        r.take(4)?;
    }
    if b & 2048 != 0 {
        r.take(1)?;
    }
    if b & 16384 != 0 {
        r.take(1)?;
    }
    let stats = r.u32()?;
    for (i, value) in p.stats.iter_mut().enumerate() {
        if stats & (1 << i) != 0 {
            *value = r.i16()?;
        }
    }
    Ok(())
}

pub fn temp_entity(r: &mut Reader) -> Result<()> {
    let kind = r.u8()?;
    let len = match kind {
        0 | 1 | 2 | 4 | 9 | 12 | 13 | 14 | 26 | 30 | 42 | 43 | 44 | 46 | 55 => 7,
        10 | 15 | 25 | 29 => 9,
        3 | 11 | 23 | 27 | 31 | 34 | 41 => 12,
        5 | 6 | 7 | 8 | 17 | 18 | 20 | 21 | 22 | 28 | 35 | 45 | 47 | 48 | 49 | 51 | 52 | 53
        | 54 => 6,
        16 | 19 | 38 | 39 => 14,
        24 => 20,
        33 => 16,
        36 | 50 => 8,
        37 => 13,
        40 => {
            let id = r.i16()?;
            r.take(11)?;
            if id != -1 {
                r.take(4)?;
            }
            return Ok(());
        }
        _ => return Err(format!("unsupported temporary entity {}", kind)),
    };
    r.take(len)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn gun_animation_frame_is_decoded_without_shifting_inventory_stats() {
        let mut p = PlayerState::default();
        let mut bytes = 8192u16.to_le_bytes().to_vec();
        bytes.extend([11, 0, 0, 0, 0, 0, 0]);
        bytes.extend(2u32.to_le_bytes());
        bytes.extend(100i16.to_le_bytes());
        player(&mut Reader::new(&bytes), &mut p).unwrap();
        assert_eq!(p.gun_frame, 11);
        assert_eq!(p.stats[1], 100);
        let mut delta = 0u16.to_le_bytes().to_vec();
        delta.extend(0u32.to_le_bytes());
        player(&mut Reader::new(&delta), &mut p).unwrap();
        assert_eq!(p.gun_frame, 11);
    }

    use super::*;
    #[test]
    fn truncated_packets_fail() {
        assert!(Reader::new(&[1]).u16().is_err());
        assert!(Reader::new(b"bad").string().is_err());
    }
    #[test]
    fn stale_packet_and_reliable_ack() {
        let mut c = Channel::new(123);
        c.command("new".into());
        let p = c.packet(&[]);
        assert_eq!(u32::from_le_bytes(p[..4].try_into().unwrap()), 0x80000001);
        let mut reply = Vec::new();
        reply.extend(1u32.to_le_bytes());
        reply.extend(0x80000001u32.to_le_bytes());
        assert!(c.receive(&reply).unwrap().is_some());
        assert!(c.reliable.is_empty());
        assert!(c.receive(&reply).unwrap().is_none());
    }
    #[test]
    fn retransmit_after_later_ack() {
        let mut c = Channel::new(1);
        c.command("new".into());
        c.packet(&[]);
        let mut reply = Vec::new();
        reply.extend(1u32.to_le_bytes());
        reply.extend(2u32.to_le_bytes());
        c.receive(&reply).unwrap();
        let p = c.packet(&[]);
        assert_ne!(p[3] & 128, 0);
        assert_eq!(&p[10..], b"\x04new\0");
    }
    #[test]
    fn player_deltas_preserve_stats() {
        let mut p = PlayerState::default();
        let mut b = vec![2, 0];
        for n in [80i16, 160, 240] {
            b.extend(n.to_le_bytes());
        }
        b.extend(2u32.to_le_bytes());
        b.extend(100i16.to_le_bytes());
        player(&mut Reader::new(&b), &mut p).unwrap();
        assert_eq!(p.origin, [10., 20., 30.]);
        assert_eq!(p.stats[1], 100);
        player(&mut Reader::new(&[0, 0, 0, 0, 0, 0]), &mut p).unwrap();
        assert_eq!(p.stats[1], 100);
    }
    #[test]
    fn movement_timer_and_ground_flags_have_distinct_protocol_bits() {
        let mut b = vec![];
        b.extend((8u16 | 16 | 32).to_le_bytes());
        b.push(7);
        b.push(4);
        b.extend(800i16.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        let mut p = PlayerState::default();
        player(&mut Reader::new(&b), &mut p).unwrap();
        assert_eq!(p.pm_flags, 4);
        assert_eq!(p.gravity, 800);
        player(&mut Reader::new(&[0, 0, 0, 0, 0, 0]), &mut p).unwrap();
        assert_eq!(p.pm_flags, 4);
    }
    #[test]
    fn movement_crc_and_backups() {
        let cmd = UserCmd {
            forward: 200,
            msec: 50,
            ..Default::default()
        };
        let p = movement(-1, 9, &[cmd; 3]);
        assert_eq!(p[1], checksum(&p[2..], 9));
        assert_eq!(p[6], 8);
        assert_eq!(p[11], 0);
    }
}

#[cfg(test)]
mod ctf_effect_tests {
    use super::*;
    #[test]
    fn carrier_effects_decode_and_persist_until_explicitly_cleared() {
        // U_MOREBITS1|U_EFFECTS8, U_MOREBITS2, U_EFFECTS16: a full u32.
        let mut bytes = vec![128, 64 | 128, 8, 2];
        bytes.extend(0x80000u32.to_le_bytes());
        let delta = entity_delta(&mut Reader::new(&bytes)).unwrap().unwrap();
        let carrier = delta.apply(&EntityState::default());
        assert_eq!(carrier.effects, 0x80000);
        let unchanged = entity_delta(&mut Reader::new(&[0, 2]))
            .unwrap()
            .unwrap()
            .apply(&carrier);
        assert_eq!(unchanged.effects, 0x80000);
        let cleared = entity_delta(&mut Reader::new(&[128, 64, 2, 0]))
            .unwrap()
            .unwrap()
            .apply(&unchanged);
        assert_eq!(cleared.effects, 0);
    }
}

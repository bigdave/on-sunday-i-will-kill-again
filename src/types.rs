use crate::{EntityState, PlayerState};
use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

/// Classic CTF team selection, not an assignment of tactical roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Team {
    Red,
    Blue,
}
impl Team {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Blue => "blue",
        }
    }
    pub fn from_skin(s: &str) -> Option<Self> {
        match s.rsplit('/').next()? {
            "ctf_r" => Some(Self::Red),
            "ctf_b" => Some(Self::Blue),
            _ => None,
        }
    }
}
/// Classic base-game team-deathmatch grouping. Supply the server's rule;
/// protocol 34 does not advertise dmflags in the player snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathmatchTeamRule {
    Skin,
    Model,
}
impl DeathmatchTeamRule {
    /// DF_MODELTEAMS (128) takes precedence over DF_SKINTEAMS (64), as on the server.
    pub fn from_dmflags(flags: u32) -> Option<Self> {
        if flags & 128 != 0 {
            Some(Self::Model)
        } else if flags & 64 != 0 {
            Some(Self::Skin)
        } else {
            None
        }
    }
}
#[derive(Clone, Debug)]
pub struct Config {
    pub server: SocketAddr,
    pub name: String,
    pub skin: String,
    /// Classic CTF only. For base-game team deathmatch set `skin` and leave this None.
    pub team: Option<Team>,
    /// Optional ordinary game-server password. Never exposed in observations.
    pub password: Option<String>,
    pub connect_timeout: Duration,
    pub server_timeout: Duration,
    pub snapshot_timeout: Duration,
    /// Stop applying input unless the controller keeps submitting fresh actions.
    pub action_timeout: Duration,
    /// Poll the normal inventory command. None disables this convenience.
    pub inventory_interval: Option<Duration>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            server: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 27910),
            name: "RustPlayer".into(),
            skin: "male/grunt".into(),
            team: None,
            password: None,
            connect_timeout: Duration::from_secs(10),
            server_timeout: Duration::from_secs(15),
            snapshot_timeout: Duration::from_millis(500),
            action_timeout: Duration::from_millis(300),
            inventory_interval: Some(Duration::from_secs(2)),
        }
    }
}
impl Config {
    pub fn new(server: SocketAddr, name: impl Into<String>) -> Self {
        Self {
            server,
            name: name.into(),
            ..Self::default()
        }
    }
    pub(crate) fn validate(&self) -> Result<(), crate::Error> {
        for (label, value, max) in [
            ("name", self.name.as_str(), 32),
            ("skin", self.skin.as_str(), 64),
            ("password", self.password.as_deref().unwrap_or(""), 64),
        ] {
            if value.len() > max
                || value
                    .chars()
                    .any(|c| !c.is_ascii() || c.is_ascii_control() || matches!(c, '\\' | '"' | ';'))
                || (label == "name" && value.is_empty())
            {
                return Err(crate::Error::Configuration(format!("invalid {label}")));
            }
        }
        if self.connect_timeout.is_zero()
            || self.server_timeout.is_zero()
            || self.snapshot_timeout.is_zero()
            || self.action_timeout.is_zero()
            || self.action_timeout > Duration::from_secs(5)
            || self.inventory_interval == Some(Duration::ZERO)
        {
            return Err(crate::Error::Configuration(
                "invalid timeout/interval".into(),
            ));
        }
        Ok(())
    }
}
/// World-space degrees, corrected for the server's delta-angle offsets.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewAngles {
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
}
/// One ordinary player's input. Positive side movement is RIGHT; up=200 jumps,
/// up=-200 crouches/dives. Releasing up is required between repeated jumps.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Action {
    pub view: Option<ViewAngles>,
    pub forward: i16,
    pub side: i16,
    pub up: i16,
    /// Also supplies BUTTON_ANY, so callers can explicitly respawn/leave intermission.
    pub attack: bool,
    pub use_weapon: Option<String>,
}
impl Action {
    pub(crate) fn validate(&self) -> Result<(), crate::Error> {
        if self
            .view
            .map(|v| ![v.pitch, v.yaw, v.roll].iter().all(|n| n.is_finite()))
            .unwrap_or(false)
        {
            return Err(crate::Error::Configuration("non-finite view angle".into()));
        }
        if let Some(w) = &self.use_weapon {
            if w.is_empty()
                || w.len() > 48
                || !w
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b' ' | b'_' | b'-'))
            {
                return Err(crate::Error::Configuration("invalid weapon name".into()));
            }
        }
        Ok(())
    }
}
/// Rejects input from another client, reconnection or map incarnation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Context {
    pub client_id: u64,
    pub connection_epoch: u64,
    pub map_epoch: u64,
}
#[derive(Clone, Debug)]
pub struct PlayerInfo {
    pub slot: usize,
    pub entity_id: u16,
    pub name: String,
    pub skin: String,
    pub team: Option<Team>,
}
impl PlayerInfo {
    /// Exact classic team key from the configured model/skin rule, not a tactic.
    /// Empty slots have no team. This does not apply custom mod team semantics.
    pub fn deathmatch_team(&self, rule: DeathmatchTeamRule) -> Option<&str> {
        if self.name.is_empty() || self.skin.is_empty() {
            return None;
        }
        Some(match (rule, self.skin.split_once('/')) {
            (DeathmatchTeamRule::Model, Some((model, _))) => model,
            (DeathmatchTeamRule::Skin, Some((_, skin))) => skin,
            (_, None) => self.skin.as_str(),
        })
    }
}
#[derive(Clone, Debug)]
pub struct Inventory {
    pub counts: [i16; 256],
    pub age: Duration,
    pub updates: u64,
}
/// Direct classic CTF HUD values. No inferred locations, roles or strategy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtfHud {
    pub red_flag_icon: String,
    pub blue_flag_icon: String,
    pub carried_flag_icon: String,
    pub red_captures: i16,
    pub blue_captures: i16,
}
#[derive(Clone, Debug)]
pub enum Event {
    Message {
        level: u8,
        text: String,
    },
    CenterPrint {
        text: String,
    },
    Layout {
        text: String,
    },
    Sound {
        index: u8,
        volume: f32,
        attenuation: f32,
        offset: f32,
        entity: Option<u16>,
        channel: Option<u8>,
        position: Option<[f32; 3]>,
    },
    MapChanged {
        map: String,
    },
    MuzzleFlash {
        entity: u16,
        flash: u8,
        monster: bool,
    },
    /// Protocol-34 effect discriminator and uninterpreted bytes following it.
    TemporaryEntity {
        kind: u8,
        data: Vec<u8>,
    },
    ProtocolWarning {
        message: String,
    },
}
#[derive(Clone, Debug)]
pub struct EventRecord {
    pub id: u64,
    pub map_epoch: u64,
    pub event: Event,
}
#[derive(Clone, Debug, Default)]
pub struct Observation {
    pub context: Context,
    /// Local publication sequence, not the server frame number.
    pub sequence: u64,
    pub connected: bool,
    /// Spawned and holding a recent valid player snapshot. Dead players may be ready.
    pub ready: bool,
    pub frame: i32,
    pub game_dir: String,
    pub map: String,
    pub slot: Option<usize>,
    pub player: PlayerState,
    /// Network snapshot entities, NOT visibility-filtered vision. See docs.
    pub entities: BTreeMap<u16, EntityState>,
    pub players: Vec<PlayerInfo>,
    pub models: Vec<String>,
    pub images: Vec<String>,
    pub sounds: Vec<String>,
    pub item_names: Vec<String>,
    pub inventory: Option<Inventory>,
    pub snapshot_age: Option<Duration>,
    /// Rolling bounded history; use event IDs to avoid handling a record twice.
    pub events: Vec<EventRecord>,
    pub decode_errors: u64,
    pub error: Option<String>,
}
impl Observation {
    pub fn health(&self) -> i16 {
        self.player.stats[1]
    }
    pub fn ammo(&self) -> i16 {
        self.player.stats[3]
    }
    pub fn armor(&self) -> i16 {
        self.player.stats[5]
    }
    pub fn score(&self) -> i16 {
        self.player.stats[14]
    }
    /// Classic CTF skin-based red/blue identification, not base-game deathmatch teams.
    pub fn team(&self) -> Option<Team> {
        self.slot
            .and_then(|s| self.players.get(s))
            .and_then(|p| p.team)
    }
    pub fn weapon_model(&self) -> Option<&str> {
        self.models
            .get(self.player.gun_index as usize)
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }
    pub fn ctf_hud(&self) -> Option<CtfHud> {
        let image = |i: usize| {
            self.images
                .get(self.player.stats[i].max(0) as usize)
                .cloned()
                .unwrap_or_default()
        };
        let red = image(17);
        let blue = image(19);
        if self.game_dir != "ctf" && self.team().is_none() && red.is_empty() && blue.is_empty() {
            return None;
        }
        Some(CtfHud {
            red_flag_icon: red,
            blue_flag_icon: blue,
            carried_flag_icon: image(21),
            red_captures: self.player.stats[18],
            blue_captures: self.player.stats[20],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classic_deathmatch_team_rules_match_server_keys() {
        let p = PlayerInfo {
            slot: 0,
            entity_id: 1,
            name: "Player".into(),
            skin: "female/grunt".into(),
            team: None,
        };
        assert_eq!(p.deathmatch_team(DeathmatchTeamRule::Skin), Some("grunt"));
        assert_eq!(p.deathmatch_team(DeathmatchTeamRule::Model), Some("female"));
        assert_eq!(DeathmatchTeamRule::from_dmflags(0), None);
        assert_eq!(
            DeathmatchTeamRule::from_dmflags(64 | 256),
            Some(DeathmatchTeamRule::Skin)
        );
        assert_eq!(
            DeathmatchTeamRule::from_dmflags(64 | 128),
            Some(DeathmatchTeamRule::Model)
        );
        assert_eq!(
            PlayerInfo {
                name: String::new(),
                ..p.clone()
            }
            .deathmatch_team(DeathmatchTeamRule::Skin),
            None
        );
        assert_eq!(
            PlayerInfo {
                skin: "noslash".into(),
                ..p
            }
            .deathmatch_team(DeathmatchTeamRule::Skin),
            Some("noslash")
        );
    }
    #[test]
    fn unsafe_text_and_nonfinite_inputs_are_rejected() {
        for name in ["", "a;quit", "a\\password\\b", "a\nquit", "a\"b"] {
            assert!(Config::new("127.0.0.1:27910".parse().unwrap(), name)
                .validate()
                .is_err());
        }
        for weapon in ["", "Blaster;quit", "Rocket Launcher\nquit", "\"Blaster\""] {
            assert!(Action {
                use_weapon: Some(weapon.into()),
                ..Default::default()
            }
            .validate()
            .is_err());
        }
        assert!(Action {
            view: Some(ViewAngles {
                yaw: f32::NAN,
                ..Default::default()
            }),
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(Action {
            use_weapon: Some("Super Shotgun".into()),
            ..Default::default()
        }
        .validate()
        .is_ok());
    }
}

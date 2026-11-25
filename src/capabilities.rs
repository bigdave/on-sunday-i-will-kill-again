//! Worker-owned capability state: neither repeated reads nor submissions advance it.
use crate::{
    Action, Context, Event, Observation, PlayerCapabilities, PlayerState, SoundPan, ViewAngles,
};
use std::{
    collections::{BTreeMap, VecDeque},
    time::{Duration, Instant},
};

const DELAY: Duration = Duration::from_millis(200);
const V3_DELAY: Duration = Duration::from_millis(100);
const DT: f32 = 0.05;
const SPEED: f32 = 360.;
const ACCEL: f32 = 1440.;

pub(crate) struct Capabilities {
    profile: Option<PlayerCapabilities>,
    context: Option<Context>,
    pending: VecDeque<(Instant, Observation)>,
    sound_pans: BTreeMap<u64, Option<SoundPan>>,
    view: Option<ViewAngles>,
    velocity: [f32; 2],
    delta: Option<[i16; 3]>,
    alive: bool,
}
fn wrap(a: f32) -> f32 {
    (a + 180.).rem_euclid(360.) - 180.
}
fn player_view(p: &PlayerState) -> ViewAngles {
    ViewAngles {
        pitch: p.view_angles[0] as i16 as f32 * 360. / 65536.,
        yaw: p.view_angles[1] as f32 * 360. / 65536.,
        roll: 0.,
    }
}
impl Capabilities {
    pub(crate) fn new(profile: Option<PlayerCapabilities>) -> Self {
        Self {
            profile,
            context: None,
            pending: VecDeque::new(),
            sound_pans: BTreeMap::new(),
            view: None,
            velocity: [0.; 2],
            delta: None,
            alive: false,
        }
    }
    fn reset_context(&mut self, context: Context) {
        if self.context != Some(context) {
            self.context = Some(context);
            self.pending.clear();
            self.sound_pans.clear();
            self.view = None;
            self.velocity = [0.; 2];
            self.delta = None;
        }
    }
    pub(crate) fn action(
        &mut self,
        mut action: Action,
        p: &PlayerState,
        context: Context,
    ) -> Action {
        if self.profile.is_none() {
            return action;
        }
        self.reset_context(context);
        let alive = p.stats[1] > 0;
        if self.view.is_none() || self.delta != Some(p.delta_angles) || (alive && !self.alive) {
            self.view = Some(player_view(p));
            self.velocity = [0.; 2];
        }
        self.delta = Some(p.delta_angles);
        self.alive = alive;
        let mut view = self.view.unwrap();
        if self.profile == Some(PlayerCapabilities::HumanLimitationsV4) {
            let max_step = SPEED * DT;
            // Use the same pitch bounds as the ordinary command encoder.
            view.pitch = view.pitch.clamp(-89., 89.);
            if let Some(target) = action.view {
                let pitch_error = target.pitch.clamp(-89., 89.) - view.pitch;
                let yaw_error = wrap(wrap(target.yaw) - view.yaw);
                view.pitch += pitch_error.clamp(-max_step, max_step);
                view.yaw = wrap(view.yaw + yaw_error.clamp(-max_step, max_step));
                view.roll = target.roll;
            } else {
                view.roll = f32::from(p.view_angles[2] as i16) * 360. / 65536.;
            }
            self.view = Some(view);
            action.view = Some(view);
            return action;
        }
        {
            let target = action.view.unwrap_or(view);
            let error = [
                target.pitch.clamp(-89., 89.) - view.pitch,
                wrap(target.yaw) - wrap(view.yaw),
            ];
            let error = [error[0], wrap(error[1])];
            let distance = error[0].hypot(error[1]);
            // Brake as the target approaches; angular momentum may overshoot.
            let desired_speed = SPEED.min((2. * ACCEL * distance).sqrt()).min(distance / DT);
            let desired = if distance > 0. {
                [
                    error[0] / distance * desired_speed,
                    error[1] / distance * desired_speed,
                ]
            } else {
                [0.; 2]
            };
            let change = [desired[0] - self.velocity[0], desired[1] - self.velocity[1]];
            let scale = (ACCEL * DT / change[0].hypot(change[1]).max(f32::EPSILON)).min(1.);
            for (i, change) in change.iter().enumerate() {
                self.velocity[i] += change * scale;
            }
            view.pitch = (view.pitch + self.velocity[0] * DT).clamp(-89., 89.);
            view.yaw = wrap(view.yaw + self.velocity[1] * DT);
            // Preserve requested world-space travel while the camera catches up.
            let angle = (wrap(target.yaw) - view.yaw).to_radians();
            let forward = action.forward.clamp(-400, 400) as f32;
            let side = action.side.clamp(-400, 400) as f32;
            action.forward = (forward * angle.cos() + side * angle.sin())
                .round()
                .clamp(-400., 400.) as i16;
            action.side = (-forward * angle.sin() + side * angle.cos())
                .round()
                .clamp(-400., 400.) as i16;
        }
        view.roll = 0.;
        self.view = Some(view);
        action.view = Some(view);
        action
    }
    pub(crate) fn observation(&mut self, mut raw: Observation, now: Instant) -> Observation {
        let Some(profile) = self.profile else {
            return raw;
        };
        let delay = match profile {
            PlayerCapabilities::HumanLimitationsV1 | PlayerCapabilities::HumanLimitationsV2 => {
                DELAY
            }
            PlayerCapabilities::HumanLimitationsV3 => V3_DELAY,
            PlayerCapabilities::HumanLimitationsV4 => return raw,
        };
        self.reset_context(raw.context);
        let sequence = raw.sequence;
        let neutral = Observation {
            context: raw.context,
            sequence,
            connected: raw.connected,
            map: raw.map.clone(),
            game_dir: raw.game_dir.clone(),
            error: raw.error.clone(),
            ..Default::default()
        };
        if !raw.ready {
            self.pending.clear();
            return neutral;
        }
        if let Some(view) = self.view {
            raw.player.view_angles = [view.pitch, view.yaw, view.roll]
                .map(|v| (v.rem_euclid(360.) * 65536. / 360.) as u16);
        }
        let current_player = matches!(
            profile,
            PlayerCapabilities::HumanLimitationsV2 | PlayerCapabilities::HumanLimitationsV3
        )
        .then(|| raw.player.clone());
        let sound_pans =
            (profile == PlayerCapabilities::HumanLimitationsV3).then_some(&mut self.sound_pans);
        filter(&mut raw, sound_pans);
        self.pending.push_back((now, raw));
        while self.pending.len() > 8 {
            self.pending.pop_front();
        }
        let mut delivered = None;
        while self
            .pending
            .front()
            .map(|(t, _)| now.duration_since(*t) >= delay)
            .unwrap_or(false)
        {
            delivered = self.pending.pop_front();
        }
        if let Some((at, mut observation)) = delivered {
            let age = now.duration_since(at);
            observation.sequence = sequence;
            if let Some(current) = current_player {
                let player = &mut observation.player;
                player.origin = current.origin;
                player.velocity = current.velocity;
                player.view_angles = current.view_angles;
                player.view_offset = current.view_offset;
                player.delta_angles = current.delta_angles;
                player.pm_type = current.pm_type;
                player.pm_flags = current.pm_flags;
                player.gravity = current.gravity;
            }
            if let Some(a) = &mut observation.snapshot_age {
                *a += age;
            }
            if let Some(inv) = &mut observation.inventory {
                inv.age += age;
            }
            observation
        } else {
            neutral
        }
    }
}
fn stereo_pan(source: [f32; 3], eye: [f32; 3], right: [f32; 3]) -> SoundPan {
    let delta = std::array::from_fn::<_, 3, _>(|i| source[i] - eye[i]);
    let distance = delta[0].hypot(delta[1]).hypot(delta[2]);
    let lateral = (0..3).map(|i| delta[i] * right[i]).sum::<f32>();
    if lateral > 0.25 * distance {
        SoundPan::Right
    } else if lateral < -0.25 * distance {
        SoundPan::Left
    } else {
        SoundPan::Center
    }
}

fn filter(o: &mut Observation, mut sound_pans: Option<&mut BTreeMap<u64, Option<SoundPan>>>) {
    let eye = std::array::from_fn::<_, 3, _>(|i| o.player.origin[i] + o.player.view_offset[i]);
    let view = player_view(&o.player);
    let (sy, cy) = view.yaw.to_radians().sin_cos();
    let (sp, cp) = view.pitch.to_radians().sin_cos();
    let forward = [cp * cy, cp * sy, -sp];
    let right = [sy, -cy, 0.];
    let up = [sp * cy, sp * sy, cp];
    let own_entity = o.slot.map(|slot| slot + 1);
    if let Some(history) = sound_pans.as_deref_mut() {
        history.retain(|id, _| o.events.first().is_some_and(|oldest| *id >= oldest.id));
    }
    // Resolve sounds from the server snapshot before applying visual FOV.
    // An event appears in multiple snapshots, so keep its original stereo cue.
    for record in &mut o.events {
        if let Event::Sound {
            volume,
            attenuation,
            entity,
            position,
            channel,
            pan,
            ..
        } = &mut record.event
        {
            *pan = sound_pans.as_deref_mut().and_then(|history| {
                *history.entry(record.id).or_insert_with(|| {
                    if *volume == 0.
                        || *attenuation == 0.
                        || entity.is_some_and(|id| Some(usize::from(id)) == own_entity)
                    {
                        return None;
                    }
                    let source = position.or_else(|| {
                        entity.and_then(|id| o.entities.get(&id).map(|state| state.origin))
                    })?;
                    Some(stereo_pan(source, eye, right))
                })
            });
            *entity = None;
            *position = None;
            *channel = None;
        }
    }
    o.entities.retain(|id, e| {
        if Some(usize::from(*id)) == own_entity {
            return true;
        }
        let d = std::array::from_fn::<_, 3, _>(|i| e.origin[i] - eye[i]);
        let dot = |v: [f32; 3]| (0..3).map(|i| d[i] * v[i]).sum::<f32>();
        let depth = dot(forward);
        depth > 0.
            && dot(right).abs() <= depth * 55f32.to_radians().tan()
            && dot(up).abs() <= depth * 40f32.to_radians().tan()
    });
    o.events.retain(|r| match &r.event {
        Event::MuzzleFlash { entity, .. } => o.entities.contains_key(entity),
        // Uninterpreted effect payloads can contain hidden world coordinates.
        Event::TemporaryEntity { .. } => false,
        _ => true,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntityState, EventRecord, Inventory};
    fn limited() -> Capabilities {
        Capabilities::new(Some(PlayerCapabilities::human_limitations_v1()))
    }
    fn snapshot(frame: i32) -> Observation {
        let mut o = Observation {
            connected: true,
            ready: true,
            frame,
            sequence: frame as u64,
            snapshot_age: Some(Duration::from_millis(10)),
            ..Default::default()
        };
        o.player.stats[1] = 100;
        o
    }
    #[test]
    fn disabled_preserves_actions_and_complete_observations() {
        let mut c = Capabilities::new(None);
        let a = Action {
            view: Some(ViewAngles {
                pitch: 30.,
                yaw: 270.,
                roll: 8.,
            }),
            forward: 123,
            side: -241,
            up: 200,
            attack: true,
            use_weapon: Some("Railgun".into()),
        };
        assert_eq!(
            c.action(a.clone(), &PlayerState::default(), Context::default()),
            a
        );
        let mut o = snapshot(1);
        o.entities.insert(
            2,
            EntityState {
                origin: [-100., 0., 0.],
                ..Default::default()
            },
        );
        o.events.push(EventRecord {
            id: 1,
            map_epoch: 0,
            event: Event::TemporaryEntity {
                kind: 1,
                data: vec![1, 2, 3],
            },
        });
        let before = format!("{o:?}");
        assert_eq!(format!("{:?}", c.observation(o, Instant::now())), before);
    }

    #[test]
    fn v4_caps_pitch_and_yaw_independently_and_preserves_other_inputs() {
        let mut c = Capabilities::new(Some(PlayerCapabilities::human_limitations_v4()));
        let p = snapshot(1).player;
        let context = Context::default();
        let mut action = Action {
            view: Some(ViewAngles {
                pitch: 60.,
                yaw: 90.,
                roll: 45.,
            }),
            forward: 123,
            side: -241,
            up: 200,
            attack: true,
            use_weapon: Some("Railgun".into()),
        };
        let mut expected = action.clone();
        expected.view.as_mut().unwrap().pitch = 18.;
        expected.view.as_mut().unwrap().yaw = 18.;
        assert_eq!(c.action(action.clone(), &p, context), expected);

        action.view.as_mut().unwrap().pitch = -60.;
        action.view.as_mut().unwrap().yaw = 90.;
        let view = c.action(action, &p, context).view.unwrap();
        assert_eq!((view.pitch, view.yaw), (0., 36.));

        let view = c.action(Action::default(), &p, context).view.unwrap();
        assert_eq!((view.pitch, view.yaw), (0., 36.));
        let mut p = p;
        p.delta_angles[1] = 16384;
        p.view_angles[1] = 32768;
        let view = c
            .action(
                Action {
                    view: Some(ViewAngles {
                        yaw: -175.,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                &p,
                context,
            )
            .view
            .unwrap();
        assert_eq!(view.yaw, -175.);
    }

    #[test]
    fn v4_publishes_the_decoded_observation_without_delay_or_filtering() {
        let mut c = Capabilities::new(Some(PlayerCapabilities::human_limitations_v4()));
        let mut o = snapshot(1);
        o.player.kick_angles = [1., 2., 3.];
        o.entities.insert(
            2,
            EntityState {
                origin: [-100., 0., 0.],
                ..Default::default()
            },
        );
        o.events.push(EventRecord {
            id: 1,
            map_epoch: 0,
            event: Event::Sound {
                index: 2,
                volume: 1.,
                attenuation: 1.,
                offset: 0.,
                entity: Some(2),
                channel: Some(3),
                position: Some([-100., 0., 0.]),
                pan: None,
            },
        });
        o.events.push(EventRecord {
            id: 2,
            map_epoch: 0,
            event: Event::TemporaryEntity {
                kind: 1,
                data: vec![1, 2, 3],
            },
        });
        c.action(
            Action {
                view: Some(ViewAngles {
                    yaw: 90.,
                    ..Default::default()
                }),
                ..Default::default()
            },
            &o.player,
            o.context,
        );
        let before = format!("{o:?}");
        assert_eq!(format!("{:?}", c.observation(o, Instant::now())), before);
    }

    #[test]
    fn observations_wait_full_delay_and_keep_causal_contents() {
        let mut c = limited();
        let start = Instant::now();
        for n in 0..4 {
            let o = c.observation(snapshot(n), start + Duration::from_millis(n as u64 * 50));
            assert!(!o.ready);
            assert!(o.entities.is_empty());
        }
        let mut first = limited();
        let mut o = snapshot(42);
        o.inventory = Some(Inventory {
            counts: [0; 256],
            age: Duration::from_millis(5),
            updates: 1,
        });
        first.observation(o, start);
        let got = first.observation(snapshot(43), start + DELAY);
        assert!(got.ready);
        assert_eq!(got.frame, 42);
        assert_eq!(got.sequence, 43);
        assert_eq!(got.snapshot_age, Some(Duration::from_millis(210)));
        assert_eq!(got.inventory.unwrap().age, Duration::from_millis(205));
        let got = c.observation(snapshot(4), start + DELAY);
        assert!(got.ready);
        assert_eq!(got.frame, 0);
    }
    #[test]
    fn v2_refreshes_local_motion_but_delays_filtered_world_and_events() {
        let start = Instant::now();
        let mut c = Capabilities::new(Some(PlayerCapabilities::human_limitations_v2()));
        let mut old = snapshot(10);
        old.slot = Some(0);
        old.player.origin = [10., 0., 0.];
        old.player.kick_angles = [-2., 0., 1.];
        old.inventory = Some(Inventory {
            counts: [0; 256],
            age: Duration::ZERO,
            updates: 1,
        });
        old.entities.insert(
            2,
            EntityState {
                origin: [110., 0., 0.],
                ..Default::default()
            },
        );
        old.entities.insert(
            3,
            EntityState {
                origin: [-100., 0., 0.],
                ..Default::default()
            },
        );
        old.events.push(EventRecord {
            id: 1,
            map_epoch: 0,
            event: Event::Message {
                level: 1,
                text: "old".into(),
            },
        });
        let mut fresh = old.clone();
        fresh.sequence = 12;
        fresh.frame = 12;
        fresh.player.origin = [40., 0., -8.];
        fresh.player.velocity = [250., 0., -60.];
        fresh.player.view_angles[1] = 4000;
        fresh.player.kick_angles = [3., 0., -1.];
        fresh.player.pm_flags = 16;
        fresh.player.stats[1] = 80;
        fresh.entities.get_mut(&2).unwrap().origin = [140., 0., 0.];
        fresh.events.push(EventRecord {
            id: 2,
            map_epoch: 0,
            event: Event::Message {
                level: 1,
                text: "fresh".into(),
            },
        });
        fresh.inventory.as_mut().unwrap().updates = 2;
        assert!(!c.observation(old, start).ready);
        let got = c.observation(fresh, start + DELAY);
        assert!(got.ready);
        assert_eq!(got.frame, 10);
        assert_eq!(got.sequence, 12);
        assert_eq!(got.player.origin, [40., 0., -8.]);
        assert_eq!(got.player.velocity, [250., 0., -60.]);
        assert_eq!(got.player.view_angles[1], 4000);
        assert_eq!(got.player.kick_angles, [-2., 0., 1.]);
        assert_eq!(got.player.pm_flags, 16);
        assert_eq!(got.health(), 100);
        assert_eq!(got.entities[&2].origin, [110., 0., 0.]);
        assert!(!got.entities.contains_key(&3));
        assert_eq!(got.events.len(), 1);
        assert_eq!(got.events[0].id, 1);
        assert_eq!(got.inventory.unwrap().updates, 1);
    }
    #[test]
    fn v3_publishes_world_at_100_ms_with_fresh_local_motion() {
        let mut c = Capabilities::new(Some(PlayerCapabilities::human_limitations_v3()));
        let start = Instant::now();
        let mut old = snapshot(10);
        old.player.kick_angles = [-2., 0., 1.];
        old.events.push(EventRecord {
            id: 1,
            map_epoch: 0,
            event: Event::Message {
                level: 1,
                text: "delayed".into(),
            },
        });
        assert!(!c.observation(old, start).ready);
        let early = c.observation(snapshot(11), start + Duration::from_millis(99));
        assert!(!early.ready);
        assert!(early.events.is_empty());
        assert_eq!(early.player.kick_angles, [0.; 3]);
        let mut fresh = snapshot(12);
        fresh.player.origin = [32., 0., 0.];
        fresh.player.kick_angles = [3., 0., -1.];
        let got = c.observation(fresh, start + Duration::from_millis(100));
        assert!(got.ready);
        assert_eq!(got.frame, 10);
        assert_eq!(got.player.origin, [32., 0., 0.]);
        assert_eq!(got.player.kick_angles, [-2., 0., 1.]);
        assert_eq!(got.snapshot_age, Some(Duration::from_millis(110)));
        assert_eq!(got.events.len(), 1);
        assert_eq!(got.events[0].id, 1);
    }
    #[test]
    fn map_reconnect_and_stale_snapshot_flush_pending_information() {
        for reconnect in [false, true] {
            let mut c = limited();
            let now = Instant::now();
            c.observation(snapshot(1), now);
            let mut next = snapshot(2);
            if reconnect {
                next.context.connection_epoch = 1;
            } else {
                next.context.map_epoch = 1;
            }
            let got = c.observation(next.clone(), now + DELAY);
            assert!(!got.ready);
            assert_eq!(got.context, next.context);
            next.ready = false;
            assert!(!c.observation(next, now + DELAY + DELAY).ready);
            assert!(c.pending.is_empty());
        }
    }
    #[test]
    fn fov_and_event_filter_remove_unseen_spatial_information() {
        let mut o = snapshot(1);
        o.slot = Some(0);
        for (id, pos) in [
            (1, [-10., 0., 0.]),
            (2, [100., 0., 0.]),
            (3, [-100., 0., 0.]),
            (4, [100., 200., 0.]),
            (5, [100., 0., 200.]),
        ] {
            o.entities.insert(
                id,
                EntityState {
                    number: id,
                    origin: pos,
                    ..Default::default()
                },
            );
        }
        o.events = vec![
            EventRecord {
                id: 1,
                map_epoch: 0,
                event: Event::Sound {
                    index: 2,
                    volume: 1.,
                    attenuation: 1.,
                    offset: 0.,
                    entity: Some(3),
                    channel: Some(1),
                    position: Some([-100., 0., 0.]),
                    pan: None,
                },
            },
            EventRecord {
                id: 2,
                map_epoch: 0,
                event: Event::MuzzleFlash {
                    entity: 3,
                    flash: 1,
                    monster: false,
                },
            },
            EventRecord {
                id: 3,
                map_epoch: 0,
                event: Event::TemporaryEntity {
                    kind: 1,
                    data: vec![1],
                },
            },
            EventRecord {
                id: 4,
                map_epoch: 0,
                event: Event::Message {
                    level: 1,
                    text: "hello".into(),
                },
            },
        ];
        filter(&mut o, None);
        assert_eq!(o.entities.keys().copied().collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(o.events.len(), 2);
        assert!(matches!(
            o.events[0].event,
            Event::Sound {
                entity: None,
                position: None,
                channel: None,
                pan: None,
                ..
            }
        ));
        o.player.view_angles[1] = 32768;
        o.entities.insert(
            3,
            EntityState {
                origin: [-100., 0., 0.],
                ..Default::default()
            },
        );
        filter(&mut o, None);
        assert!(o.entities.contains_key(&3));
        assert!(!o.entities.contains_key(&2));
    }

    #[test]
    fn v3_delivers_stereo_sound_from_outside_fov_without_source_metadata() {
        let mut c = Capabilities::new(Some(PlayerCapabilities::human_limitations_v3()));
        let now = Instant::now();
        let mut world = snapshot(1);
        world.entities.insert(
            2,
            EntityState {
                origin: [-100., 100., 0.],
                ..Default::default()
            },
        );
        for (id, entity, position) in [
            (1, Some(2), None),
            (2, None, Some([-100., -100., 0.])),
            (3, None, Some([-100., 0., 0.])),
            (4, None, None),
        ] {
            world.events.push(EventRecord {
                id,
                map_epoch: 0,
                event: Event::Sound {
                    index: 2,
                    volume: 1.,
                    attenuation: 1.,
                    offset: 0.,
                    entity,
                    channel: Some(1),
                    position,
                    pan: None,
                },
            });
        }
        let cues = |o: Observation| {
            o.events
                .into_iter()
                .map(|event| match event.event {
                    Event::Sound {
                        entity,
                        position,
                        channel,
                        pan,
                        ..
                    } => {
                        assert_eq!((entity, position, channel), (None, None, None));
                        pan
                    }
                    event => panic!("expected sound, got {event:?}"),
                })
                .collect::<Vec<_>>()
        };
        assert!(c.observation(world.clone(), now).events.is_empty());
        let delayed = c.observation(world.clone(), now + V3_DELAY);
        assert!(delayed.entities.is_empty());
        let expected = [
            Some(SoundPan::Left),
            Some(SoundPan::Right),
            Some(SoundPan::Center),
            None,
        ];
        assert_eq!(cues(delayed), expected);
        world.player.view_angles[1] = 32768;
        c.observation(world.clone(), now + V3_DELAY * 2);
        assert_eq!(cues(c.observation(world, now + V3_DELAY * 3)), expected);
    }
    #[test]
    fn turning_is_bounded_across_wraparound_reversals_and_release() {
        let mut c = limited();
        let p = snapshot(1).player;
        let ctx = Context::default();
        let mut previous = 0.;
        let mut velocity = 0.;
        for n in 0..80 {
            let target = if n < 20 {
                Some(170.)
            } else if n < 40 {
                Some(-170.)
            } else if n < 60 {
                Some(0.)
            } else {
                None
            };
            let a = c.action(
                Action {
                    view: target.map(|yaw| ViewAngles {
                        yaw,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                &p,
                ctx,
            );
            let yaw = a.view.unwrap().yaw;
            let v = wrap(yaw - previous) / DT;
            assert!(v.abs() <= SPEED + 0.01, "speed {v}");
            assert!(
                (v - velocity).abs() <= ACCEL * DT + 0.02,
                "acceleration {}",
                (v - velocity) / DT
            );
            previous = yaw;
            velocity = v;
        }
        let mut c = limited();
        let mut p = p;
        p.view_angles[1] = (179. * 65536. / 360.) as u16;
        let out = c.action(
            Action {
                view: Some(ViewAngles {
                    yaw: -179.,
                    ..Default::default()
                }),
                ..Default::default()
            },
            &p,
            ctx,
        );
        assert!(wrap(out.view.unwrap().yaw - 179.).abs() < 4.);
    }
    #[test]
    fn delayed_reads_do_not_advance_camera_and_server_teleports_reset_it() {
        let mut c = limited();
        let mut p = snapshot(1).player;
        let target = Action {
            view: Some(ViewAngles {
                yaw: 90.,
                roll: 45.,
                ..Default::default()
            }),
            forward: 400,
            ..Default::default()
        };
        let a = c.action(target.clone(), &p, Context::default());
        let yaw = a.view.unwrap().yaw;
        assert!(yaw > 0. && yaw < 4.);
        assert_eq!(a.view.unwrap().roll, 0.);
        // Desired +Y travel remains +Y after conversion into the limited view's axes.
        let angle = yaw.to_radians();
        assert!((a.forward as f32 * angle.cos() + a.side as f32 * angle.sin()).abs() < 2.);
        assert!(a.forward as f32 * angle.sin() - a.side as f32 * angle.cos() > 398.);
        let now = Instant::now();
        for _ in 0..100 {
            c.observation(snapshot(1), now);
        }
        assert_eq!(c.view.unwrap().yaw, yaw);
        assert!(c.pending.len() <= 8);
        p.delta_angles[1] = 16384;
        p.view_angles[1] = 16384;
        assert_eq!(
            c.action(target, &p, Context::default()).view.unwrap().yaw,
            90.
        );
    }
}

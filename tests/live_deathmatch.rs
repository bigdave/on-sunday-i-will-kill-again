//! Opt-in classic base-game deathmatch and team-deathmatch tests.
use on_sunday_i_will_kill_again::{
    Action, Client, Config, DeathmatchTeamRule, Error, Observation, ViewAngles,
};
use std::{
    env,
    fs::{self, File},
    io::Write,
    net::{SocketAddr, UdpSocket},
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Server {
    child: Child,
    log: PathBuf,
    address: SocketAddr,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Server {
    fn new(flags: u32, label: &str) -> Self {
        let port = UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let log = env::temp_dir().join(format!("os-iwka-{label}-{}.log", std::process::id()));
        let f = File::create(&log).unwrap();
        let child = Command::new(env::var("Q2_TEST_ENGINE").expect("Q2_TEST_ENGINE"))
            .args([
                "+set",
                "basedir",
                &env::var("Q2_TEST_DATA").expect("Q2_TEST_DATA"),
                "+set",
                "sys_forcegamelib",
                &env::var("Q2_TEST_DM_GAME").expect("Q2_TEST_DM_GAME"),
                "+set",
                "game",
                "baseq2",
                "+set",
                "net_ip",
                "127.0.0.1",
                "+set",
                "net_port",
                &port.to_string(),
                "+set",
                "deathmatch",
                "1",
                "+set",
                "dmflags",
                &flags.to_string(),
                "+set",
                "maxclients",
                "8",
                "+set",
                "sv_iplimit",
                "8",
                "+set",
                "timelimit",
                "0",
                "+set",
                "fraglimit",
                "0",
                "+set",
                "public",
                "0",
                "+map",
                "q2dm1",
            ])
            .stdin(Stdio::piped())
            .stdout(f.try_clone().unwrap())
            .stderr(f)
            .spawn()
            .unwrap();
        let mut s = Self {
            child,
            log,
            address: format!("127.0.0.1:{port}").parse().unwrap(),
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !fs::read_to_string(&s.log)
            .unwrap_or_default()
            .contains("Q2PRO initialized")
        {
            assert!(
                s.child.try_wait().unwrap().is_none(),
                "server exited: {}",
                s.log.display()
            );
            assert!(
                Instant::now() < deadline,
                "startup timeout: {}",
                s.log.display()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        s
    }
    fn command(&mut self, text: &str) {
        writeln!(self.child.stdin.as_mut().unwrap(), "{text}").unwrap();
        self.child.stdin.as_mut().unwrap().flush().unwrap();
    }
    fn client(&self, name: &str, skin: &str) -> Client {
        let mut c = Config::new(self.address, name);
        c.skin = skin.into();
        c.inventory_interval = Some(Duration::from_millis(250));
        Client::connect(c).unwrap()
    }
}
fn until(client: &mut Client, seconds: u64, p: impl Fn(&Observation) -> bool) -> Observation {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if let Some(o) = client.wait_for_update(Duration::from_millis(100)).unwrap() {
            if p(&o) {
                return o;
            }
        }
    }
    let o = client.observation();
    panic!(
        "timeout: map={} ready={} hp={} position={:?} score={} weapon={:?} errors={} terminal={:?}",
        o.map,
        o.ready,
        o.health(),
        o.player.origin,
        o.score(),
        o.weapon_model(),
        o.decode_errors,
        o.error
    );
}
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>()
        .sqrt()
}
fn pair(server: &mut Server, a: &mut Client, b: &mut Client) {
    server.command("sv dmcheck_pair Shooter Target");
    until(b, 5, |o| {
        o.ready
            && distance(o.player.origin, a.observation().player.origin) > 100.
            && distance(o.player.origin, a.observation().player.origin) < 140.
    });
}
fn shoot(
    a: &mut Client,
    b: &Client,
    seconds: f32,
    done: impl Fn(&Observation, &Observation) -> bool,
) -> bool {
    let end = Instant::now() + Duration::from_secs_f32(seconds);
    while Instant::now() < end {
        let o = a
            .wait_for_update(Duration::from_millis(100))
            .unwrap()
            .unwrap();
        let t = b.observation();
        if done(&o, &t) {
            a.submit(&o, Action::default()).unwrap();
            return true;
        }
        let dx = t.player.origin[0] - o.player.origin[0];
        let dy = t.player.origin[1] - o.player.origin[1];
        let dz = t.player.origin[2] + t.player.view_offset[2]
            - o.player.origin[2]
            - o.player.view_offset[2];
        a.submit(
            &o,
            Action {
                view: Some(ViewAngles {
                    yaw: dy.atan2(dx).to_degrees(),
                    pitch: -dz.atan2(dx.hypot(dy)).to_degrees(),
                    roll: 0.,
                }),
                attack: true,
                use_weapon: Some("Machinegun".into()),
                ..Default::default()
            },
        )
        .unwrap();
    }
    a.submit(&a.observation(), Action::default()).unwrap();
    false
}
fn exercise(flags: u32, label: &str, skins: [&str; 2], friendly: bool) {
    let mut server = Server::new(flags, label);
    let mut a = server.client("Shooter", skins[0]);
    let mut b = server.client("Target", skins[1]);
    let start = until(&mut a, 15, |o| {
        o.ready && o.health() > 0 && o.map.contains("q2dm1")
    });
    until(&mut b, 15, |o| o.ready && o.health() > 0);
    assert!(start.game_dir.is_empty() || start.game_dir == "baseq2");
    assert!(start.ctf_hud().is_none());
    assert_ne!(a.observation().slot, b.observation().slot);
    until(&mut a, 5, |o| {
        o.players
            .iter()
            .any(|p| p.name == "Target" && p.skin == skins[1])
    });
    if let Some(rule) = DeathmatchTeamRule::from_dmflags(flags) {
        let o = a.observation();
        let shooter = o.players.iter().find(|p| p.name == "Shooter").unwrap();
        let target = o.players.iter().find(|p| p.name == "Target").unwrap();
        assert_eq!(
            shooter.deathmatch_team(rule) == target.deathmatch_team(rule),
            friendly
        );
    }
    server.command("sv dmcheck_loadout Shooter \"Machinegun\"");
    let armed = until(&mut a, 5, |o| {
        o.weapon_model().is_some_and(|s| s.contains("machn"))
            && o.inventory
                .as_ref()
                .is_some_and(|i| i.counts.iter().any(|n| *n > 1))
    });
    a.submit(
        &armed,
        Action {
            use_weapon: Some("Blaster".into()),
            ..Default::default()
        },
    )
    .unwrap();
    until(&mut a, 5, |o| {
        o.weapon_model().is_some_and(|s| s.contains("v_blast"))
    });
    pair(&mut server, &mut a, &mut b);
    // Move a short distance toward the fixture's clear line, then stop refreshing.
    let original = a.observation();
    let p = original.player.origin;
    let q = b.observation().player.origin;
    a.submit(
        &original,
        Action {
            view: Some(ViewAngles {
                yaw: (q[1] - p[1]).atan2(q[0] - p[0]).to_degrees(),
                ..Default::default()
            }),
            forward: 100,
            ..Default::default()
        },
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(700));
    let stopped = a.observation();
    std::thread::sleep(Duration::from_millis(400));
    assert!(distance(p, stopped.player.origin) > 2., "no movement");
    assert!(
        distance(stopped.player.origin, a.observation().player.origin) < 2.,
        "expired input moved"
    );
    assert_eq!(
        a.submit(&original, Action::default()),
        Err(Error::StaleObservation)
    );
    server.command("sv dmcheck_health Target 20");
    until(&mut b, 5, |o| o.health() == 20);
    let before = a.observation().score();
    if friendly {
        assert!(
            !shoot(&mut a, &b, 1.5, |_, t| t.health() < 20),
            "friendly damage was not blocked"
        );
        assert_eq!(b.observation().health(), 20);
        assert_eq!(a.observation().score(), before);
        // Enabling friendly damage must turn the same shot into a teamkill penalty.
        server.command(&format!("dmflags {}", flags & !256));
        assert!(
            shoot(&mut a, &b, 5., |o, t| t.health() <= 0
                && o.score() == before - 1),
            "teamkill penalty missing"
        );
    } else {
        assert!(
            shoot(&mut a, &b, 5., |o, t| t.health() <= 0
                && o.score() == before + 1),
            "enemy kill/damage/score missing"
        );
    }
    let weapon = a.observation();
    assert!(
        weapon.weapon_model().is_some_and(|s| s.contains("machn")),
        "weapon switch did not reach server"
    );
    assert!(
        weapon.ammo() > 0 && weapon.ammo() < 200,
        "weapon ammo did not decrease"
    );
    let dead = b.observation();
    assert!(dead.health() <= 0);
    // The policy, not the interface, requests respawn by pressing attack.
    let deadline = Instant::now() + Duration::from_secs(6);
    while b.observation().health() <= 0 && Instant::now() < deadline {
        let o = b
            .wait_for_update(Duration::from_millis(100))
            .unwrap()
            .unwrap();
        b.submit(
            &o,
            Action {
                attack: true,
                ..Default::default()
            },
        )
        .unwrap();
    }
    assert!(b.observation().health() > 0, "explicit respawn failed");
    server.command("gamemap q2dm2");
    let changed = until(&mut a, 15, |o| o.ready && o.map.contains("q2dm2"));
    until(&mut b, 15, |o| o.ready && o.map.contains("q2dm2"));
    assert_ne!(changed.context, original.context);
    assert_eq!(
        a.submit(&original, Action::default()),
        Err(Error::StaleObservation)
    );
    assert_eq!(changed.decode_errors, 0);
    assert_eq!(b.observation().decode_errors, 0);
    a.close();
    b.close();
    assert!(!a.observation().connected);
    assert!(!b.observation().connected);
    eprintln!("PASS {label}: {}", server.log.display());
}
#[test]
#[ignore = "requires Q2_TEST_ENGINE, Q2_TEST_DATA and Q2_TEST_DM_GAME"]
fn deathmatch() {
    exercise(0, "deathmatch", ["male/grunt", "male/grunt"], false);
}
#[test]
#[ignore = "requires Q2_TEST_ENGINE, Q2_TEST_DATA and Q2_TEST_DM_GAME"]
fn skin_teams() {
    exercise(
        64 | 256,
        "skin-enemies",
        ["male/grunt", "male/major"],
        false,
    );
    exercise(
        64 | 256,
        "skin-friendly",
        ["male/grunt", "female/grunt"],
        true,
    );
}
#[test]
#[ignore = "requires Q2_TEST_ENGINE, Q2_TEST_DATA and Q2_TEST_DM_GAME"]
fn model_teams() {
    exercise(
        128 | 256,
        "model-enemies",
        ["male/grunt", "female/athena"],
        false,
    );
    exercise(
        128 | 256,
        "model-friendly",
        ["male/grunt", "male/major"],
        true,
    );
}

//! Opt-in integration test. See docs/testing.md for external asset requirements.
use quake2_player::{Action, Client, Config, Error, Event, Observation, Team, ViewAngles};
use std::{
    env,
    fs::File,
    io::Write,
    net::UdpSocket,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Server(Child);
impl Server {
    fn command(&mut self, s: &str) {
        writeln!(self.0.stdin.as_mut().unwrap(), "{s}").unwrap();
        self.0.stdin.as_mut().unwrap().flush().unwrap();
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn until(
    client: &mut Client,
    seconds: u64,
    predicate: impl Fn(&Observation) -> bool,
) -> Observation {
    let end = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < end {
        if let Some(o) = client.wait_for_update(Duration::from_millis(200)).unwrap() {
            if predicate(&o) {
                return o;
            }
        }
    }
    let o = client.observation();
    panic!("condition timed out: context={:?}, map={}, ready={}, health={}, weapon={:?}, errors={}, terminal={:?}",o.context,o.map,o.ready,o.health(),o.weapon_model(),o.decode_errors,o.error);
}
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>()
        .sqrt()
}
#[test]
#[ignore = "requires separately installed Q2PRO, legal game assets, and fixture module"]
fn classic_ctf_lifecycle() {
    let engine = env::var("Q2_TEST_ENGINE").expect("Q2_TEST_ENGINE");
    let data = env::var("Q2_TEST_DATA").expect("Q2_TEST_DATA");
    let library = env::var("Q2_TEST_GAME").expect("Q2_TEST_GAME");
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let log_path = env::temp_dir().join(format!("quake2-player-test-{}.log", std::process::id()));
    let log = File::create(&log_path).unwrap();
    let mut server = Server(
        Command::new(engine)
            .args([
                "+set",
                "basedir",
                &data,
                "+set",
                "sys_forcegamelib",
                &library,
                "+set",
                "game",
                "ctf",
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
                "ctf",
                "1",
                "+set",
                "competition",
                "0",
                "+set",
                "maxclients",
                "9",
                "+set",
                "sv_iplimit",
                "8",
                "+set",
                "timelimit",
                "0",
                "+set",
                "capturelimit",
                "0",
                "+set",
                "public",
                "0",
                "+map",
                "q2ctf6",
            ])
            .stdin(Stdio::piped())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if std::fs::read_to_string(&log_path)
            .unwrap_or_default()
            .contains("Q2PRO initialized")
        {
            break;
        }
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "server exited; see {}",
            log_path.display()
        );
        assert!(
            Instant::now() < deadline,
            "server startup timed out; see {}",
            log_path.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let address = format!("127.0.0.1:{port}").parse().unwrap();
    let config = |name: &str, team| {
        let mut c = Config::new(address, name);
        c.team = Some(team);
        c.inventory_interval = Some(Duration::from_millis(250));
        c
    };
    let mut red = Client::connect(config("InterfaceRed", Team::Red)).unwrap();
    let mut blue = Client::connect(config("InterfaceBlue", Team::Blue)).unwrap();
    let original = until(&mut red, 15, |o| {
        o.ready && o.team() == Some(Team::Red) && o.ctf_hud().is_some()
    });
    until(&mut blue, 15, |o| o.ready && o.team() == Some(Team::Blue));
    assert_ne!(red.observation().slot, blue.observation().slot);
    server.command("sv ctfcheck_place InterfaceRed -1152 2400 24.125");
    server.command("sv ctfcheck_place InterfaceBlue -552 2400 40.125");
    server.command("sv ctfcheck_loadout InterfaceRed \"Machinegun\"");
    let start = until(&mut red, 5, |o| {
        o.ready && (o.player.origin[0] + 1152.).abs() < 5.
    });
    // A single action must stop by itself while application code does no polling.
    red.submit(
        &start,
        Action {
            view: Some(ViewAngles {
                yaw: 0.,
                ..Default::default()
            }),
            forward: 200,
            ..Default::default()
        },
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(700));
    let stopped = red.observation();
    std::thread::sleep(Duration::from_millis(500));
    let idle = red.observation();
    assert!(
        distance(start.player.origin, stopped.player.origin) > 5.,
        "movement did not reach server"
    );
    assert!(
        distance(stopped.player.origin, idle.player.origin) < 2.,
        "expired input kept moving"
    );
    assert!(
        idle.connected && idle.ready,
        "worker did not keep connection alive"
    );
    red.submit(
        &red.observation(),
        Action {
            use_weapon: Some("Blaster".into()),
            ..Default::default()
        },
    )
    .unwrap();
    until(&mut red, 5, |o| {
        o.weapon_model().is_some_and(|s| s.contains("v_blast"))
    });
    server.command("sv ctfcheck_health InterfaceBlue 10000");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut fired = false;
    while Instant::now() < deadline {
        let o = red
            .wait_for_update(Duration::from_millis(100))
            .unwrap()
            .unwrap();
        if o.events.iter().any(|e|matches!(e.event,Event::MuzzleFlash{entity,flash:1,..} if Some(entity as usize)==o.slot.map(|s|s+1))){fired=true;break;}
        red.submit(
            &o,
            Action {
                view: Some(ViewAngles {
                    yaw: 0.,
                    ..Default::default()
                }),
                attack: true,
                use_weapon: Some("Machinegun".into()),
                ..Default::default()
            },
        )
        .unwrap();
    }
    assert!(fired, "no firing event");
    let armed = until(&mut red, 5, |o| {
        o.weapon_model().is_some_and(|s| s.contains("machn")) && o.inventory.is_some()
    });
    assert!(armed
        .inventory
        .as_ref()
        .unwrap()
        .counts
        .iter()
        .any(|n| *n > 1));
    assert_eq!(armed.decode_errors, 0);
    assert_eq!(
        red.submit(&original, Action::default()),
        Err(Error::StaleObservation)
    );
    server.command("gamemap q2ctf2");
    let changed = until(&mut red, 20, |o| o.ready && o.map.contains("q2ctf2"));
    until(&mut blue, 20, |o| o.ready && o.map.contains("q2ctf2"));
    assert_ne!(changed.context, armed.context);
    assert_eq!(
        red.submit(&armed, Action::default()),
        Err(Error::StaleObservation)
    );
    assert_eq!(changed.decode_errors, 0);
    red.close();
    blue.close();
    assert!(!red.observation().connected);
    assert_eq!(red.submit(&changed, Action::default()), Err(Error::Closed));
    eprintln!("CTF lifecycle passed; server log: {}", log_path.display());
}

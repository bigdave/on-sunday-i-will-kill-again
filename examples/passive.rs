//! Connect as an ordinary player, report state, then disconnect. No AI.
use quake2_player::{Client, Config, Team};
use std::{
    error::Error,
    time::{Duration, Instant},
};
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let address = args
        .next()
        .unwrap_or_else(|| "127.0.0.1:27910".into())
        .parse()?;
    let mut config = Config::new(address, args.next().unwrap_or_else(|| "RustPlayer".into()));
    config.team = match args.next().as_deref() {
        Some("red") => Some(Team::Red),
        Some("blue") => Some(Team::Blue),
        None => None,
        Some(_) => return Err("team must be red or blue".into()),
    };
    let mut client = Client::connect(config)?;
    let end = Instant::now() + Duration::from_secs(30);
    let mut last_frame = -1;
    while Instant::now() < end {
        if let Some(o) = client.wait_for_update(Duration::from_millis(250))? {
            if !o.connected {
                return Err(o.error.unwrap_or_else(|| "disconnected".into()).into());
            }
            if o.ready && o.frame >= last_frame + 10 {
                println!(
                    "map={} team={:?} health={} position={:?} score={} ctf={:?}",
                    o.map,
                    o.team(),
                    o.health(),
                    o.player.origin,
                    o.score(),
                    o.ctf_hud()
                );
                last_frame = o.frame;
            }
        }
    }
    client.close();
    Ok(())
}

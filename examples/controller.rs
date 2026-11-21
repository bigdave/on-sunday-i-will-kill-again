//! Deliberately inert policy scaffold. Replace decide() with your bot logic.
use quake2_player::{Action, Client, Config, Observation, Team};
use std::{error::Error, time::Duration};
fn decide(_observation: &Observation) -> Action {
    // Your policy owns navigation, perception, weapons, tactics and respawning.
    Action::default()
}
fn main() -> Result<(), Box<dyn Error>> {
    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:27910".into())
        .parse()?;
    let mut config = Config::new(address, "MyBot");
    config.team = Some(Team::Red);
    let mut client = Client::connect(config)?;
    loop {
        if let Some(o) = client.wait_for_update(Duration::from_millis(100))? {
            if !o.connected {
                return Err(o.error.unwrap_or_else(|| "disconnected".into()).into());
            }
            if o.ready {
                client.submit(&o, decide(&o))?;
            }
        }
    }
}

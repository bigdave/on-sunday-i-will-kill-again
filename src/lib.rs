//! A headless ordinary-player connection for classic Quake II (protocol 34).
//!
//! This crate owns UDP setup, reliable commands, snapshot decoding and timed
//! player input. It owns no navigation, collision map, map-based visibility,
//! combat decisions, team coordination, game assets or monitoring integration.
//!
//! See `examples/passive.rs` for the minimum client lifecycle.
#![doc = include_str!("../README.md")]
mod capabilities;
mod client;
mod game;
mod salt;
mod types;
mod wire;
/// Version of the linked interface implementation.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub use client::Client;
pub use types::*;
pub use wire::{EntityState, PlayerState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Configuration(String),
    Io(String),
    Protocol(String),
    Disconnected(String),
    Timeout,
    StaleObservation,
    Closed,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Configuration(s) => write!(f, "configuration: {s}"),
            Self::Io(s) => write!(f, "network: {s}"),
            Self::Protocol(s) => write!(f, "protocol: {s}"),
            Self::Disconnected(s) => write!(f, "disconnected: {s}"),
            Self::Timeout => write!(f, "server timed out"),
            Self::StaleObservation => write!(f, "action refers to a stale observation"),
            Self::Closed => write!(f, "client is closed"),
        }
    }
}
impl std::error::Error for Error {}

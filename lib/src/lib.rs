//! Camera discovery shared by the app (app/) and the CLI (cli/):
//! legacy mDNS and FDP discovery, merging and TCP health checks (`scanner`),
//! Change IP and identify (`actions`) and trust on first use (`trust`).

pub mod actions;
pub mod client;
pub mod mdns;
pub mod messages;
pub mod scanner;
pub mod trust;
pub mod types;

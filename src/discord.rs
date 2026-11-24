use std::{collections::HashMap, path::PathBuf, sync::OnceLock, time::Duration};

use futures::{SinkExt, StreamExt, channel::mpsc};
use serde_json::{Value, json};
use tokio::runtime::Runtime;
use tokio_tungstenite::tungstenite::Message as WsMessage;

mod activity;
pub use activity::*;
use activity::{launch_error, parse_message_activity};
#[cfg(test)]
use activity::{oauth_code, parse_shelf};

const GATEWAY: &str = "wss://gateway.discord.gg/?v=10&encoding=json";
pub const API: &str = "https://discord.com/api/v10";
pub(crate) const CDN: &str = "https://cdn.discordapp.com";
pub(crate) const VIEW_CHANNEL: u64 = 1 << 10;

// The user-account gateway and several fields in these modules (nameplates, guild tags, member list ops,
// read states) are undocumented; shapes are from memory of the official client. Not verified live.

pub mod perm;

mod client;
mod gateway;
mod guild;
mod http;
mod identity;
mod message;
mod parse;
mod pacing;
mod parse_chat;
mod people;
mod profile;
#[cfg(test)]
mod tests;

pub use client::*;
pub use guild::*;
pub use http::*;
pub use identity::{identity, load_build_number};
pub use message::*;
pub use parse::*;
pub use parse_chat::*;
pub use people::*;
pub use profile::*;
use gateway::*;

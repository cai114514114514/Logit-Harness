//! Versioned local transport. Public views never include model contexts.
mod client;
mod protocol;
mod server;
pub use client::{check_protocol, receive, request, subscribe, subscribe_target};
pub use protocol::{ClientMessage, PROTOCOL, ServerMessage};
pub use server::Server;

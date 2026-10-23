//! Terminal lifetime never owns the runner. All control goes through the durable socket inbox.
mod activity;
mod brand;
mod chrome;
mod client;
mod clipboard;
mod composer;
mod delivery;
mod edit;
mod history;
mod input;
mod jobs;
mod launch;
mod menu;
mod output;
mod ownership;
mod present;
mod render;
mod setup;
mod state;
mod status;
mod submission;
mod surface;
mod team;
#[cfg(test)]
mod team_tests;
mod text;
mod tool_text;
mod transcript;
mod transport;

pub use input::Input;
pub use launch::configure;
pub use launch::{Launch, welcome};
pub use render::render;
pub use state::Recent;
pub use state::Screen;
pub use transcript::Transcript;
pub use transport::{attach, attach_view};

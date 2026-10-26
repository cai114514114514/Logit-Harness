//! Terminal lifetime never owns the runner. All control goes through the durable socket inbox.
mod activity;
mod brand;
mod chrome;
mod client;
mod clipboard;
mod composer;
mod contract;
mod contract_bar;
mod delivery;
mod edit;
mod feedback;
mod history;
mod input;
mod jobs;
mod launch;
mod ledger;
mod menu;
mod output;
mod ownership;
mod present;
mod render;
#[cfg(test)]
mod review_tests;
mod setup;
mod state;
mod status;
mod submission;
mod surface;
mod team;
#[cfg(test)]
mod team_tests;
mod text;
mod theme;
mod tool_text;
mod transcript;
mod transport;

pub use input::Input;
pub use launch::configure;
pub use launch::{Launch, welcome};
pub use render::{height as active_height, render};
pub use state::Recent;
pub use state::Screen;
pub use transcript::Transcript;
pub use transport::{attach, attach_view};

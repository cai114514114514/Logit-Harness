//! Terminal lifetime never owns the runner. All control goes through the durable socket inbox.
mod activity;
mod brand;
mod client;
mod clipboard;
mod edit;
mod history;
mod input;
mod jobs;
mod launch;
mod render;
mod setup;
mod state;
mod submission;
mod surface;
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
pub use transport::attach;

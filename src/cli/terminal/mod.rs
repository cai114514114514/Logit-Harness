//! Terminal lifetime never owns the runner. All control goes through the durable socket inbox.
mod actions;
mod archive;
mod brand;
mod edit;
mod input;
mod launch;
mod menu;
mod render;
mod setup;
mod state;
mod surface;
mod text;
mod transcript;
mod transport;

pub use input::{Input, Intent, parse};
pub use launch::configure;
pub use launch::{Launch, welcome};
pub use render::render;
pub use state::Recent;
pub use state::Screen;
pub use transport::attach;

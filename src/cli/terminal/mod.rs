//! Terminal lifetime never owns the runner. All control goes through the durable socket inbox.
mod actions;
mod input;
mod render;
mod state;
mod transport;

pub use input::{Input, Intent, parse};
pub use render::render;
pub use state::Screen;
pub use transport::attach;

//! Source-readonly development processes, separate from foreground commands.
mod manager;
mod tool;
pub use manager::{ServiceInfo, Services};
pub use tool::tools;

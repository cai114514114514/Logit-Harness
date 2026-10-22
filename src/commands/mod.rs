//! Terminal-independent commands. Discovery, help and parsing share DIRECTORY;
//! executing a local command never constructs a provider or runs a model.
mod capabilities;
mod catalog;
mod completion;
mod inspect;
mod parse;
mod service;

pub use catalog::{DIRECTORY, Entry, Kind, Timing, help, matches};
pub use completion::Completion;
pub use inspect::doctor;
pub use parse::{Command, parse};
pub use service::{History, Navigation, Outcome, Service};

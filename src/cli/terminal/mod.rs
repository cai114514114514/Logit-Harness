//! Terminal lifetime never owns the runner. All control goes through the durable socket inbox.
//!
//! Files are grouped by responsibility; every module stays a direct child of
//! this one, so `super::name` paths read the same from any file:
//!
//! - this folder: the client loop, screen state, launch and transport;
//! - `layout/`: the measured active region and how output reaches the terminal;
//! - `transcript/`: the event projection and how settled history is presented;
//! - `modal/`: temporary full-screen pages;
//! - `input/`: the editor and clipboard;
//! - `requests/`: submitted work, its delivery and feedback;
//! - `setup/`: the configuration form;
//! - `tests/`: unit tests that span several modules.
mod client;
mod launch;
mod state;
mod team;
mod transport;

#[path = "layout/activity.rs"]
mod activity;
#[path = "layout/brand.rs"]
mod brand;
#[path = "layout/chrome.rs"]
mod chrome;
#[path = "layout/composer.rs"]
mod composer;
#[path = "layout/contract_bar.rs"]
mod contract_bar;
#[path = "layout/menu.rs"]
mod menu;
#[path = "layout/output.rs"]
mod output;
#[path = "layout/render.rs"]
mod render;
#[path = "layout/status.rs"]
mod status;
#[path = "layout/surface.rs"]
mod surface;
#[path = "layout/text.rs"]
mod text;
#[path = "layout/theme.rs"]
mod theme;

#[path = "transcript/ledger.rs"]
mod ledger;
#[path = "transcript/ownership.rs"]
mod ownership;
#[path = "transcript/present.rs"]
mod present;
#[path = "transcript/tool_text.rs"]
mod tool_text;
#[path = "transcript/transcript.rs"]
mod transcript;

#[path = "modal/contract.rs"]
mod contract;
#[path = "modal/history.rs"]
mod history;

#[path = "input/clipboard.rs"]
mod clipboard;
#[path = "input/edit.rs"]
mod edit;
mod input;

#[path = "requests/delivery.rs"]
mod delivery;
#[path = "requests/feedback.rs"]
mod feedback;
#[path = "requests/jobs.rs"]
mod jobs;
#[path = "requests/submission.rs"]
mod submission;

mod setup;

#[cfg(test)]
#[path = "tests/review_tests.rs"]
mod review_tests;
#[cfg(test)]
#[path = "tests/team_tests.rs"]
mod team_tests;

pub use input::Input;
pub use launch::configure;
pub use launch::{Launch, welcome};
pub use render::{height as active_height, render};
pub use state::Recent;
pub use state::Screen;
pub use transcript::Transcript;
pub use transport::{attach, attach_view};

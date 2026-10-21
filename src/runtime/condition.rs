//! Experimental mechanism switches. Normal sessions always default to the full controller.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum Condition {
    Prompt,
    Replan,
    Tracking,
    #[default]
    Dynamic,
    NoStagnation,
}
impl Condition {
    pub fn cancels_active(self) -> bool {
        matches!(self, Self::Replan | Self::Dynamic | Self::NoStagnation)
    }
    pub fn verifies(self) -> bool {
        self != Self::Prompt
    }
    pub fn diagnoses(self) -> bool {
        self != Self::NoStagnation
    }
}

//! Each tool retains only the relay tail the runner has not durably consumed.
use crate::tools::ToolEvent;
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct Log {
    sequence: usize,
    pending: VecDeque<(usize, ToolEvent)>,
}
impl Log {
    pub fn push(&mut self, event: ToolEvent) -> usize {
        let index = self.sequence;
        self.sequence += 1;
        self.pending.push_back((index, event));
        index
    }
    pub fn acknowledge(&mut self, index: usize) {
        while self.pending.front().is_some_and(|(next, _)| *next <= index) {
            self.pending.pop_front();
        }
    }
    pub fn drain(&mut self) -> VecDeque<(usize, ToolEvent)> {
        std::mem::take(&mut self.pending)
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.pending.len()
    }
}

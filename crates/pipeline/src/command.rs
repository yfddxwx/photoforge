//! Command implementation for the non-destructive edit state (ADR-0009).

use crate::mask::Mask;
use crate::EditState;
use photoforge_history::Command;
use serde::{Deserialize, Serialize};

/// Full-state snapshot command: carries the state before and after the edit.
/// Edit states are small (a few KB of JSON), so whole-state commands are
/// simpler and just as cheap as per-field diffs; the history's memory budget
/// bounds the stack either way.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StateCommand {
    pub label: String,
    pub old: EditState,
    pub new: EditState,
}

impl StateCommand {
    pub fn new(label: impl Into<String>, old: EditState, new: EditState) -> Self {
        Self {
            label: label.into(),
            old,
            new,
        }
    }
}

impl Command<EditState> for StateCommand {
    fn apply(&self, state: &mut EditState) {
        *state = self.new.clone();
    }

    fn revert(&self, state: &mut EditState) {
        *state = self.old.clone();
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn bytes(&self) -> usize {
        // Rough estimate: adjustments + mask strokes + two JSON copies.
        let adj_bytes: usize = self
            .new
            .adjustments
            .iter()
            .map(|a| {
                128 + a
                    .mask
                    .as_ref()
                    .map(|m| match m {
                        Mask::Brush { strokes } => {
                            strokes.iter().map(|st| 64 + st.points.len() * 16).sum()
                        }
                        _ => 64,
                    })
                    .unwrap_or(0)
            })
            .sum();
        adj_bytes * 2 + 256
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use photoforge_ops::OpParams;

    #[test]
    fn state_command_roundtrip_via_json() {
        let mut old = EditState::with_global_stack();
        old.adjustments[1].op = OpParams::Exposure { stops: 0.0 };
        let mut new = old.clone();
        new.adjustments[1].op = OpParams::Exposure { stops: 1.5 };
        let cmd = StateCommand::new("exposure", old.clone(), new.clone());
        let json = serde_json::to_string(&cmd).expect("serialize");
        let back: StateCommand = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, cmd);
        let mut s = old.clone();
        back.apply(&mut s);
        assert_eq!(s, new);
        back.revert(&mut s);
        assert_eq!(s, old);
    }
}

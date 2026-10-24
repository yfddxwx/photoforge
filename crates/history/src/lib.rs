//! Generic command-pattern undo/redo with a memory budget and disk spill.
//!
//! The undo stack never grows without bound (user requirement): when the
//! estimated in-memory size exceeds the budget, the oldest commands are
//! serialized to JSON files and evicted; they are transparently reloaded
//! when the undo cursor walks back into them. A snapshot ring supports
//! jump-to-state and crash-recovery autosave (ADR-0009).

use std::collections::VecDeque;
use std::marker::PhantomData;
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::Serialize;

/// A reversible edit over state `S`.
pub trait Command<S>: Serialize + DeserializeOwned + Clone + Send + Sync {
    fn apply(&self, state: &mut S);
    fn revert(&self, state: &mut S);
    /// Human-readable label for UI histories.
    fn label(&self) -> &str;
    /// Rough memory footprint (drives the spill budget).
    fn bytes(&self) -> usize;
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// One contiguous run of commands, either resident or spilled to disk.
enum Segment<S, C: Command<S>> {
    Mem(VecDeque<C>, PhantomData<S>),
    Spilled { path: PathBuf, count: usize },
}

impl<S, C> Segment<S, C>
where
    S: Serialize + DeserializeOwned + Clone + Send + Sync,
    C: Command<S>,
{
    fn len(&self) -> usize {
        match self {
            Self::Mem(v, _) => v.len(),
            Self::Spilled { count, .. } => *count,
        }
    }
}

pub struct History<S, C: Command<S>>
where
    S: Serialize + DeserializeOwned + Clone + Send + Sync,
{
    segments: VecDeque<Segment<S, C>>,
    /// Absolute cursor: number of applied commands (0..=total).
    pos: usize,
    mem_used: usize,
    budget: usize,
    spill_dir: PathBuf,
    spill_counter: u64,
    snapshots: VecDeque<(usize, ArcSnapshot<S>)>,
    snapshot_every: usize,
    pushes_since_snapshot: usize,
}

struct ArcSnapshot<S>(S);

impl<S, C> History<S, C>
where
    S: Serialize + DeserializeOwned + Clone + Send + Sync,
    C: Command<S>,
{
    /// Creates a history with the given in-memory byte budget. Spilled
    /// chunks are written under `spill_dir`.
    pub fn new(budget: usize, spill_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&spill_dir).ok();
        Self {
            segments: VecDeque::new(),
            pos: 0,
            mem_used: 0,
            budget,
            spill_dir,
            spill_counter: 0,
            snapshots: VecDeque::new(),
            snapshot_every: 8,
            pushes_since_snapshot: 0,
        }
    }

    fn total(&self) -> usize {
        self.segments.iter().map(|s| s.len()).sum()
    }

    /// Current number of applied commands.
    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn can_undo(&self) -> bool {
        self.pos > 0
    }

    pub fn can_redo(&self) -> bool {
        self.pos < self.total()
    }

    /// Pushes a new command, dropping any redo tail. Returns spilled bytes.
    pub fn push(&mut self, cmd: C, state: &S) -> usize {
        // Truncate the redo tail: drop commands at/after `pos`.
        let mut drop_from = self.total();
        let mut removed_bytes = 0usize;
        while drop_from > self.pos {
            match self.segments.back_mut() {
                Some(Segment::Mem(v, _)) => {
                    let seg_len = v.len();
                    let seg_start = drop_from - seg_len;
                    if self.pos >= seg_start {
                        let to_drop = drop_from - self.pos.max(seg_start);
                        for _ in 0..to_drop {
                            if let Some(c) = v.pop_back() {
                                removed_bytes += c.bytes();
                            }
                        }
                        drop_from -= to_drop;
                    } else {
                        removed_bytes += v.iter().map(|c| c.bytes()).sum::<usize>();
                        drop_from -= seg_len;
                        self.segments.pop_back();
                    }
                }
                Some(seg @ Segment::Spilled { .. }) => {
                    let seg_len = seg.len();
                    drop_from -= seg_len;
                    // Spilled redo tail is simply deleted.
                    if let Segment::Spilled { path, .. } = self.segments.pop_back().expect("back") {
                        std::fs::remove_file(path).ok();
                    }
                }
                None => break,
            }
        }
        let _ = removed_bytes;
        self.mem_used += cmd.bytes();
        match self.segments.back_mut() {
            Some(Segment::Mem(v, _)) => v.push_back(cmd),
            _ => self
                .segments
                .push_back(Segment::Mem(VecDeque::from([cmd]), PhantomData)),
        }
        self.pos += 1;
        self.maybe_spill();
        self.maybe_snapshot(state);
        self.spilled_bytes()
    }

    /// Undoes the last applied command onto `state`. Returns its label.
    pub fn undo(&mut self, state: &mut S) -> Option<String> {
        if self.pos == 0 {
            return None;
        }
        let cmd = self.entry_at(self.pos - 1)?;
        cmd.revert(state);
        self.pos -= 1;
        Some(cmd.label().to_string())
    }

    /// Redoes the next command onto `state`.
    pub fn redo(&mut self, state: &mut S) -> Option<String> {
        if self.pos >= self.total() {
            return None;
        }
        let cmd = self.entry_at(self.pos)?;
        cmd.apply(state);
        self.pos += 1;
        Some(cmd.label().to_string())
    }

    /// The command at absolute index `i`, loading spilled segments on demand.
    fn entry_at(&mut self, i: usize) -> Option<C> {
        let mut base = 0usize;
        let mut seg_idx = 0usize;
        let n_segments = self.segments.len();
        while seg_idx < n_segments {
            let len = self.segments[seg_idx].len();
            if i < base + len {
                match &mut self.segments[seg_idx] {
                    Segment::Mem(v, _) => return v.get(i - base).cloned(),
                    Segment::Spilled { path, count } => {
                        // Load and replace with a resident segment.
                        let text = std::fs::read_to_string(path).ok()?;
                        let cmds: Vec<C> = serde_json::from_str(&text).ok()?;
                        log::debug!("reloaded {} spilled commands", cmds.len());
                        let vd: VecDeque<C> = cmds.into();
                        let out = vd.get(i - base).cloned();
                        self.mem_used += vd.iter().map(|c| c.bytes()).sum::<usize>();
                        let _ = count;
                        self.segments[seg_idx] = Segment::Mem(vd, PhantomData);
                        return out;
                    }
                }
            }
            base += len;
            seg_idx += 1;
        }
        None
    }

    /// Spills the oldest resident commands while over budget. Returns bytes
    /// written to disk.
    fn maybe_spill(&mut self) -> usize {
        let mut written = 0usize;
        while self.mem_used > self.budget {
            // Find the first resident segment.
            let Some(idx) = self
                .segments
                .iter()
                .position(|s| matches!(s, Segment::Mem(_, _)))
            else {
                break;
            };
            let Segment::Mem(v, _) = &mut self.segments[idx] else {
                unreachable!()
            };
            // Spill at least half of it (but never the newest command).
            let keep = (v.len() / 2).max(1);
            if v.len() <= keep {
                break; // single newest command over budget: accept
            }
            let drain = v.len() - keep;
            let mut cmds = Vec::with_capacity(drain);
            for _ in 0..drain {
                if let Some(c) = v.pop_front() {
                    self.mem_used -= c.bytes();
                    cmds.push(c);
                }
            }
            if v.is_empty() {
                self.segments.remove(idx);
            }
            self.spill_counter += 1;
            let path = self
                .spill_dir
                .join(format!("undo_{}.json", self.spill_counter));
            match serde_json::to_string(&cmds) {
                Ok(text) => match std::fs::write(&path, &text) {
                    Ok(()) => {
                        written += text.len();
                        let count = cmds.len();
                        self.segments.insert(idx, Segment::Spilled { path, count });
                    }
                    Err(e) => {
                        log::warn!("spill write failed: {e}");
                        // Put the commands back at the front of the segment.
                        if let Some(Segment::Mem(v, _)) = self.segments.get_mut(idx) {
                            for c in cmds {
                                self.mem_used += c.bytes();
                                v.push_front(c);
                            }
                        }
                        break;
                    }
                },
                Err(e) => {
                    log::warn!("spill serialize failed: {e}");
                    for c in cmds {
                        self.mem_used += c.bytes();
                        if let Some(Segment::Mem(v, _)) = self.segments.get_mut(idx) {
                            v.push_front(c);
                        }
                    }
                    break;
                }
            }
        }
        written
    }

    /// Bytes currently spilled to disk (rough: file sizes are not tracked
    /// individually; this counts spill events via file listing).
    fn spilled_bytes(&self) -> usize {
        std::fs::read_dir(&self.spill_dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter_map(|e| e.metadata().ok())
                    .map(|m| m.len() as usize)
                    .sum()
            })
            .unwrap_or(0)
    }

    fn maybe_snapshot(&mut self, state: &S) {
        self.pushes_since_snapshot += 1;
        if self.pushes_since_snapshot >= self.snapshot_every {
            self.pushes_since_snapshot = 0;
            self.snapshots
                .push_back((self.pos, ArcSnapshot(state.clone())));
            while self.snapshots.len() > 32 {
                self.snapshots.pop_front();
            }
        }
    }

    /// The most recent snapshot (position, state) for crash recovery.
    pub fn latest_snapshot(&self) -> Option<(usize, &S)> {
        self.snapshots.back().map(|(p, s)| (*p, &s.0))
    }

    /// Number of resident (non-spilled) commands, for tests/debug.
    pub fn resident_len(&self) -> usize {
        self.segments
            .iter()
            .map(|s| match s {
                Segment::Mem(v, _) => v.len(),
                Segment::Spilled { .. } => 0,
            })
            .sum()
    }

    /// Estimated in-memory bytes.
    pub fn mem_used(&self) -> usize {
        self.mem_used
    }

    /// Removes all spill files (call on session close).
    pub fn cleanup(&self) {
        std::fs::remove_dir_all(&self.spill_dir).ok();
    }
}

impl<S, C> Drop for History<S, C>
where
    S: Serialize + DeserializeOwned + Clone + Send + Sync,
    C: Command<S>,
{
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Doc {
        text: String,
        rev: u64,
    }

    /// Appends `n` 'x' chars on apply, removes them on revert.
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct Append {
        n: usize,
        name: String,
    }

    impl Command<Doc> for Append {
        fn apply(&self, state: &mut Doc) {
            state.text.push_str(&"x".repeat(self.n));
            state.rev += 1;
        }
        fn revert(&self, state: &mut Doc) {
            let l = state.text.len();
            state.text.truncate(l - self.n);
            state.rev -= 1;
        }
        fn label(&self) -> &str {
            &self.name
        }
        fn bytes(&self) -> usize {
            64 + self.n
        }
    }

    fn h(tmp: &std::path::Path, budget: usize) -> History<Doc, Append> {
        History::new(budget, tmp.join(format!("spill_{}", std::process::id())))
    }

    #[test]
    fn undo_redo_roundtrip() {
        let dir = std::env::temp_dir();
        let mut hist = h(&dir, 1 << 20);
        let mut doc = Doc {
            text: String::new(),
            rev: 0,
        };
        hist.push(
            Append {
                n: 3,
                name: "a".into(),
            },
            &doc,
        );
        Append {
            n: 3,
            name: "a".into(),
        }
        .apply(&mut doc);
        hist.push(
            Append {
                n: 5,
                name: "b".into(),
            },
            &doc,
        );
        Append {
            n: 5,
            name: "b".into(),
        }
        .apply(&mut doc);
        assert_eq!(doc.text, "xxxxxxxx");
        assert_eq!(hist.undo(&mut doc), Some("b".into()));
        assert_eq!(doc.text, "xxx");
        assert_eq!(hist.undo(&mut doc), Some("a".into()));
        assert_eq!(doc.text, "");
        assert!(hist.undo(&mut doc).is_none());
        assert_eq!(hist.redo(&mut doc), Some("a".into()));
        assert_eq!(hist.redo(&mut doc), Some("b".into()));
        assert_eq!(doc.text, "xxxxxxxx");
        assert!(hist.redo(&mut doc).is_none());
    }

    #[test]
    fn push_drops_redo_tail() {
        let dir = std::env::temp_dir();
        let mut hist = h(&dir, 1 << 20);
        let mut doc = Doc {
            text: String::new(),
            rev: 0,
        };
        for n in [1usize, 2, 3] {
            let cmd = Append {
                n,
                name: format!("c{n}"),
            };
            cmd.apply(&mut doc);
            hist.push(cmd, &doc);
        }
        hist.undo(&mut doc);
        hist.undo(&mut doc);
        assert_eq!(doc.text, "x");
        // New branch: replaces the redo tail.
        let cmd = Append {
            n: 10,
            name: "d".into(),
        };
        cmd.apply(&mut doc);
        hist.push(cmd, &doc);
        assert!(!hist.can_redo());
        assert_eq!(doc.text, "xxxxxxxxxxx");
        assert_eq!(hist.undo(&mut doc), Some("d".into()));
        assert_eq!(hist.undo(&mut doc), Some("c1".into()));
        assert_eq!(doc.text, "");
    }

    #[test]
    fn spills_and_reloads_transparently() {
        let dir = std::env::temp_dir();
        // Budget fits ~3 commands of 64+512 bytes.
        let mut hist = h(&dir, 4 * 576);
        let mut doc = Doc {
            text: String::new(),
            rev: 0,
        };
        for i in 0..12 {
            let cmd = Append {
                n: 512,
                name: format!("c{i}"),
            };
            cmd.apply(&mut doc);
            hist.push(cmd, &doc);
        }
        // Everything must still undo correctly, across reloads.
        for _ in 0..12 {
            assert!(hist.undo(&mut doc).is_some());
        }
        assert_eq!(doc.text, "");
        assert_eq!(doc.rev, 0);
        for _ in 0..12 {
            assert!(hist.redo(&mut doc).is_some());
        }
        assert_eq!(doc.text.len(), 12 * 512);
    }

    #[test]
    fn snapshot_ring_tracks_positions() {
        let dir = std::env::temp_dir();
        let mut hist = h(&dir, 1 << 20);
        let mut doc = Doc {
            text: String::new(),
            rev: 0,
        };
        for i in 0..20 {
            let cmd = Append {
                n: 1,
                name: format!("s{i}"),
            };
            cmd.apply(&mut doc);
            hist.push(cmd, &doc);
        }
        let (pos, snap) = hist.latest_snapshot().expect("snapshot exists");
        assert_eq!(pos, 16); // 20 = 2 snapshots: at 8 and 16
        assert_eq!(snap.text.len(), 16);
    }

    #[test]
    fn budget_holds_largest_command() {
        let dir = std::env::temp_dir();
        let mut hist = h(&dir, 100);
        let mut doc = Doc {
            text: String::new(),
            rev: 0,
        };
        let huge = Append {
            n: 100_000,
            name: "huge".into(),
        };
        huge.apply(&mut doc);
        hist.push(huge, &doc);
        // Single over-budget command is kept resident (accepted).
        assert_eq!(hist.resident_len(), 1);
        assert!(hist.undo(&mut doc).is_some());
    }
}

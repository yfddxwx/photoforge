//! photoforge-history
//!
//! Generic command-pattern undo/redo: the undo stack never grows without
//! bound — it has a memory budget and spills old entries to disk as JSON,
//! reloading them on demand. Also provides a snapshot ring for jump-to-state
//! and crash-recovery autosave.

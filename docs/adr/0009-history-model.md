# 0009 — History model: whole-state commands with a memory budget

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Every user action must be undoable/redoable; the undo stack must not grow
without bound (memory cap + disk spill, user requirement); crash recovery
needs periodic state snapshots.

## Decision

1. **Generic core.** `history::History<S, C>` is generic over the state `S`
   and command `C: Command<S>` (`apply`/`revert`/`label`/`bytes`, serde-
   serializable for spill). The crate knows nothing about images.
2. **Whole-state commands.** The pipeline provides
   `StateCommand { old, new: EditState }` — a snapshot pair. An `EditState`
   is a few KB (adjustment parameters, mask geometry), so whole-state
   commands are as cheap as field diffs and cover every edit uniformly:
   parameter changes, mask strokes, enable toggles, local add/remove,
   geometry and crops. No per-field command plumbing to maintain.
3. **Memory budget + spill.** `bytes()` estimates each command's footprint;
   past the budget (64 MiB in the GUI), the oldest resident commands are
   serialized to JSON files under a spill directory and reloaded
   transparently when the undo cursor walks back into them. A single
   command larger than the budget is kept resident (correctness first).
4. **Snapshot ring.** Every 8 pushes a `(position, state)` snapshot is kept
   (32 entries) for crash recovery; the GUI autosaves the latest one plus
   the open image path to the session file.
5. **Redo semantics.** Pushing a command truncates the redo tail (spilled
   tail chunks are deleted).

## Consequences

- Undo/redo of any edit is one `Arc` swap away; the renderer's prefix-hash
  cache (ADR-0006) revalidates automatically after undo/redo.
- Spill correctness is property-tested across reload boundaries
  (12-command spill cycle: full undo + redo round-trip).
- History is per image; switching images in the filmstrip resets it.

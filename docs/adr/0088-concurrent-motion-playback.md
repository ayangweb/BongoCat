# ADR-0088: Concurrent motion playback

- Status: accepted
- Date: 2026-10-08
- Depends on: ADR-0061, ADR-0012 (ordered motion audio service)
- Issue: https://github.com/ayangweb/BongoCat/issues/1107

## Decision

Add `model.allow_motion_overlap`, a boolean defaulting to false and using
`#[serde(default)]` for existing v1 documents. The Model behavior page exposes
the setting on both supported platforms through a typed, revision-checked command.

When enabled, distinct motion identities coexist. Every layer owns its clip,
start time, natural completion, explicit fade-out and UserData cursor. Evaluate
layers in accepted start order, oldest first. Restarting a matching identity moves
it to the newest position; idempotent repeats and stops do not reorder layers.
Use insertion order rather than timestamps so starts at the same clock sample
remain ordered. model3 has groups rather than a dedicated stacking priority, and
the standard preset's groups reference the same files. Sorting by group name
would let a completed later-sorted layer hide every replay from an earlier group.
Later layers blend over earlier layers using the existing Cubism curve weights.
Distinct parameters remain independent. This does not make conflicting writes
additive or introduce a new model format.

A repeated product trigger for the same unfinished motion at the same priority
remains a no-op. A preview restarts only its matching layer. A completed one-shot
keeps its naturally faded terminal pose, as required by ADR-0061, and can replay.
Stops match one identity and remove only that layer after its own fade. Priority
arbitration applies to that identity in overlap mode, and to the current motion
in replacement mode. Automatic idle motions still yield to any unfinished manual
motion. Turning overlap off keeps the most recently started layer immediately;
successful model commits and shutdown clear every layer. Failed model preparation
preserves all current layers.

The runtime publishes all active motion identities. Its existing `active_motion`
view is derived from the most recently started surviving identity for consumers
that only display one motion. Renderer UserData carries its originating identity.
Audio retains the single ordered worker: each accepted start uses the existing
replace-audio rule, and an accepted stop uses the existing stop-audio rule.

## Validation And Remaining Work

Phase 3 playback and Phase 4 settings scope: verify old-document loading,
typed settings persistence, independent evaluation/fades/stops, deterministic
precedence in both start orders, standard-preset cross-group replays over held
terminal poses, repeated-trigger audio suppression, model cleanup and opt-out behavior.
Run workspace quality gates and macOS product smoke. Windows product smoke and
real-model visual comparison remain platform validation tasks until recorded;
this change does not close the broader Cubism compatibility or release gates.

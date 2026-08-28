# Animation and Model Command Semantics

状态：Phase 0 draft
版本：1

## Commands represented by fixtures

- `model_switch` changes the selected model and clears the active motion and expression only after the switch command is accepted.
- `motion_start` carries an explicit `idle`, `normal`, or `force` priority. A lower-priority request cannot replace an active higher-priority motion; equal priority uses the latest request.
- `motion_stop` only stops the named active motion. Stopping an old motion must not cancel a newer motion.
- `expression_set` replaces the current expression for the selected model.
- `audio_trigger` is an ordered side effect. It is not part of the render snapshot and must never block input edges.

The command IDs in these fixtures are product protocol values, not Cubism group/index identifiers. A model adapter resolves them to validated resources before runtime commit; failed resolution leaves the previous model and animation state usable.

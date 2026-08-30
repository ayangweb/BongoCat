# Animation and Model Command Semantics

状态：Phase 0 draft
版本：1

## Commands represented by fixtures

- `model_switch` changes the selected model and clears the active motion and expression only after the switch command is accepted.
- `motion_start` carries an explicit `idle`, `normal`, or `force` priority. A lower-priority request cannot replace an active higher-priority motion; equal priority uses the latest request.
- `motion_stop` only stops the named active motion. Stopping an old motion must not cancel a newer
  motion. A non-zero model3 `FadeOutTime` keeps the motion active while a sine-eased outer weight
  reaches zero; runtime snapshots retain the first stop command sequence until completion. Repeated
  stops are idempotent and cannot restart the fade. A zero-duration fade clears the motion
  immediately.
- `expression_set` resolves a non-blank expression name against the selected model before changing
  runtime state. A failed resolution leaves the current expression active.
- Setting an expression fades it in with sine easing. A later expression keeps only the immediately
  previous visible layer for sine fade-out, so at most two layers coexist during a bounded
  transition; the newest expression is the sole active product identity.
- Expression parameters support `Add`, `Multiply`, and `Overwrite`. Layers are folded oldest to
  newest from the post-motion parameter value. Product input is applied after expression layers so
  a physically pressed key or button remains authoritative for mapped controls.
- A successful model commit clears motion and expression state. CPU/GPU preparation failure keeps
  the previous model, motion, expression, and input bindings usable.
- `audio_trigger` is an ordered side effect. It is not part of the render snapshot and must never block input edges.

The command IDs in these fixtures are product protocol values, not Cubism group/index identifiers. A model adapter resolves them to validated resources before runtime commit; failed resolution leaves the previous model and animation state usable.

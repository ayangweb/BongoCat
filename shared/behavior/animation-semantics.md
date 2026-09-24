# Animation and Model Command Semantics

状态：Phase 0 draft
版本：1

## Commands represented by fixtures

- `model_switch` changes the selected model and clears the active motion and expression only after the switch command is accepted.
- `motion_start` carries an explicit `idle`, `normal`, or `force` priority. While a motion is running, a lower-priority request cannot replace a higher-priority motion; a request for a different motion at equal priority uses the latest request. A repeat request for the motion that is already running at the same priority is ignored while that run is unfinished, following the R5 motion queue, so key repeat and press bursts neither restart the clip nor replay its motion audio. Once the run completes, it no longer reserves priority and the next request may replace or restart it.
- A triggered motion plays exactly one cycle even when the clip declares `Meta.Loop = true`: the loop flag describes how the asset was authored, not how the product drives it. At the clip's declared duration, the motion becomes completed and its final evaluated parameter, part-opacity, and model-opacity samples remain the current motion layer on later frames instead of clearing to idle or continuing to advance. A replacement motion, a completed explicit stop, a successful model switch, or shutdown removes it. A UI preview follows the same one-cycle/final-pose rule but restarts on every request.
- A model behaviour shortcut triggers its motion once per physical key press. Operating systems repeat the pressed event while a chord stays held, and every repeat is dropped rather than dispatched, so holding a shortcut never retriggers its action.
- `motion_stop` only stops the named current motion, including a completed motion holding its final
  pose. Stopping an old motion must not cancel a newer
  motion. A non-zero model3 `FadeOutTime` keeps the motion active while a sine-eased outer weight
  reaches zero; runtime snapshots retain the first stop command sequence until completion. Repeated
  stops are idempotent and cannot restart the fade. A zero-duration fade clears the motion
  immediately.
- A motion `PartOpacity` curve follows the R5 Framework sink: its ID resolves against Core
  parameters and its evaluated value is written without the ordinary parameter-curve fade weight.
  Missing IDs are skipped without invalidating the remaining motion.
- Model3 `Groups` retain their declared order. `EyeBlink` and `LipSync` use the first matching
  `Parameter` group and at most its first 64 IDs, matching the R5 Framework target bound.
- A motion `Model/EyeBlink` value multiplies a matching Parameter curve before that curve's fade;
  `Model/LipSync` adds to a matching Parameter curve. Group parameters without matching motion
  Parameter curves interpolate toward the model value with the motion fade. Missing Core parameter
  IDs have no visible effect and do not invalidate the motion.
- A motion `Model/Opacity` value is clamped to the renderer's `[0, 1]` alpha contract and persists
  until a later opacity curve updates it or a model switch creates a new model. It multiplies final
  drawable alpha after mask generation, independently of the configured overlay opacity.
- `expression_set` resolves a non-blank expression name against the selected model before changing
  runtime state. A failed resolution leaves the current expression active.
- Setting an expression fades it in with sine easing. A later expression keeps only the immediately
  previous visible layer for sine fade-out, so at most two layers coexist during a bounded
  transition; the newest expression is the sole active product identity and remains applied at full
  weight after fade-in until another valid expression replaces it, a model commit succeeds, or
  shutdown occurs. Expressions have no duration or automatic clear-to-idle transition.
- Expression parameters support `Add`, `Multiply`, and `Overwrite`. Layers are folded oldest to
  newest from the post-motion parameter value. Product input is applied after expression layers so
  a physically pressed key or button remains authoritative for mapped controls.
- Automatic model effects are evaluated after motion and expression layers and before product input.
  `ParamBreath` receives a deterministic four-second sine cycle when declared by the model. The
  first `EyeBlink` group parameters (`ParamEyeLOpen` and `ParamEyeROpen` when present) remain open
  except for a deterministic 180ms closed window at the start of each five-second cycle. Missing
  parameters are ignored, and product input remains authoritative for mapped controls.
- A successful model commit clears motion and expression state. CPU/GPU preparation failure keeps
  the previous model, motion, expression, and input bindings usable.
- `audio_trigger` is an ordered side effect. It is not part of the render snapshot and must never
  block input edges. Only an accepted motion triggers audio. A replacement motion stops the current
  voice before starting its validated relative sound; a replacement without sound also stops the
  old voice. Explicit motion stop, disabling audio, successful model commit, and shutdown stop the
  voice immediately. A rejected motion leaves it unchanged. File, decode, device, and queue errors
  are observable diagnostics but never fail animation or rendering.
- Motion UserData is emitted once for every timestamp crossed in `(previous_elapsed, elapsed]`, with
  time-zero events included on the first evaluation. Loop boundaries preserve chronological order,
  clock rollback emits nothing, and a bounded batch reports skipped occurrences rather than making
  an unbounded allocation after a long suspension.

The command IDs in these fixtures are product protocol values, not Cubism group/index identifiers. A model adapter resolves them to validated resources before runtime commit; failed resolution leaves the previous model and animation state usable.

# ADR-0061: Live2D playback boundary

- Status: accepted
- Date: 2026-09-24
- Depends on: ADR-0060, ADR-0041, ADR-0050, ADR-0055

## Context

`bongocat-live2d` combined Cubism Core ownership with motion3/exp3 parsing and
pure curve evaluation. The Core wrapper is tied to raw bindings, model
resources, parameter IDs, part-opacity sinks, and GPU-facing snapshots, while
the motion and expression parsers only need bytes, monotonic elapsed time, and
project-owned numeric results. Keeping both responsibilities in one crate made
pure playback tests compile the vendor SDK and obscured which layer owns
resource loading versus Core writes.

## Decision

Create `bongocat-live2d-playback` for the pure clip layer. It owns:

- motion3 metadata/curve/segment parsing, linear/Bezier/stepped/inverse-stepped
  evaluation, fade/loop time, PartOpacity samples, and UserData crossings;
- exp3 parsing, fade weights, blend modes, and pure expression-layer mixing;
- the playback-specific `PlaybackError`/`PlaybackErrorCode` for malformed
  motion or expression bytes.

The crate depends only on `serde`, `serde_json`, and the standard library. It
does not depend on `bongocat-model`, `bongocat-render`, `bongocat-live2d`,
runtime, GPUI, platform APIs, or the filesystem. `MotionClip::load` and
`ExpressionClip::load` are not part of this crate; `bongocat-live2d` reads the
committed model resources through the render-resource adapter and calls
`from_slice`.

`bongocat-live2d` remains responsible for:

- `CommittedModel` resource lookup and bounded file reads;
- Cubism Core loading, parameter ranges/defaults, Core writes, part-opacity
  sinks, dynamic flags, RenderResources, and model opacity;
- adapting `PlaybackError` into the existing stable `Live2dErrorCode` catalog;
- `MotionApplyStatus` and `ExpressionApplyStatus`, whose counts describe actual
  writes accepted by Core.

`bongocat-runtime` remains the owner of active playback state, priority,
single-loop product policy, stop/crossfade timestamps, command sequencing,
model-generation cleanup, and audio side effects. It may depend directly on the
playback value types, but playback never sees runtime commands or Core state.

The production dependency direction is:

```text
bongocat-live2d-playback -> serde / serde_json / std
bongocat-live2d ----------> bongocat-live2d-playback
bongocat-runtime --------> bongocat-live2d + bongocat-live2d-playback
```

## Consequences

- motion/expression parser and numeric tests no longer compile the vendor SDK;
- Core-coupled loading and application tests remain in `bongocat-live2d`;
- runtime's existing frame order and playback ownership do not change;
- no physics/pose crate or placeholder is introduced by this slice; those
  evaluators remain a future extension of the same pure boundary;
- the old `bongocat-live2d` playback re-export is removed after runtime imports
  the playback types directly.

## Amendment: completed motions hold their terminal pose

The original one-shot policy removed renderer playback and the public active-motion identity as
soon as a clip reached its declared duration. The following evaluation restored Core defaults, so
ordinary motion parameters disappeared one frame after the terminal sample. This solved accidental
looping but did not match the 2026-09-24 product requirement to stop on the action's final state.

A one-shot motion now transitions to an internal completed state at the clip duration. Its local
sample time is pinned to that duration, and the fully evaluated parameter, part-opacity, and
model-opacity samples remain a renderer-owned motion layer that is reapplied after each per-frame
default reset. "Terminal sample" includes the resource's natural model3/curve fade weights; completion
does not strip a natural fade-out to expose an earlier raw curve endpoint. Core part opacities are
restored to their fresh-model values before the active motion is applied, so removing or replacing a
motion cannot leave stale part visibility. The later reference-compatible amendment below updates
the automatic/product/physics sub-order; the completed-motion lifecycle itself is unchanged.

Completed playback is not advanced again and does not reserve motion priority. The worker derives
completion from the injected monotonic clock as well as the last delivered frame, so a hidden or
sleeping overlay cannot swallow a replay or lower-priority request sent after the duration. The public
active identity remains current so an explicit `StopMotion` can still fade and remove it; a
replacement, successful model commit, or shutdown removes the layer as before. `StopMotion` continues
to target the current identity: a different old ID cannot stop a newer motion, while a replayed run
with the same ID is intentionally the current target of a later same-ID stop.

Motion UserData crossing evaluation uses the runtime's effective playback mode. Playing a
`Meta.Loop = true` asset once therefore does not emit its time-zero event again at the duration
boundary. Expressions retain the already-correct stateful lifecycle: the newest expression remains
pinned at full weight after fade-in, including across a test-clock rollback, until another valid
expression replaces it, a model commit succeeds, or shutdown clears it. No expression clear,
duration, new crate, dependency, FFI call, or frozen whole-model snapshot is introduced.

### Superseded amendment: automatic breath is an explicit weighted model-range group

> This intermediate rule is retained as history only; the reference-compatible
> amendment below supersedes it for the current runtime.

A parameter merely named `ParamBreath` is not sufficient intent: third-party models can use that
conventional ID for authored pose selection. A signed sine written through the generic `[-1, 1]`
normalized-parameter helper also turns a common one-sided `0..1` range into a long clamp at the
minimum followed by a full-range sweep. A model-authored drawable threshold can then switch large
groups of parts every cycle even when the package has no motion or expression.

Automatic breathing is therefore opt-in through the first model3 `Parameter` group named `Breath`,
with the same first-64-ID bound as EyeBlink and LipSync. A model without that group keeps its
authored parameter defaults. For declared IDs, the runtime emits a deterministic four-second
unit-interval sine phase; the Core-coupled adapter maps it across each parameter's declared range
and applies it with `0.5` weight after motion/expression. Eye blink keeps its existing normalized
open/closed input. This is a parameter-evaluation correction, not a motion or expression trigger,
and it does not add renderer-side state.

### Superseding amendment: restore the Mver reference breath and physics path

The explicit-group-only rule above was sufficient to stop the imported model's full-pose threshold
crossing, but it also removed the model's authored idle hair movement. The fixed Mver source shows
that this is not an optional model-specific effect: `myUserModel.cpp` always creates a
`CubismBreath` instance with the conventional `ParamAngleX`, `ParamAngleY`, `ParamAngleZ`,
`ParamBodyAngleX`, and `ParamBreath` IDs, then loads and evaluates the declared `physics3` resource.
The update order is motion/expression, product drag, breath, physics, and Core update.

Native therefore applies the same five fixed breath targets with their reference offsets, peaks,
cycles, and `0.5` contribution weight after typed product input. A model3 `Breath` group remains an
optional source of up to 64 additional IDs; it is no longer required for the conventional targets,
and fixed IDs are not applied twice. For the reported model, the `ParamBreath` range is `0..1` with
default `0`, so the reference contribution stays at or below `0.5` and does not cross its authored
visibility threshold, while the fixed angle targets feed the declared physics rig and restore the
hair's idle motion.

Validated physics3 v3 definitions are parsed by the model contract and evaluated by a bounded Rust
runtime with fixed-step interpolation, inertia, delay, and typed parameter IDs. Unknown input or
output IDs are skipped rather than reaching Core. This is a compatibility path for the declared
physics resource, not a claim that every Cubism feature or every platform has completed the full
R5 black-box gate; pose evaluation remains separate and unfinished.

## Rejected alternatives

- Moving Cubism Core, model loading, or GPU resource preparation into the new
  crate: rejected because that would make pure playback depend on FFI and
  rendering.
- Moving runtime `MotionPlayback`/`ExpressionPlayback` state into playback:
  rejected because those values encode product command, priority, generation,
  and audio/shutdown semantics.
- Keeping file loaders in playback with a `CommittedModel` dependency:
  rejected because resource I/O and model identity are not part of numeric clip
  evaluation.
- Creating empty physics/pose modules: rejected until authorized fixtures and
  the R5 compatibility gate define their contracts. A bounded physics evaluator
  is now allowed only for declared, validated v3 resources; pose remains gated.

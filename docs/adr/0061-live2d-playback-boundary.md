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
  the R5 compatibility gate define their contracts.

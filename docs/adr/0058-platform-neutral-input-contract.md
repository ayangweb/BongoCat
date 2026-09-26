# ADR-0058: Platform-neutral input contract boundary

- Status: accepted
- Date: 2026-09-24
- Depends on: ADR-0004, ADR-0007, ADR-0044

## Context

`bongocat-platform` originally imported the public input types from
`bongocat-runtime`. That made a platform adapter depend on the complete runtime
graph, including Cubism, audio, model state, and worker lifecycle. The same
runtime crate also contained the pure input state reducer and the platform
input adapters, so the dependency direction did not express the actual
ownership boundary.

## Decision

Create `bongocat-input` as a platform-neutral contract and transport crate.

It owns:

- physical key, mouse, and gamepad identities;
- `InputEvent`, sequencing, reset, and reconciliation vocabulary;
- reliable input producer hand-off and anonymous transport diagnostics;
- cursor and gamepad-axis latest-value transports;
- platform-input diagnostic values and stable error-code validation;
- the canonical `GLOBE_KEY_USAGE` constant.

`bongocat-runtime` continues to own the single mutable `InputState`, model
bindings and `ModelInputSnapshot` projection, command queue, worker, and
shutdown lifecycle. The input crate does not depend on runtime, renderer,
GPUI, configuration, or an operating-system API.

The runtime adapts its command producer through the input crate's
`InputSubmitter` trait. Platform input services use the input crate directly;
they do not depend on the normal `bongocat-runtime` library. Global shortcut
registration likewise accepts an application-owned typed callback, so the
platform crate does not construct runtime commands.

`bongocat-render` re-exports `GLOBE_KEY_USAGE` for existing model/render
vocabulary, but does not own the physical input contract.

## Consequences

- platform input code no longer pulls Cubism, model, audio, or runtime worker
  dependencies;
- input transport and reducer tests are isolated from the runtime worker;
- the runtime remains the only owner of mutable pressed state and shutdown
  ordering;
- existing runtime re-exports may remain temporarily for source compatibility,
  but new consumers should depend on `bongocat-input` directly;
- moving a reducer or model projection out of runtime requires a separate
  decision and is not implied by this crate.

## Rejected alternatives

- Moving `InputState` and `ModelInputSnapshot` into `bongocat-input`: rejected
  because it would make the public input crate depend on renderer vocabulary
  and blur the single runtime owner.
- Keeping platform input types in runtime: rejected because it preserves the
  platform-to-runtime dependency and makes unsafe input adapters compile with
  unrelated Cubism/audio code.
- Creating a separate crate for every transport or reducer submodule: rejected
  as unnecessary fragmentation; these types share one stable input contract.

# ADR-0060: Model package and model-store boundary

- Status: accepted
- Date: 2026-09-24
- Depends on: ADR-0036, ADR-0037, ADR-0038, ADR-0041, ADR-0047, ADR-0050, ADR-0055

## Context

`bongocat-model` combined two different responsibilities: read-only model3
package parsing and the mutable lifecycle that copies external sources into an
environment-owned store. The latter also owned UUID allocation, writer locks,
staging and atomic commit, legacy Mver conversion, key-image normalization, and
user-side preset cover replacement. As a result, parser-only consumers compiled
against image composition, UUID, and storage-write dependencies, while the
application's storage errors and import progress were mixed with package
validation types.

## Decision

Create `bongocat-model-store` as the filesystem-facing persistence and import
layer. It owns:

- `ModelStore`, its lock, recovery, staging, copy/validate/commit/delete flow,
  and installed catalog scan;
- `ModelStoreDiagnostic`, `ModelStoreError`, import stage/progress, and
  user-side preset cover storage;
- Mver detection/conversion and PNG composition;
- normalization of legacy key-image names inside the store's own staging copy.

`bongocat-model` remains the read-only model domain layer. It continues to own
model3/sidecar parsing, package limits and path safety, `ModelId`, immutable
model metadata, `PreparedModel`, `CommittedModel`, `ModelSnapshot`, and the
read-only `PresetModelCatalog`/`ModelCatalogEntry` projection. The preset
catalog stays here so overlay, Live2D, and runtime consumers do not acquire the
mutable store's image/UUID dependencies just to inspect bundled models.

The production dependency direction is:

```text
app ───────────────> model-store ───────> model
runtime/live2d/overlay ──────────────────> model
```

`bongocat-model-store` receives roots and lock paths from the application; it
does not read `StorageLayout`, select Development/Production, or depend on
config, platform, runtime, renderer, Live2D, or GPUI.

### Commit ownership

Rust cannot express “only this sibling crate may call this constructor” without
adding another lower-level crate. The split therefore uses a narrow, documented
product seam instead of claiming compile-time sealing: `PreparedModel::relocate`
rebinds an already validated package after the store's atomic rename, and
`InstalledModel::from_prepared` is called by the store transaction before the
existing `CommittedModel` conversion. The model-store/app wiring is the only
supported caller. A future requirement for hostile third-party callers would
need a separate sealed commit type; this ADR does not pretend the current Rust
visibility provides that stronger guarantee.

## Consequences

- parser-only crates no longer compile the Mver image/PNG/UUID/storage-write
  dependency set;
- model-store tests cover permissions, locks, staging, cancellation, recovery,
  Mver conversion, cover writes, and every registered shared fixture;
- source directories remain read-only inputs; only store staging and committed
  destinations are written;
- `CommittedModel` construction is a documented store/preset product invariant,
  not a compiler-enforced sealed type in this slice;
- Cubism Core lifecycle and Core-coupled parameter application remain in `bongocat-live2d`; pure motion/expression playback is owned by `bongocat-live2d-playback`.

## Rejected alternatives

- Moving `CommittedModel` and all model consumers into the store crate:
  rejected because it would make every renderer/runtime crate depend on the
  mutable import layer merely to hold a parsed model.
- Moving the read-only preset catalog into the store crate: rejected for this
  slice because it would unnecessarily broaden the dependency graph of
  overlay, Live2D, and runtime.
- Keeping the old modules in `bongocat-model` behind re-exports: rejected
  because it would preserve the dependency direction this boundary is intended
  to remove.
- Adding a new lower-level commit crate just to regain compile-time sealing:
  rejected as disproportionate for the current in-process, application-owned
  store; revisit only if an untrusted plugin boundary appears.

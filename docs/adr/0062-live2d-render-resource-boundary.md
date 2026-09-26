# ADR-0062: Live2D render-resource preparation boundary

- Status: accepted
- Date: 2026-09-24
- Depends on: ADR-0042, ADR-0060, ADR-0061, ADR-0003

## Context

`bongocat-live2d` combined the Cubism Core owner with model-package render
resource work. The Core owner needs a committed model, a Moc, Core parameter
IDs, and drawable snapshots. Key-image inventory, background discovery, PNG
dimension checks, and key-overlay resolution do not need Core or a GPU; they
only need the committed package and the immutable `bongocat-render` resource
types.

Keeping those CPU resource concerns in `bongocat-live2d` made the app and
runtime depend on the Core crate for key-image contracts, and made it harder
to test the package-to-render boundary without compiling the vendor SDK.

## Decision

Create `bongocat-live2d-render` for the platform-neutral model-to-render
resource preparation layer. It owns:

- construction of `RenderResources` from a `CommittedModel` index;
- background and key-image discovery, regular-file filtering, and PNG
  dimension validation;
- `KeyImageInventory`, the ordered HID-to-artwork candidate vocabulary, and
  `resolve_key_overlays`;
- a small resource-preparation error that is mapped to the existing
  `Live2dErrorCode::ResourceIo` by the Core adapter.

The crate depends on `bongocat-model`, `bongocat-render`, `image`, and the
standard library. It does not depend on Cubism Core, `bongocat-live2d`,
runtime, GPUI, platform APIs, or a GPU backend. It does not decide input
bindings or mutate runtime state; the app still applies the inventory when
building bindings, and runtime still consumes only the resulting immutable
`RenderSnapshot`.

`bongocat-live2d` now asks `bongocat-live2d-render` to prepare resources during
model loading, then retains Core loading, parameter/part writes, Core error
mapping, and snapshot production. `bongocat-app` and `bongocat-runtime` import
the inventory and overlay resolver from the new crate directly.

`bongocat-render` remains the immutable snapshot/resource/transport contract.
The actual Metal and D3D11 GPU owners, window surfaces, and present loops stay
in `bongocat-overlay`; this slice does not move platform GPU code or make the
renderer read configuration or input services.

## Consequences

- model resource preparation and key-image contracts can be tested without the
  Cubism SDK;
- `bongocat-live2d` no longer owns the image decoder or key-image directory
  scan;
- app/runtime no longer acquire those contracts indirectly through the Core
  crate;
- the existing "missing artwork means no binding" behavior and side-specific
  overlay rules remain unchanged;
- no new renderer backend, window owner, or GPU lifecycle is introduced.

## Rejected alternatives

- Moving all of `bongocat-overlay` into a new crate: rejected because its
  platform files also own windows, input routing, placement, and shutdown;
  separating those owners requires a separate platform seam.
- Moving PNG decoding into `bongocat-render`: rejected because that crate is the
  immutable contract and must remain independent of filesystem/image details.
- Letting the renderer decide whether a key has a binding: rejected by ADR-0042;
  the app gates bindings using the shared inventory contract.
- Reimplementing key-image scanning in app and runtime: rejected because the
  inventory and the loader must remain one source of truth.

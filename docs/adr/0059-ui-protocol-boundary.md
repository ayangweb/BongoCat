# ADR-0059: UI protocol boundary

- Status: accepted
- Date: 2026-09-24
- Depends on: ADR-0035, ADR-0054

## Context

The settings and update services exposed their typed DTOs, commands, replies,
snapshots, and bounded channel clients from `bongocat-ui`. That made the
application service layer depend on a GPUI crate even though the service never
rendered a window. It also put presentation-only policy (debounce timing,
localized labels, update polling cadence, and filesystem-backed title probing)
next to the wire contract.

## Decision

Create `bongocat-ui-protocol` as the process-local contract crate. It owns:

- settings and update snapshots, stable enums, error codes, and project-owned
  DTOs;
- typed commands, replies, operation controls, state handles, and bounded
  service clients/endpoints;
- `SettingsWindowPlacement`/`SettingsWindowState`, which are shared service
  state rather than GPUI entities;
- pure, path-based model-source title normalization when the adapter supplies
  whether the selected path is a directory.

The crate has one direct external dependency, `async-channel`. It does not
depend on GPUI, an operating-system API, `bongocat-config`,
`bongocat-platform`, `bongocat-i18n`, `bongocat-update`, model, or runtime
crates. The application maps config/platform/update values into these project
types at its service boundary.

`bongocat-ui` keeps GPUI views and presentation policy: the settings patch
debouncer, language display names, update error message keys, the update
window poll interval, and filesystem inspection performed by the picker/view
adapter. It depends on the protocol crate and re-exports it only inside the UI
crate for the existing view-module wiring; application services import the
protocol directly.

The GPUI executable still depends on `bongocat-ui` for windows, views, and
handles. It imports settings/update protocol types from
`bongocat-ui-protocol`; this keeps the composition root explicit without
making the service layer depend on rendering.

## Consequences

- settings and update service tests run without a GPUI test platform;
- app service code no longer obtains protocol DTOs through the rendering crate;
- protocol tests pin stable codes, command ordering, bounded transport, and
  snapshot revisions;
- GPUI tests retain rendering, localization, debounce, and window-handle
  assertions;
- the protocol is in-process and typed; it is not a serialization or IPC
  schema, so no serde dependency is introduced;
- changing a producer's public DTO now requires an explicit protocol change
  rather than an incidental UI module edit.

## Rejected alternatives

- Moving all of `bongocat-ui` into a protocol crate: rejected because it would
  preserve GPUI and presentation dependencies in the service contract.
- Keeping protocol types in `bongocat-ui` with a facade: rejected because it
  leaves the application-to-rendering dependency intact.
- Adding serde or serializing the command channel: rejected because the current
  boundary is an in-process typed queue, not a persisted or network protocol.
- Moving debounce, localization, and render scheduling into the protocol:
  rejected because those are view policy and would make the contract depend on
  presentation details.

# ADR-0084: Single-process multiplayer display on Linux

## Status

Accepted on 2026-11-26.

## Decision

Keep one BongoCat application, one room connection, one local input collector
and one settings owner per host. Each displayed cat has its own runtime and
Wayland window. The local producers feed the local runtime and can publish to
the room. The room bridge routes edges and pointer samples by device ID.
Each runtime owns its pressed state and produces its own frames.

Reuse the main window's Vulkan instance across renderers, with separate
GPU resources and surfaces. The host supports the local cat and up to two
selected remote cats. Each remote source has its own pressed state and
pointer consumer. All layer-shell positions are stored locally by device
ID and restored when a device is displayed again. Alpha output subscribes
to the selected runtimes and composites them into one camera frame.

The display controls use the room roster for selection. Device names are the
primary labels and IDs are secondary details. A selected peer retains its
last observed name while the settings window remains open.

The existing model-window visibility control gates all desktop cats. Every
desktop cat uses the current window settings. Idle visibility follows that
cat's input source, while the interaction modifier is read from the host's
physical keyboard. The display controls use switches for local and remote
devices.
The device selection is kept independently: showing model windows restores
the selected sources, and an empty selection still draws no cat.
Hiding the windows does not change the room connection or input sharing.

Saving the display list updates the existing room's input route and window
selection. The room bridge releases the previous source's held controls when
the route changes. Changing a remote device while the local cat is selected
leaves that cat's input alone. A room session prepares the second runtime at
startup, while its Wayland window is created only when two sources are shown.
The settings worker prepares the desired listener and authenticated client
before saving configuration. An unavailable candidate leaves the original
transport live where both can coexist. If a candidate must reuse the old
listener's port, the worker stops that listener first and restores it on
failure. A failed configuration write reapplies the previous network settings.
The Linux product owns a stoppable listener. Stopping it closes accepted
sockets, including clients still performing the handshake, and joins its
workers before the operation returns. Reconfiguration coordinates the relay
and client lifecycles so a Follow Local connection switches with its relay.
The product also owns the room client worker. Cancellation interrupts its
current socket and prevents it from joining or reconnecting once cancelled.
Shutdown waits within a bounded budget. An OS name lookup or TCP dial already
in progress can finish after that budget, without activating a session.
The capture producer outlives its room transport. Its outbound queue can be
replaced independently, and stopping a transport drains or stops its bridge
workers and releases held input from the departing room. The permission check
for a candidate room now waits for the relay's admission response. A rejected
candidate leaves the active session alone. A successful admission on the same
relay with the same device ID retires the existing socket. If attaching the
new client fails, the settings worker opens the previous room again.

## Rationale

This reuses the existing application lifecycle and avoids coordinating
configuration, room membership and shutdown across rendering processes. It
uses a second model and renderer only in the two-window mode. A renderer or
driver failure can affect both windows within the process.

On Linux, relay listeners use `SO_REUSEADDR` before binding so closed
connections in `TIME_WAIT` do not prevent reusing the port. A live listener
still causes a bind failure. The Linux-only dependency is socket2 0.6.5,
already pinned in the workspace lockfile. It is maintained under rust-lang,
supports Linux with Rust 1.70, and is licensed MIT OR Apache-2.0. Its safe
socket API stays inside the listener factory. The WebSocket upgrade uses a
handshake deadline before switching to the short input polling interval.

## Verification

Test independent pressed state when the same key is held locally and released
by the peer. On two Linux desktop sessions, verify up to three distinct windows on a
host, keyboard and pointer routing in both directions, relay rebinding, room recovery, and clean application exit.

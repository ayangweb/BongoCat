# ADR-0083: Linux OBS Alpha output

## Status

Proposed; implementation and operational verification are incomplete.

## Context

The Linux/Wayland fork needs an optional transparent source in OBS. Capturing the compositor surface does not establish that transparency survives. The laptop spike in `docs/benchmark/obs-alpha-spike.md` confirmed that OBS 33.0.1 can compose a v4l2loopback 0.15.4 AR24 test source with fully transparent and partially transparent pixels. This evidence does not establish correct model edges, sustained performance or device lifecycle behavior.

## Decision

Add an opt-in Alpha output to the Linux fork. An off-screen output consumes existing render data and produces 1280×720 BGRA frames at an initial target of 30 frames per second. It must not wait for a Wayland surface to become visible. The desktop renderer and input/runtime path must not wait for OBS, GPU readback or device writes: bound in-flight readbacks and queued frames and discard stale frames when the consumer falls behind. Define and test conversion between the renderer premultiplied representation and the OBS input representation; preserve transparency in the background and model edges. Report errors and stop output without stopping the desktop cat or multiplayer.

The settings surface provides separately observable driver, module, camera and output states, explicit refresh, install and uninstall actions, and an output switch which defaults off for existing configurations. Installation is an explicit user action, never part of application installation. Missing drivers are reported rather than installed. A narrowly scoped privileged operation may load the installed module and create a BongoCat-owned device; normal frame production runs as the desktop user. Per-device access is part of installation. No boot service or persistent privileged executable is installed; after a reboot, the user reinstalls the device in settings and authorizes the operation again. Uninstall removes only verifiably owned resources, never the module or another application device; uncertain identity, denied authorization, active users and partial failure are reported without destructive guessing. Cleanup must be idempotent and recoverable after interruption.

## Consequences and validation

The OBS spike gates this approach but does not complete it. Verify lifecycle and permissions on the target Linux system, including manual reinstallation after reboot, interruption, simultaneous other loopback devices, authorization refusal and cleanup. Verify ordinary and remote input, model switch, transparent and semitransparent edges over light and dark backgrounds, OBS disconnect/reconnect, bounded shutdown, and CPU/GPU/memory/latency at the target frame size. Record unsupported environments and do not declare full support without the evidence. New persisted fields require default-on-missing behavior, schema/fixture updates and an old-config regression test.

## Integration observations

The Linux product session currently acquires and presents frames through `ProductOverlaySession::tick`; it also resizes its canvas to the Wayland window. A fixed 1280×720 output cannot safely reuse that window-sized texture as its only render target. The output must retain an independently sized target and scheduling path while consuming the same model frame. The persisted preference alone is not an operational switch: changes must reach the live output owner through a typed command and produce status that reflects actual startup or failure. A user-visible toggle must not be exposed as working until that path exists.

Install and uninstall invoke the currently running application via pkexec after
explicit user authorization. This executes that application with root privileges
for the fixed requested operation; a user-writable development build must
therefore be trusted by the user at authorization time. There is no root service
or separately installed privileged executable. Ownership records and udev rules
remain root-controlled; validate recorded device identity immediately before
deleting a node and fail closed when ownership cannot be established.

## V4L2 adapter dependency

`bongocat-platform` uses `v4l` 0.14.0, the latest non-yanked stable version
reported by `cargo info` on 2026-11-02. Its MIT license is compatible with the
workspace policy. The latest crate changelog entry is 2023-05-13, so maintenance
activity is a risk; the project-owned `LinuxAlphaCamera` and `AlphaCameraError`
types isolate it from the public API of shared business crates. The dependency
contains Linux FFI/unsafe code and generates kernel bindings with bindgen; our
adapter contains no unsafe code. Linux is the only target that compiles it. The
adapter validates the loopback driver, dedicated label, AR24 format, stride and
straight-alpha flag, opens nonblocking, and treats full queues as dropped frames.
The label is a guard against accidental output, not proof of ownership for the
privileged install or removal operation. Toolchain and license audit still need
verification across all supported build environments.

## Vulkan instance reuse

On the laptop, enabling Alpha at startup while GPUI opened settings crashed
reproducibly in the Vulkan loader. GDB showed the main GPUI thread and the
Alpha thread simultaneously enumerating Vulkan instance extensions; the latter
was loading the NVIDIA ICD while the former called a null address inside the
loader. This observation establishes concurrency at the crash, not the driver's
ultimate root cause. The output renderer now clones the overlay's existing wgpu
instance and requests its own adapter/device and queue; it does not create a
second instance. Separate devices retain independent error scopes and bounded
output work. The Alpha-enabled application now starts with settings open, produces
a real AR24 model frame, and exits normally in an ignored Wayland integration test.
Long-duration stability, settings-window reopen during output, and performance
remain required before declaring the feature complete.


## Wayland compositor pacing

A laptop run with the overlay covered produced only about one camera frame per
second in both debug and release builds, although the loopback reported AR24 at
30 fps. Timing instrumentation showed the output worker mostly waited for
frames from the foreground loop. The overlay was acquiring swapchain images
without waiting for its existing Wayland frame callback; a throttled compositor
could consequently block that loop. The session now skips surface acquisition
while the callback is pending but still processes snapshots and publishes Alpha
frames. The debug build then produced approximately 11.9 fps in a five-second
capture. The ignored integration smoke test checks sustained production. A
subsequent optimized Development build on the same laptop delivered approximately
29.9 fps over a ten-second V4L2 capture at AR24 1280×720; process lifetime-average
CPU at 15 seconds was 21.0% and RSS was 194728 KiB. This short capture is not a
long-duration stability test. GPU utilization, end-to-end latency and behavior
under other compositors remain unmeasured.


## Independent runtime frame subscription

During a longer live laptop session with OBS capturing the camera, the main
thread was observed waiting in an Intel Vulkan DRM timeline ioctl for at least
five seconds. Over a separate four-second sample, process write accounting
increased by approximately four 1280×720 BGRA frames, consistent with an output
cadence near one frame per second. Kernel wait and process counters establish
the observed stall, not its ultimate driver or compositor cause. The earlier
Wayland frame-callback gate alone therefore does not establish independent
output pacing.

The runtime-to-overlay channel now supports a single bounded latest-frame video
subscription. The producer tries the subscription lock without waiting and
drops stale frames; the video worker consumes it independently of desktop
presentation. Model preparation frames are excluded, so the output only sees
the committed model generation. A real GPU/V4L2 smoke test captured transparent,
opaque and partial-alpha model pixels and twelve successive frames without
polling the desktop consumer, then stopped and removed its separate temporary
production-named camera. A subsequent 60-frame debug capture on the same GPU
reported roughly 11.4–12 fps while the user's older app remained at about 1 fps.
The optimized GPU/V4L2 test then captured 120 frames at approximately
30.1–30.7 fps without polling the desktop consumer, with the user's normal
camera left running separately. After the rebuilt application was started,
the user reported smooth OBS output; a 60-second sample of process write
accounting was consistent with approximately 29.93 fps, but includes
non-video writes. Long-duration stability and end-to-end latency still require
verification.


## Group output

One camera composites the selected local and remote cats into one transparent
frame. The output worker subscribes to each selected runtime independently of
desktop draws, fits the union of the local window rectangles proportionally
inside 1280×720, and blends the readbacks into straight-alpha BGRA. Offline
selections still contribute their bounds so an online cat stays in the same
place when another device disconnects. The group uses the same bounded
30 FPS scheduler and the existing device installation and ownership path.

## Eight-hour validation decision

The user requested skipping the eight-hour run on 2026-11-03. A roughly
three-minute sample was collected and stopped at their request; it does not
establish eight-hour stability. Reboot/manual installation, graphical
authorization, the identity audit documented in the handoff, mixed-input
durability and end-to-end latency remain separate open checks.


## Verifiable camera identity and existing installations

The user approved one explicit, authorized reinstall for the existing device.
The root-owned record retains its v1 fields and may now include the device-node
inode. The UID, node number, boot ID, readiness and inode are already observable
through system device and process metadata; new records are root-owned,
world-readable and not writable by unprivileged users. This lets the ordinary
output process validate the record, current boot, expected owner, device inode
and root-owned udev rule before opening the camera. After opening, it checks
the inode of the actual file descriptor again to catch replacement between
validation and open. The privileged installer still reads old mode-0600 records
and accepts the original five-field format.
An explicit reinstall in the same boot checks the old owner, label, driver and
record, then enrolls the currently matching device without replacing it if the
old record marks installation complete. An interrupted installation with no
recorded inode reports a conflict when a matching node already exists. Legacy
records cannot independently prove that an identically named device was never
substituted before this one-time enrollment; this is a limitation of those
previous records, and the current user's device was confirmed in use before
the approved upgrade. Inode and boot checks detect subsequent replacement;
uninstall fails closed if they disagree.

A privileged real-device regression exercises old-boot records, legacy private
records, explicit upgrade and a same-name/same-number replacement. It verifies
that status and output reject stale or replaced identities, and that uninstall
does not remove the replacement. The test uses an isolated production-named
camera and cleans it afterward.

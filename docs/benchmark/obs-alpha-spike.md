# OBS Alpha input spike

Date: 2026-11-02. Host: hyqaq-laptop, Linux/Wayland.

- OBS Studio 33.0.1; v4l2loopback 0.15.4.
- A temporary dynamically created loopback device was configured for 1280×720, `AR24` (32-bit BGRA), sRGB, with 5120 bytes per row.
- A test producer sent red bands with alpha values 0, 64, 128, 192, and 255. The user confirmed that transparency and partial transparency appeared correctly when composed in OBS.
- This validates the basic camera-to-OBS composition path for this setup. It does not validate the renderer's premultiplied-alpha conversion, model edges, frame pacing, restart, device lifecycle, or performance.
- The temporary producer and device were removed after the OBS source released the device.

## Renderer integration constraints

The Linux renderer draws into a `Bgra8Unorm` canvas cleared to transparent, with a premultiplied Wayland presentation mode. The canvas is recreated on window resize, and may contain a model background. The OBS spike used straight-alpha red pixels, so it does not prove that copying the existing canvas directly will produce correct edges in OBS. A product implementation must explicitly test and, where required, convert the alpha convention and isolate the intended transparent output. The renderer currently acquires a Wayland surface before drawing and defers when it is occluded or unavailable; OBS output must be evaluated independently of that acquisition so a hidden or occluded desktop surface does not freeze the stream.


### Integrated application sampling (2026-11-03)

With the user's existing development configuration, Alpha output enabled and the
PC publisher connected, a 10-second sample of the running optimized laptop app
reported 20.9% of one CPU core and 197624 KiB RSS. Deduplicated per-process i915
DRM client counters reported 6.4% summed render-engine busy time during the
same interval. This is process render-engine accounting, not total GPU use or
a measurement of end-to-end latency. The user was free to interact during the
sample; it is not a controlled benchmark or long-duration stability test.


### Network baseline (2026-11-03)

A ten-packet ICMP probe from the PC to the laptop on the active LAN measured
1.718 / 1.922 / 2.604 ms minimum / mean / maximum round trip with no packet
loss. This is network round-trip time only; it does not measure capture, relay,
rendering, V4L2, OBS compositing or input-to-photon delay.


### Independent output after desktop stall fix (2026-11-03)

A separate optimized GPU/V4L2 test captured 120 AR24 1280×720 frames at
approximately 30.1–30.7 fps without polling the desktop render consumer, while
the user's development camera remained active. The temporary second device was
removed. After the user restarted the application, they reported the OBS scene
was smooth. During the first 60-second process sample, total written bytes
corresponded to approximately 29.93 frame-sized writes per second; this includes
non-video writes, so it is an upper-bound estimate rather than direct V4L2 frame
accounting. The process used 26.92% of one CPU core, RSS was 207916 KiB, and
deduplicated i915 per-process render-engine busy time was 8.93% for that window.
The measurement overlaps a low-priority release test build and is not an
end-to-end latency or eight-hour stability result.


### Short live sample and skipped soak (2026-11-03)

At the user's request the eight-hour sample was stopped after approximately
three minutes. The three one-minute samples reported 29.93 frame-sized writes
per second, 26.92–28.33% of one CPU core, 207916–207920 KiB RSS, 49 threads
and 93 file descriptors. This write-volume figure includes non-video writes;
the sample is not an eight-hour stability result.

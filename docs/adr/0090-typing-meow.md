# Typing meow

Keyboard presses may play a short synthesized meow using the existing audio owner.
The existing, default-off motion sound setting controls both motion and typing sounds;
no configuration format change or additional audio device owner is introduced.

Only accepted, fresh captured keyboard down edges trigger playback. Releases, repeats,
mouse/gamepad input, reconciliation and reset events do not. Hidden overlays, ignored
keyboard input and model activation suppress typing sounds. A monotonic 180 ms cooldown
limits rapid typing; events are never delayed or replayed later. Typing does not interrupt
an existing voice. Motion playback retains priority and may replace a typing sound.

The clip is synthesized locally, cached by the audio backend, and needs no recording,
network request or external asset license. Missing output devices use the existing
degraded-service reporting. Mute and shutdown use the existing stop/join path.

Validation: input filtering/cooldown tests, generated waveform bounds, audio priority,
mute and shutdown tests, followed by Windows listening and full repository checks.

Windows validation: `just check`, 133 focused audio/runtime tests, the explicit real
output-device typing test, and `just dev-smoke` passed. The local development preview
uses release-profile environment overrides (LTO off, 16 codegen units, optimization
level 1) to reduce build memory. Subjective listening and macOS validation remain open.

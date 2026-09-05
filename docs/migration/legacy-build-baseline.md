# Legacy Vue/Tauri Build Baseline

This record freezes the last independently reproducible legacy application
build. It is behavior evidence only; the Native Rewrite does not depend on the
legacy workspace, its configuration, or its generated artifacts.

## Environment

- Date: 2026-09-06 (Asia/Shanghai)
- Host: macOS 26.5.2, arm64 (Apple Silicon)
- Xcode: 26.6 (Command Line Tools installed)
- Rust: `rustc 1.97.1`, `cargo 1.97.1`
- Node.js: `v25.9.0`
- pnpm: `9.12.3`

`pnpm tauri info` reports the legacy stack as Vue.js + Vite and Tauri 2.10.3
(with the repository's pinned plugin versions).

## Reproduction

From the repository root, with the checked-in `pnpm-lock.yaml`:

```text
pnpm build
pnpm tauri build --debug --bundles app
```

`pnpm build` transformed 4,406 modules, produced 18 files under `dist/`, and
completed the icon generation script. The Tauri command rebuilt the frontend,
compiled the `bongo-cat` Rust target, and produced:

- `target/debug/bundle/macos/BongoCat.app`
- `target/debug/bundle/macos/BongoCat.app.tar.gz`
- `target/debug/bundle/macos/BongoCat.app.tar.gz.sig`

The bundle metadata is `CFBundleIdentifier=com.ayangweb.BongoCat` and
`CFBundleShortVersionString=1.1.0`. SHA-256 values from this run:

```text
2f71dff4cc597265eb6c55533b295251ab7cf43bfad00bfb338696651bb1b28c  target/debug/bundle/macos/BongoCat.app.tar.gz
1e9dffbbb09e0822c9c76eb8868ee8c809ad15126933f4aa21568848d15cae9b  target/debug/bundle/macos/BongoCat.app/Contents/Info.plist
```

## Launch observation

Launching `target/debug/bongo-cat` reached
`applicationDidFinishLaunching` and created both the main and preference
windows on the macOS event loop. The legacy window handler hides windows on
close and provides no command-line auto-quit mode, so this baseline records
launch evidence rather than claiming an automated graceful-exit assertion.

The build emits a pre-existing warning for `block 0.1.6`, and Tauri reports
that the configured updater signing secret does not match its public key. Both
are legacy release risks and are intentionally out of Native Rewrite scope.

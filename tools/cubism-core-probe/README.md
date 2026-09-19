# Cubism Core Probe

This standalone Rust tool validates a locally supplied Cubism Core and generated raw
bindings without committing licensed SDK material. It checks the fixed Core/Moc
versions, aligned Moc and Model lifecycle, parameter/part/drawable arrays, enhanced
rendering blend/offscreen arrays, masks, geometry, canvas data, and repeated teardown.

The tool intentionally has no default binding or library path. Prepare the official
header, generated binding, and matching Core outside the repository, then compile with
explicit paths:

```text
BONGOCAT_CUBISM_BINDINGS=/external/bindings.rs \
RUSTFLAGS='-L native=/external/core -l dylib=Live2DCubismCore' \
cargo build --manifest-path tools/cubism-core-probe/Cargo.toml --release \
  --target-dir /external/probe-target
```

Run one or more absolute Moc paths as `id=/absolute/path.moc3`; use repeated lifecycle
cycles for the Phase 0 evidence:

```text
DYLD_LIBRARY_PATH=/external/core \
/external/probe-target/release/bongocat-cubism-core-probe --cycles 100 \
  standard=/absolute/path/demomodel.moc3
```

On Windows, link the matching x64 import library and place the official DLL next to the
probe executable or on its controlled DLL search path. Windows ARM64 remains unsupported
until Live2D supplies a desktop ARM64 Core; this probe must not use the UWP artifact.

Never commit the official header, Core binary, real generated bindings, extracted SDK,
or probe target directory. Probe output proves Core ABI and model-array access only; it
does not prove Framework behavior, GPU rendering, publication rights, or another target's
ABI.

# Cubism Core Binding Generator

This offline tool fixes the raw-binding generation contract for Cubism 5 SDK
for Native R5. It does not download the SDK, accept a license, link Cubism
Core, or implement product behavior.

The committed fixture header and expected bindings are synthetic BongoCat test
data. They verify the symbol allowlist, three R5 desktop target configurations,
the C ABI, deterministic output, and generated-file drift. They are not copied
from the Cubism SDK. Windows x86 is outside the product target set; Windows
ARM64 remains a product target but R5 has no matching desktop Core to bind.

## Fixture check

```text
cargo run --manifest-path tools/cubism-bindgen/Cargo.toml --locked -- check-fixtures
```

Expected bindings are generated files and must not be edited manually. After
reviewing an intentional generator or synthetic-header change, refresh them
with:

```text
cargo run --manifest-path tools/cubism-bindgen/Cargo.toml --locked -- refresh-fixtures
```

## Licensed R5 header

Only a maintainer who has legally acquired the pinned R5 SDK may run real
generation. Use the header SHA-256 reported by `tools/inspect-cubism-sdk.py`:

```text
cargo run --manifest-path tools/cubism-bindgen/Cargo.toml --locked -- \
  generate \
  --header /absolute/path/outside/repository/Live2DCubismCore.h \
  --expected-header-sha256 <64-hex-digest> \
  --target aarch64-apple-darwin \
  --output-directory /absolute/new/path/outside/repository/cubism-bindings-arm64
```

The tool refuses repository-local licensed headers and output, refuses an
existing output directory, and emits only `bindings.rs` plus
`provenance.json`. The provenance records header/output/config hashes,
bindgen, libclang, Rust edition, and target configuration without recording
the local SDK path.

Until Live2D provides written permission, neither the real header nor generated
bindings may be committed, cached by CI, attached to issues, or distributed.
Review must happen in the controlled external directory and cover:

1. header hash against the independently verified R5 SDK report;
2. exact generated symbol inventory and target calling convention;
3. a second generation with the recorded bindgen/libclang configuration;
4. diff and SHA-256 equality between both outputs;
5. compile/link/ABI smoke against the matching official Core artifact.

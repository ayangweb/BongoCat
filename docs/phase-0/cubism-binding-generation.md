# Cubism Core Raw Binding Generation

状态：合成输入的生成契约与漂移检查已完成；真实 R5 header 已取得但生成物保持仓库外
记录日期：2026-08-30

## 1. Scope

`tools/cubism-bindgen` 是不随应用发布的离线 Rust 工具。它只负责把固定的
Cubism Core C header 转换为 raw Rust declarations，不加载 Core、不创建模型，
也不包含动作、物理、渲染或其他业务逻辑。

真实 R5 header 和由它派生的 bindings 当前都受 Live2D 许可门禁约束。在
Live2D 书面确认可发布前，两者只能位于仓库外的受控开发目录，不能进入 Git、
CI cache、workflow artifact、issue 或 release。

## 2. Fixed Generator Contract

| Item                   | Fixed value                                                     |
| ---------------------- | --------------------------------------------------------------- |
| SDK release            | Cubism 5 SDK for Native R5 (`5-r.4.1`)                          |
| Core version           | `05.01.0000`                                                    |
| Header path inside SDK | `Core/include/Live2DCubismCore.h`                               |
| Generator              | `bindgen 0.72.1`                                                |
| Hash implementation    | `sha2 0.11.0`                                                   |
| Generated Rust target  | Rust `1.85`, edition 2024                                       |
| Symbol allowlist       | functions, types and variables matching `^csm[A-Za-z0-9_]*$`    |
| Formatter              | bindgen `prettyplease` feature from the same locked graph       |
| Output                 | `bindings.rs` and `provenance.json` in a new external directory |

Direct dependencies were checked against crates.io on 2026-08-29 and are exact-pinned
in the tool's `Cargo.toml`/`Cargo.lock`. `bindgen` is maintained by the rust-bindgen
project under BSD-3-Clause; `sha2` is maintained by RustCrypto under MIT OR
Apache-2.0. Both are build-time tooling only and do not enter the application binary.
The replacement boundary is the tool CLI and its two output files: a future generator
can replace bindgen only after reproducing the same symbol/ABI contract and passing
real Core smoke tests.

The generator disables recursive allowlisting, comments, layout tests and `Debug`
derivation; merges extern blocks; sorts declarations semantically; uses `core`; and
does not invoke an independently changing `rustfmt`. These options are hashed into
the provenance as config revision `cubism-core-r5-v1`.

## 3. Target ABI Inputs

| Rust target              | Clang target             | Required ABI result  |
| ------------------------ | ------------------------ | -------------------- |
| `x86_64-pc-windows-msvc` | `x86_64-pc-windows-msvc` | C calling convention |
| `aarch64-apple-darwin`   | `arm64-apple-darwin`     | C calling convention |
| `x86_64-apple-darwin`    | `x86_64-apple-darwin`    | C calling convention |

`i686-pc-windows-msvc` is deliberately rejected because ADR-0010 excludes Windows
x86 from the Native Rewrite. `aarch64-pc-windows-msvc` remains a product target but
is also rejected by this R5 generator because R5 has no desktop Windows ARM64 Core
artifact. Generating declarations would create a false impression that ARM64 can be
linked and released.

libclang is a parser input that bindgen discovers at runtime. Its full version is
recorded in `provenance.json`; a real binding is accepted only after a second run with
the recorded configuration produces the identical output hash. CI additionally runs
the synthetic golden on its own libclang environment, so a parser change that alters
output is visible rather than silently accepted.

## 4. Safety and Review Flow

The real generation command requires:

- an absolute header path named exactly `Live2DCubismCore.h`;
- an expected 64-character header SHA-256 from the offline SDK inspector;
- one of the three explicit generation targets;
- a new absolute output directory outside the repository.

The canonical header and output parent are checked against the repository root. The
tool refuses an existing output directory and never overwrites review evidence. It
does not include the source path in generated bindings or provenance. Output is
created only after header hashing, bindgen generation, required-symbol checks,
allowlist checks and calling-convention checks succeed.

Review requires all of the following before a generated file can be used locally:

1. Match archive/header hashes to the independently reviewed SDK inspection report.
2. Confirm the complete generated symbol inventory contains only approved `csm*` API.
3. Confirm C ABI and integer/pointer types for the matching target.
4. Repeat generation and compare binding/config hashes byte for byte.
5. Compile and link against the matching official Core artifact.
6. Query Core version, then run Moc/Model/drawable lifecycle smoke tests.

No generated raw binding becomes a product source merely because steps 1-4 pass.
Publication permission, safe wrapper review and real platform ABI evidence remain
separate gates.

## 5. Synthetic Evidence

The committed header under `tools/cubism-bindgen/fixtures/` is authored test data,
not an SDK-derived file. Three generated goldens verify:

- deterministic repeated output;
- the `csm*` allowlist excludes unrelated vendor declarations;
- required lifecycle and drawable symbols remain present;
- all three available product/R5 target intersections use the expected C ABI;
- each generated file compiles as Rust 2024 metadata for its matching target;
- generated-file changes fail CI until explicitly reviewed and refreshed.

Reproduce with:

```text
cargo fmt --manifest-path tools/cubism-bindgen/Cargo.toml -- --check
cargo clippy --manifest-path tools/cubism-bindgen/Cargo.toml --locked --all-targets --all-features -- -D warnings
cargo test --manifest-path tools/cubism-bindgen/Cargo.toml --locked
cargo run --manifest-path tools/cubism-bindgen/Cargo.toml --locked -- check-fixtures
cargo check --manifest-path tools/cubism-bindgen/Cargo.toml --locked --release
```

This proves the generation workflow, not complete compatibility with the supplied R5
header or binaries. The real header SHA-256 is
`0564a03edd0d56b90bac704bbbcc4e560b3d3d9000b49a0bd5d9cb886b414022`; the P0
raw-binding TODO remains open until external generated outputs are reviewed and all
target ABI/model smoke evidence exists.

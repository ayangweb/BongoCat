# Cubism Core Raw Binding Generation

状态：生成契约已固定；真实 R5 target bindings 已进入产品 crate
记录日期：2026-08-30

## 1. Scope

`tools/cubism-bindgen` 是不随应用发布的离线 Rust 工具。它只负责把固定的
Cubism Core C header 转换为 raw Rust declarations，不加载 Core、不创建模型，
也不包含动作、物理、渲染或其他业务逻辑。

真实 R5 header 和由它派生的 bindings 已由维护者批准作为开发基线固定到
`native/vendor/cubism/5-r.5` 与 `native/crates/bongocat-live2d/src/sys`。生成仍是
显式离线维护操作，不在普通 build 或 CI 中运行；完整 SDK ZIP 和 Framework 源码
不进入 Git、CI cache、workflow artifact、issue 或 release。

## 2. Fixed Generator Contract

| Item                   | Fixed value                                                     |
| ---------------------- | --------------------------------------------------------------- |
| SDK release            | Cubism 5 SDK for Native R5 (`5-r.5`)                            |
| Core version           | `06.00.0001`                                                    |
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

重新生成命令要求：

- header 必须是名为 `Live2DCubismCore.h` 的绝对路径；
- 必须提供 offline SDK inspector 记录的 64 字符 header SHA-256；
- target 必须属于三个显式 generation target；
- 输出必须是全新的绝对 staging 目录，review 完成前不得覆盖产品 binding。

生成前会检查 canonical header 与 staging output。工具拒绝已存在的输出目录，不会
覆盖 review 证据，也不会把源路径写入 binding 或 provenance。只有 header hash、
bindgen generation、required-symbol、allowlist 和 calling-convention 检查全部成功后
才创建输出。

Review requires all of the following before a generated file can be used locally:

1. Match archive/header hashes to the independently reviewed SDK inspection report.
2. Confirm the complete generated symbol inventory contains only approved `csm*` API.
3. Confirm C ABI and integer/pointer types for the matching target.
4. Repeat generation and compare binding/config hashes byte for byte.
5. Compile and link against the matching official Core artifact.
6. Query Core version, then run Moc/Model/drawable lifecycle smoke tests.

生成结果只有在 target ABI、symbol diff、safe wrapper 和模型 smoke review 后才能
更新产品 source。最终发布清单与真实平台 ABI evidence 仍是独立门禁。

## 5. Synthetic Evidence

The committed header under `tools/cubism-bindgen/fixtures/` is authored test data,
not an SDK-derived file. Three generated goldens verify:

- deterministic repeated output;
- the `csm*` allowlist excludes unrelated vendor declarations;
- required lifecycle and drawable symbols remain present;
- the standalone Core probe compiles and passes Clippy against synthetic bindings without an SDK;
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

This proves the generation workflow. The real r.5 header SHA-256 is
`6f1802780d1eb36ff39705e0764f9eeed9b41c313a13ac155270c6f4ad51d53f`; an external
arm64 generation produced binding SHA-256
`6cd53ddbb173d73a842b33a507c5c03c879adcb05a8c005730b58c1f0f061364` and passed the
macOS arm64 Core/model probe in `cubism-core-r5-probe.md`. Commit `57118ff` 将审阅后的
macOS arm64/x64 与 Windows x64 binding 固定到产品 crate，Windows x64 release
交叉 check 已通过。Synthetic contract 现在要求 r.5 的 `csmGetRenderOrders`、drawable
blend mode、part offscreen index 和全部 offscreen array。第二人重生成 review 及
Windows x64/macOS x64 原生 ABI evidence 完成前，P0 raw-binding TODO 保持未完成。

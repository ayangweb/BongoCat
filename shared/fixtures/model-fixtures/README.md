# Model Package Fixtures

These fully synthetic fixtures define Native Rewrite model-import preflight
behavior. They contain no Cubism Core data and must not be presented as
renderable Live2D models.

## Scope

`cases.json` registers each fixture, its preflight stage, and the exact stable
diagnostic expected from that stage. The current cases cover:

- missing moc reference;
- malformed model3 JSON;
- non-ASCII and space-containing paths;
- a compact PNG header declaring a 32768 x 32768 texture;
- a model reference escaping its package root;
- multiple model3 entry files.

The oversized texture is stored as ASCII hex and materialized only in an
isolated test directory. This keeps the repository small while exercising a
real PNG signature and IHDR. Its absence of image data is intentional: the
dimension guard must reject it before image decoding or GPU allocation.

## Boundaries

- These fixtures validate package discovery, JSON parsing, reference resolution,
  and texture-header limits only.
- Placeholder `.moc3` content in the accepted path case is not passed to Cubism
  Core.
- Cubism consistency, drawable, motion, physics, expression, and rendering tests
  require licensed SDK binaries and separately authorized model fixtures.
- Import checks must run before copying files into the environment model store.
- Rejection must not modify the source package or current active model.

`preset-model3-index.json` is a normalized snapshot produced by the Rust
`spikes/model-package` parser for the three tracked preset packages. It freezes
model3 references, image dimensions, motion/expression groups, companion image
indexes, total package size, and unreferenced files. The snapshot contains no
model bytes and does not claim Cubism runtime compatibility.

Run the repository validator with:

```text
python3 tools/validate-fixtures.py
cargo test --manifest-path spikes/model-package/Cargo.toml --locked
```

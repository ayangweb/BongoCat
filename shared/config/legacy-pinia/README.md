# Legacy Pinia Configuration Fixtures

These fixtures model the five independent JSON stores written by the historical
Tauri Pinia plugin. They are synthetic and contain no copied user paths, model
ids, shortcuts, window coordinates, or model files.

## Cases

- `default/`: a complete current-shape configuration with default-like values.
- `upgraded-with-custom-model/`: current fields mixed with conflicting legacy
  fields, stale derived state, and one synthetic custom model.
- `damaged/cat.json.invalid`: an intentionally truncated store. Its non-JSON
  extension prevents general JSON validators from accepting it accidentally.

`$BUNDLED_MODELS` and `$LEGACY_APP_DATA` are fixture tokens, not environment
variables or filesystem paths. A fixture runner must resolve them only inside an
isolated temporary directory.

The historical format has no `schemaVersion`. `manifest.json` belongs to the
Native Rewrite test suite and must not be mistaken for an old application file.

Migration tests must read copies of these files and must preserve the source on
all failures.

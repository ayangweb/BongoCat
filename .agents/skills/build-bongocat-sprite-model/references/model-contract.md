# BongoCat Sprite Model Contract

## Sources Of Truth

Re-read these files before implementing because the contract may evolve:

- `src/utils/sprite.ts`
- `src/utils/model-runtime.ts`
- `src/stores/model.ts`
- `src/composables/useGamepad.ts`
- `src/pages/preference/components/model/components/upload/index.vue`
- `src-tauri/assets/models/qingxiao/model.json`

## Folder Contract

Each sprite model is one self-contained directory. Asset paths in `model.json` must be non-empty relative paths, cannot start with `/` or `\`, cannot contain a URI scheme, and cannot contain a `..` segment.

Preset discovery scans direct children of `src-tauri/assets/models/`, skips the legacy `standard`, `keyboard`, and `gamepad` folders, and accepts folders whose `model.json` has `"renderer": "sprite"`.

Custom import strictly validates only a parseable manifest whose `renderer` is exactly `sprite`, then copies the entire directory into app data. A missing or misspelled renderer can fall through to the legacy Live2D path and still appear to import successfully. Prevalidate the manifest, verify the stored model renderer after import, and never reference an asset outside the model directory.

`resources/cover.png` is used by the model card but is not currently covered by sprite import validation. Treat it as required. `resources/background.png` is optional. Do not add legacy key-overlay folders to sprite models.

## Model JSON Template

```json
{
  "version": 1,
  "id": "model-id",
  "displayName": "Model Name",
  "renderer": "sprite",
  "mode": "keyboard",
  "canvas": {
    "width": 512,
    "height": 512
  },
  "defaultAnimation": "idle",
  "animations": {
    "idle": {
      "file": "sprites/idle.webp",
      "frameWidth": 512,
      "frameHeight": 512,
      "frames": 6,
      "columns": 3,
      "fps": 8,
      "loop": true,
      "frameDurations": [80, 80, 80, 2400, 80, 80]
    },
    "pluck-01": {
      "file": "sprites/pluck-01.webp",
      "frameWidth": 512,
      "frameHeight": 512,
      "frames": 6,
      "columns": 3,
      "fps": 15,
      "loop": false,
      "frameDurations": [30, 70, 110, 110, 70, 30]
    },
    "transform": {
      "file": "sprites/transform.webp",
      "frameWidth": 512,
      "frameHeight": 512,
      "frames": 16,
      "columns": 4,
      "fps": 16,
      "loop": false,
      "frameDurations": [70, 60, 60, 60, 60, 60, 70, 180, 180, 70, 60, 60, 60, 60, 60, 90]
    }
  },
  "bindings": {
    "keyboard": {
      "KeyQ": "pluck-01",
      "KeyA": "pluck-01",
      "KeyZ": "pluck-01",
      "KeyW": "pluck-01",
      "Return": "transform",
      "Enter": "transform",
      "KpReturn": "transform"
    },
    "mouse": {
      "Left": "pluck-01"
    }
  },
  "bubbles": {
    "enabled": true,
    "duration": 1380,
    "rise": 148,
    "fontSize": 29,
    "maxVisible": 4,
    "anchorX": 256,
    "anchorY": 380,
    "fillTop": "rgba(255, 255, 255, 0.99)",
    "fill": "rgba(229, 251, 255, 0.98)",
    "fillBottom": "rgba(185, 233, 248, 0.97)",
    "highlightColor": "rgba(255, 255, 255, 0.96)",
    "stroke": "rgba(71, 183, 218, 0.92)",
    "strokeWidth": 1.75,
    "textColor": "#17435e",
    "shadowColor": "rgba(38, 128, 166, 0.38)",
    "shadowBlur": 14,
    "shadowOffsetY": 6
  }
}
```

Remove unused animation, mouse, or bubble sections. Do not keep placeholder bindings.

## Field Semantics

### Model

- `renderer` must equal `sprite`.
- `version` is currently ignored by the runtime.
- `id` and `displayName` are optional non-empty strings. Use a stable unique `id` for presets.
- `mode` may be `standard`, `keyboard`, or `gamepad`. Omitted preset mode defaults to `keyboard`; omitted custom-import mode defaults to `standard`. For sprite `gamepad` models, ordinary button names are routed through `bindings.keyboard`; stick axes and `LeftThumb`/`RightThumb` currently drive Live2D parameters only and do not trigger sprite animations. Never author `bindings.gamepad`, because sprite validation ignores it.
- `canvas.width` and `canvas.height` are positive logical model dimensions.
- `defaultAnimation` must name an existing animation.

### Animation

- `file` is a safe relative image path.
- `frameWidth`, `frameHeight`, `frames`, and `columns` are positive integers.
- Frames are read row-major from index zero.
- Required rows equal `ceil(frames / columns)`.
- The sheet must be at least `min(frames, columns) × frameWidth` wide and `ceil(frames / columns) × frameHeight` high. Produce exact dimensions even though runtime validation accepts larger sheets.
- `fps` is always required and must be positive.
- `frameDurations`, when present, overrides `fps` per frame and must contain exactly `frames` positive millisecond values.
- A non-looping action returns to `defaultAnimation` at its end.
- A looping bound action returns to default on key or mouse release.
- The renderer preloads every animation, so avoid unnecessary oversized sheets.

### Bindings

- Prefer `bindings.keyboard` and `bindings.mouse`; legacy top-level aliases are accepted but should not be authored in new models.
- A keyboard binding may name one animation or an array. An array cycles through its animations on repeated presses of that binding key.
- `*` is a keyboard or mouse fallback binding.
- Common keyboard identifiers include `KeyA` through `KeyZ`, `Num0` through `Num9`, `Return`, `Enter`, `KpReturn`, `Space`, `Minus`, and `Equal`.
- Use exact identifiers emitted by the current Rust input layer. Verify unfamiliar keys in `src-tauri/src/core/device.rs` or runtime logs.
- In `gamepad` mode, bind ordinary emitted button names in `bindings.keyboard` and test them with the target controller. Do not claim sprite support for stick-axis or thumb-stick-button actions without changing `useGamepad.ts` and the sprite runtime.
- `Return` and `Enter` alias each other only when no more specific matching entry wins. Bind all of `Return`, `Enter`, and `KpReturn` for consistent Enter behavior.
- An unbound keyboard press may still show a bubble without interrupting the current animation.
- Mouse binding supports exact buttons and `*`, but mouse input does not currently create label bubbles.

### Bubbles

- Each keyboard press creates one bubble; release creates none.
- The OS-provided printable label wins; the renderer formats the physical key identifier as fallback.
- `anchorX` and `anchorY` are logical canvas coordinates for the first-frame cloud tail tip.
- Bubble drawing occurs outside the character mirror transform.
- Omitted fields inherit renderer defaults. Keep the full style block only when the model needs a deliberate palette.
- `duration`, `rise`, `fontSize`, and `strokeWidth` must be positive.
- `anchorX`, `anchorY`, `shadowBlur`, and `shadowOffsetY` must be non-negative; anchors cannot exceed the logical canvas.
- `maxVisible` must be a positive integer.

## Rendering And Cropping

The renderer fits the logical model canvas into the actual canvas with contain scaling and centers it. It preserves aspect ratio, uses high-quality image smoothing, and supports mirror mode. `maxFPS` limits drawing frequency without slowing the animation timeline.

Every frame still needs transparent safety padding. Require zero visible alpha on all four cell edges and test square, portrait, landscape, DPR 1, DPR 2, and mirror mode. Window `borderRadius` plus `overflow-hidden` can crop otherwise valid corner pixels, so validate the actual saved appearance settings as well as the renderer math.

## Cover And References

- Store the model card image at `resources/cover.png`.
- Store the approved canonical source at `references/canonical-base.png`.
- Preserve only raw sources needed to reproduce special effects or poses.
- Runtime ignores `references/`, but generation and repair workflows depend on it.

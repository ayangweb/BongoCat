# Sprite Production And QA

## Canonical-First Rule

Choose one approved transparent RGBA frame as canonical. It owns:

- character position and scale
- face, hair, costume, prop, and body geometry
- all static colors and textures
- the alpha silhouette
- transparent padding

Never average several AI frames into the canonical source. Never independently fit, center, color-match, or scale each timeline frame.

## Image Generation Prompts

Keep prompts concise and attach the canonical/reference images.

Canonical prompt requirements:

```text
Create one polished Q-style desktop-pet character on a transparent background. Preserve the referenced identity, face, hair, costume, palette, and signature prop. Show the complete seated character and complete prop, centered with generous transparent padding. No text, scenery, floor, shadow, extra object, blur, glow, or cropped part.
```

Action donor requirements:

```text
Edit the canonical desktop-pet character into one clear action key pose. Move both hands and only the minimum connected sleeve area needed for the gesture. Preserve face, hair, torso, costume, prop geometry, camera, scale, position, lighting, palette, and transparent background. No motion blur, afterimage, text, detached effect, or extra limb.
```

Transformation reference requirements:

```text
Edit the canonical desktop-pet character into the requested transformed appearance. Preserve pose, position, proportions, face, hands, prop, camera, and silhouette. Change only the requested color treatment and attached or external effect. Transparent background; no scenery, text, blur, crop, or unrelated geometry change.
```

Treat these outputs as donors. Deterministic compositing owns final consistency.

## Idle Recipe

1. Select one canonical open-eye frame.
2. Create one crisp closed-eye donor.
3. Define independent left and right eye masks with a small feather.
4. Build the blink sequence by copying the canonical frame and replacing only pixels inside those masks.
5. Use frame durations to hold the calm state instead of duplicating many near-identical generated frames.

Acceptance:

- each eye changes by a non-zero amount
- outside-eye maximum RGBA delta equals zero
- hands, prop, torso, hair, and alpha geometry equal canonical
- no half-open opacity blend, gray iris, or double eyelid

## Two-Hand Action Recipe

Use a six-frame symmetric layout unless the requested motion needs more frames:

```text
0 canonical
1 intermediate
2 peak
3 peak
4 intermediate
5 canonical
```

Use real pose donors for frames 1 and 2. If a gesture is small, an identical intermediate and peak may be acceptable only when normal-size playback remains smooth.

For every action:

1. Define left-hand and right-hand skin/sleeve corridors as character-specific polygons or masks.
2. Expand the donor/canonical skin and sleeve difference slightly, feather the edge by about 1–2 pixels, then clip it to the correct corridor.
3. Erase the canonical hand only within that same local corridor and composite the donor patch.
4. Keep the instrument and all protected regions canonical unless a minimal occlusion repair is unavoidable.
5. Compose both sides from immutable single-hand or native two-hand donors. Never use a previously composed output as a new donor.

Do not crop companion hands at `canvasWidth / 2`. A hand or sleeve may cross the centerline. Measure independent left/right action corridors and require motion in each exclusive core.

Acceptance:

- first and last frames equal canonical exactly
- frame 1 equals frame 4 and frame 2 equals frame 3
- at least two distinct active poses exist for a visible gesture
- both exclusive hand cores change in every active frame
- action-corridor exterior delta equals zero
- protected face delta equals zero
- no duplicate hand, broken finger, ghost sleeve, seam, or prop texture jump

## Transformation Recipe

Prefer a 16-frame symmetric envelope:

```text
progress = [0, .055, .198, .394, .606, .802, .945, 1,
            1, .945, .802, .606, .394, .198, .055, 0]
```

Interpolate canonical colors toward one deterministic transformed target. Apply the same transformation to the same source RGB values within each frame. Fade approved external effects with the same or a separately specified symmetric envelope.

Suggested durations:

```text
[70, 60, 60, 60, 60, 60, 70, 180,
 180, 70, 60, 60, 60, 60, 60, 90]
```

Acceptance:

- frame 0 and final frame equal canonical exactly
- all symmetric frame pairs equal exactly
- two peak frames equal exactly
- character alpha equals canonical in every frame
- whitening or other scalar effect is monotonic into and out of the peak
- external effects never touch the cell edge
- no isolated fragments, residual fade patches, or geometry drift

## Lossless Sheet Assembly

Use Pillow or another deterministic RGBA pipeline. For a sheet with `frames`, `columns`, `frameWidth`, and `frameHeight`:

```text
rows = ceil(frames / columns)
sheetWidth = columns * frameWidth
sheetHeight = rows * frameHeight
cellX = (index % columns) * frameWidth
cellY = floor(index / columns) * frameHeight
```

Write fully transparent unused cells. Clear RGB to zero wherever alpha is zero. Save WebP losslessly with exact transparent RGB preservation when the encoder supports it.

Do not resize the composed sheet. Resize or align sources once before frame assembly and use one shared transform for the entire animation family.

## Repository Tools

`scripts/validate_sprite_sheet.py` provides a preliminary per-sheet check and creates a contact sheet plus GIF:

```bash
"$PYTHON" scripts/validate_sprite_sheet.py \
  --sheet "$SHEET" \
  --frames "$FRAMES" \
  --columns "$COLUMNS" \
  --report "$QA_DIR/report.json" \
  --contact-sheet "$QA_DIR/contact.png" \
  --preview "$QA_DIR/preview.gif"
```

The script infers cell dimensions from the sheet instead of accepting configured `frameWidth` and `frameHeight`. Before running it, independently require exact width `columns × frameWidth`, exact height `ceil(frames / columns) × frameHeight`, RGBA decoding, and cleared hidden RGB. Its GIF uses a fixed preview duration, so also produce a preview using the actual `frameDurations`. Do not use this script alone as the configuration-grid gate.

`scripts/stabilize_sprite_sheet.py` is a proven design reference but is not generic. Its eye boxes, `pluck-*` name checks, action polygons, donor graph, protected face region, two-hand corridors, color targets, effect extraction, and numeric thresholds are character-specific. With `--two-hand`, it currently selects the raw idle frame 0 as canonical instead of the stabilized canonical. Its companion and donor composition also truncates patches at `width // 2`. Replace both behaviors before using it for a new model, and adapt its checks if action names do not start with `pluck-`.

## Quantitative QA

At minimum, record these values per animation:

- decoded sheet size and expected size
- frame alpha bounding boxes and margins
- edge alpha pixel count
- hidden RGB maximum where alpha is zero
- first/last canonical maximum delta
- symmetric-pair maximum delta
- protected-region maximum delta
- static-region RGBA maximum delta and MAE
- changed-pixel count inside each intended action core
- full-frame luminance and warm/cool drift
- alpha centroid spread
- unique frame count and active pose count

Use zero as the target for exact invariants. Treat aggregate full-character color drift carefully: intended hand or effect changes may move a global mean slightly, but any change in a declared static region is a failure.

## Visual QA

For each animation create:

- labeled contact sheet on a checkerboard
- real-duration GIF
- motion-difference image against canonical
- optionally a side-by-side original/stabilized GIF

Inspect every artifact independently. Reject:

- texture crawling or water-like waves
- global hue or exposure flashes
- outline breathing, scale popping, or centroid jitter
- half-opacity double hands or eyes
- abrupt large-pose jumps without an intermediate
- broken fingers, duplicate sleeves, or patch seams
- face, hair, torso, instrument, or costume contamination
- effects that fragment, touch edges, or leave fade residue
- final-to-idle flash cuts

Do not accept an animation only because its JSON report says `ok: true`.

## Full Model And Runtime QA

Validate configuration through the application's `sprite.validateModel()` path. Then load the model in the actual app and test:

1. Idle for at least two complete cycles.
2. Every animation once, then rapid alternating actions.
3. Four simultaneous or repeated bubble labels.
4. Return, Enter, and keypad Enter.
5. Window scale, portrait and landscape aspect ratios, DPR 1 and 2, and mirror mode.
6. The user's saved opacity, scale, and corner radius.

For macOS global keys, confirm Input Monitoring for the exact built `.app`. Ad-hoc rebuilds can change the code-directory hash and invalidate an older authorization even when the bundle path and identifier stay the same.

After packaging, compare SHA-256 of source and bundled `model.json`, cover, and every sprite sheet. Verify the running process executable is inside the new bundle rather than an older build or installed copy.

## Reproducibility

- Keep raw references immutable.
- Write generated and stabilized output to a new directory.
- Never create cyclic donor dependencies.
- Make preservation guards test fixed semantic regions, not self-derived motion unions.
- Reject a supposedly two-hand sequence when only one hand corridor changes.
- Rerun the pipeline on its declared raw inputs and compare decoded frame hashes before calling it reproducible.

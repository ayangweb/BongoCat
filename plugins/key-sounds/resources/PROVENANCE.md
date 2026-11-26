# Key-sounds assets

Every `.wav` under `resources/packs/` is a copy of one recording published in
Thock's soundpack registry
([`kamillobinski/thock-soundpacks`](https://github.com/kamillobinski/thock-soundpacks)),
committed here so the plugin never has to be online to play a sound.

- Registry manifest: <https://raw.githubusercontent.com/kamillobinski/thock-soundpacks/refs/heads/main/manifest.json>
- Registry version `1.0.0`, last updated `2026-03-28T12:17:11Z`
- Frozen registry snapshot used by the tests: `registry-manifest.json`
- Recipe: `fetch_packs.py` — re-run it to reproduce every byte
- Per-file SHA-256, license and source URL: `index.json`

## Catalog and embedded audio

`index.json` lists the 25 redistributable sets in the registry (23 keyboard, 2
mouse), in the same brand, author and name order Thock's menu uses. Of each set
it can carry, the plugin embeds only the groups it can address:

- the keyboard `default`, `space`, `enter`, `del`, `tab`, `shiftLeft`,
  `shiftRight` and `ctrlLeft` groups, each split into its `down` and `up`
  recordings;
- the mouse `left` and `right` groups, likewise split.

These are the physical keys whose identity crosses the bounded plugin activity
contract (ADR-0093). Thock maps Backspace to `del`, so its unreachable
`backspace` group is not carried; the right Control key is likewise absent from
Thock's `KeyMapper`. Letter and symbol groups are deliberately dropped because
no product surface can address them. When a pack does not carry one of these
groups, playback falls back to `default`; when the group exists but has no
recording for a direction, that direction is silent, matching Thock.

## Licenses

Only sets whose license permits redistribution are carried. The generator
refuses anything else rather than reviewing it case by case:

- **MIT** — 22 keyboard sets from `tplai/kbsim`, `hainguyents13/mechvibes` and
  `webdevcody/type-joy`. Redistribution with attribution is permitted; the
  author and license URL of each set are recorded in `index.json`.
- **CC0** — the `Orochi V2` mouse set, dedicated to the public domain.
- **Pixabay Content License** — the `Quack` keyboard set and the second mouse
  set. The license grants free commercial and non-commercial use of the content
  as part of a larger work; what it forbids is redistributing content on a
  standalone basis, which is not what happens here — the recordings are
  embedded in the plugin executable to play a sound, and the plugin neither
  offers them as downloads nor exposes them as files.

One registry entry is refused: `Mamba Elite` (Razer, `kamillobinski`), licensed
`Proprietary`. Its license permits use only in the Thock application
(`dev.kamillobinski.Thock`), so none of its bytes are downloaded, written,
embedded or listed in the plugin. The script prints every refused entry each
time it runs.

## Reproducing

```text
python3 plugins/key-sounds/resources/fetch_packs.py
```

The script downloads the registry manifest and every redistributable pack, writes
the kept recordings under `resources/packs/<category>/<set-id>/`, and
regenerates both `resources/index.json` and `src/packs.rs`.
`tools/tests/test_key_sounds_assets.py` compares the catalog with the frozen
registry snapshot, checks that the files and the generated table agree, verifies
that no proprietary set is carried and that no set claims the reserved `auto`
id.

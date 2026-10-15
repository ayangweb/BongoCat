# Typing-sound assets

Every `.wav` under `resources/packs/` is a copy of one recording published in
Thock's soundpack registry
([`kamillobinski/thock-soundpacks`](https://github.com/kamillobinski/thock-soundpacks)),
committed here so the plugin never has to be online to play a sound.

- Registry manifest: <https://raw.githubusercontent.com/kamillobinski/thock-soundpacks/refs/heads/main/manifest.json>
- Registry version `1.0.0`, last updated `2026-03-28T12:17:11Z`
- Recipe: `fetch_packs.py` — re-run it to reproduce every byte
- Per-file SHA-256, license and source URL: `index.json`

## What is carried

`index.json` is the authoritative list. The registry publishes 26 sets (23
keyboard, 3 mouse); this plugin carries 25 of them, and of each set it keeps
only the groups it can address:

- the keyboard `default` group, split into its `down` and `up` recordings;
- the mouse `left` and `right` groups, likewise split.

A pack's per-key recordings (space, enter, one file per letter) are deliberately
dropped. The host hands a plugin anonymous device activity and no key identity
(ADR-0091), so no code path could ever address them; keeping them would multiply
the size of every set for sounds nothing can play.

## Licenses

Only sets whose license permits redistribution are carried. `fetch_packs.py`
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
`Proprietary`. Thock's own manifest restricts it to the Thock application, so it
is not redistributable here and is not carried. The script prints every refused
entry each time it runs.

## Reproducing

```text
python3 plugins/typing-sound/resources/fetch_packs.py
```

The script downloads the registry manifest and each pack, writes the kept
recordings under `resources/packs/<category>/<set-id>/`, and regenerates both
`resources/index.json` and `src/packs.rs`. `tools/tests/test_typing_sound_assets.py`
checks that the three stay in agreement, that no proprietary set is carried and
that no set claims the reserved `auto` id.

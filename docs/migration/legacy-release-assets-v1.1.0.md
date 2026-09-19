# Legacy Release Asset Baseline: v1.1.0

This record freezes the public legacy release metadata used by the Native
Rewrite migration. It is historical evidence only; these assets are not Native
Rewrite inputs, dependencies, or release candidates.

## Source

- Repository: `https://github.com/ayangweb/BongoCat`
- Tag: `v1.1.0`
- Target commitish: `master`
- Published: `2026-04-20T01:08:51Z`
- Observed: `2026-09-07` via the GitHub Releases API
- API endpoint: `https://api.github.com/repos/ayangweb/BongoCat/releases/tags/v1.1.0`

GitHub's release API supplied the SHA-256 values below. The assets were not
downloaded or placed in the Native workspace.

## Windows and macOS assets

| Asset                            |     Bytes | GitHub SHA-256                                                     |
| -------------------------------- | --------: | ------------------------------------------------------------------ |
| `BongoCat_1.1.0_x64-setup.exe`   | 5,397,114 | `c83f2963cb38056273aa98731704c8da650a4bb5bebe4262b4887ba2db76a935` |
| `BongoCat_1.1.0_arm64-setup.exe` | 5,095,820 | `19e717dd866bab18097ec4fe43c6ea9f9e7a6c873260a4be5c9d8fdb8c67f312` |
| `BongoCat_1.1.0_x86-setup.exe`   | 5,178,860 | `cf892a92cf8be8efc5bf4d3bf9057b3091e608bf0118c0c410411eb04f0728dc` |
| `BongoCat_1.1.0_x64.dmg`         | 9,131,680 | `7264690b9f33606ce960f274236acee111ce34ff061886ae5c2d5a154c9b4b77` |
| `BongoCat_1.1.0_aarch64.dmg`     | 8,776,671 | `ca6a890b9c1754b8f828627f2d6864b177d5cb0efca1342f7c7da88dcaf1e94e` |
| `BongoCat_x64.app.tar.gz`        | 8,924,738 | `272c922b41394a87b57eb931d7398bce6b96ea35c8ebd6f29b0dbd0be66bb313` |
| `BongoCat_aarch64.app.tar.gz`    | 8,615,767 | `7938b320b16caf1feeea497ab112a541a516774abe54f5d5449bcead90b96710` |

The release also contains detached `.sig` files and Linux RPM, AppImage, and
Debian assets. They remain part of the historical release inventory but are not
included in the Native Rewrite target matrix. In particular, the legacy x86
installer is evidence of the old product only and does not authorize an
`i686-pc-windows-msvc` Native build.

## Native boundary

The Native Rewrite keeps its own Bundle ID, schema, storage roots, signing
keys, and artifact provenance. Legacy release signatures, updater metadata, and
the legacy `com.ayangweb.BongoCat` bundle identity must not be reused. Native
Windows ARM64 remains release-blocked until an authorized desktop Cubism Core
passes the ABI and model gates; the historical ARM64 installer does not change
that decision.

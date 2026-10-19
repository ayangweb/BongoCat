"""Keep every bundled plugin icon on one asset and rendering contract.

The plugin center draws each local icon into the same square frame. Its
manifest therefore names one package-relative asset instead of letting each
plugin choose a directory or file name, and the asset itself is a square RGBA
PNG whose rounded silhouette reaches every edge of the image. This test catches
a new plugin that drifts from that shared contract before packaging hides the
difference.
"""

import json
import struct
import unittest
import zlib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PLUGINS = ROOT / "plugins"
EXPECTED_ICON = "resources/icon.png"


def read_png_rgba(path):
    """Decode the subset of PNG produced by the checked-in plugin icons."""
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path} is not a PNG")

    offset = 8
    width = height = color_type = None
    idat = bytearray()
    while offset < len(data):
        length = struct.unpack(">I", data[offset : offset + 4])[0]
        kind = data[offset + 4 : offset + 8]
        payload = data[offset + 8 : offset + 8 + length]
        offset += 12 + length
        if kind == b"IHDR":
            width, height, depth, color_type = struct.unpack(">IIBB", payload[:10])
            if depth != 8 or color_type != 6:
                raise ValueError(f"{path} must be 8-bit RGBA")
        elif kind == b"IDAT":
            idat.extend(payload)
        elif kind == b"IEND":
            break

    if width is None or height is None:
        raise ValueError(f"{path} has no IHDR")
    raw = zlib.decompress(idat)
    stride = width * 4
    previous = bytearray(stride)
    rows = []
    for y in range(height):
        filter_type = raw[y * (stride + 1)]
        row = bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
        for index in range(stride):
            left = row[index - 4] if index >= 4 else 0
            above = previous[index]
            upper_left = previous[index - 4] if index >= 4 else 0
            if filter_type == 1:
                row[index] = (row[index] + left) & 0xFF
            elif filter_type == 2:
                row[index] = (row[index] + above) & 0xFF
            elif filter_type == 3:
                row[index] = (row[index] + ((left + above) // 2)) & 0xFF
            elif filter_type == 4:
                estimate = left + above - upper_left
                pa, pb, pc = (
                    abs(estimate - left),
                    abs(estimate - above),
                    abs(estimate - upper_left),
                )
                predictor = left if pa <= pb and pa <= pc else above if pb <= pc else upper_left
                row[index] = (row[index] + predictor) & 0xFF
            elif filter_type != 0:
                raise ValueError(f"{path} uses an unsupported PNG filter")
        rows.append(row)
        previous = row
    return width, height, rows


class PluginIconTests(unittest.TestCase):
    def plugin_manifests(self):
        return sorted(PLUGINS.glob("*/plugin.json"))

    def test_every_plugin_uses_the_same_local_icon_entry(self):
        self.assertTrue(self.plugin_manifests(), "no plugin manifests found")
        for manifest_path in self.plugin_manifests():
            with self.subTest(plugin=manifest_path.parent.name):
                metadata = json.loads(manifest_path.read_text(encoding="utf-8"))["metadata"]
                self.assertEqual(metadata["icon"]["type"], "local_image")
                self.assertEqual(metadata["icon"]["value"], EXPECTED_ICON)
                self.assertTrue((manifest_path.parent / EXPECTED_ICON).is_file())

    def test_plugins_do_not_ship_a_second_icon_directory(self):
        self.assertEqual(
            [
                path
                for path in PLUGINS.glob("*/resources/icons/*")
                if path.is_file()
            ],
            [],
            "plugin icons must live at resources/icon.png",
        )

    def test_every_icon_is_a_square_with_antialiased_rounded_corners(self):
        for manifest_path in self.plugin_manifests():
            icon = manifest_path.parent / EXPECTED_ICON
            with self.subTest(plugin=manifest_path.parent.name):
                width, height, rows = read_png_rgba(icon)
                self.assertEqual(width, height)
                alpha = {
                    (x, y): rows[y][x * 4 + 3]
                    for y in range(height)
                    for x in range(width)
                }
                partial = sum(0 < value < 255 for value in alpha.values())
                self.assertGreater(
                    partial,
                    300,
                    f"{icon} has no antialiased rounded edge",
                )
                # A transparent margin becomes a visible checkerboard or
                # light frame inside the card's fixed square.
                for x, y in (
                    (width // 2, 0),
                    (width // 2, height - 1),
                    (0, height // 2),
                    (width - 1, height // 2),
                ):
                    self.assertEqual(
                        alpha[(x, y)],
                        255,
                        f"{icon} has transparent padding at {(x, y)}",
                    )
                self.assertEqual(alpha[(0, 0)], 0)
                self.assertEqual(alpha[(width - 1, 0)], 0)
                self.assertEqual(alpha[(0, height - 1)], 0)
                self.assertEqual(alpha[(width - 1, height - 1)], 0)
                self.assertEqual(alpha[(width // 2, height // 2)], 255)


if __name__ == "__main__":
    unittest.main()

import argparse
import math
import re

from PIL import Image


def parse_color(value: str):
    if not re.fullmatch(r'#[0-9a-fA-F]{6}', value):
        raise ValueError('invalid chroma key')
    return tuple(int(value[index:index + 2], 16) for index in (1, 3, 5))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--input', required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--chroma-key', default='#FF00FF')
    parser.add_argument('--threshold', type=float, default=96)
    args = parser.parse_args()

    key = parse_color(args.chroma_key)
    image = Image.open(args.input).convert('RGBA')
    pixels = []

    for red, green, blue, alpha in image.getdata():
        distance = math.sqrt(
            (red - key[0]) ** 2
            + (green - key[1]) ** 2
            + (blue - key[2]) ** 2
        )
        pixels.append((0, 0, 0, 0) if distance <= args.threshold else (red, green, blue, alpha))

    image.putdata(pixels)
    image.save(args.output)


if __name__ == '__main__':
    main()

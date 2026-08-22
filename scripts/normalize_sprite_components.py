import argparse
import json
from collections import deque
from pathlib import Path

import numpy as np
from PIL import Image


def nearest_seed(foreground, labels, center_x, center_y, bounds):
    left, top, right, bottom = bounds
    ys, xs = np.nonzero(foreground[top:bottom, left:right] & (labels[top:bottom, left:right] == 0))

    if len(xs) == 0:
        raise ValueError('no unassigned sprite pixels near expected pose center')

    xs = xs + left
    ys = ys + top
    index = np.argmin((xs - center_x) ** 2 + (ys - center_y) ** 2)
    return int(xs[index]), int(ys[index])


def flood(foreground, labels, seeds):
    height, width = foreground.shape
    queue = deque()
    counts = [0] * len(seeds)

    for label, (seed_x, seed_y) in enumerate(seeds, start=1):
        labels[seed_y, seed_x] = label
        queue.append((seed_x, seed_y, label))

    while queue:
        x, y, label = queue.popleft()
        counts[label - 1] += 1

        for next_x, next_y in (
            (x - 1, y - 1), (x, y - 1), (x + 1, y - 1),
            (x - 1, y), (x + 1, y),
            (x - 1, y + 1), (x, y + 1), (x + 1, y + 1),
        ):
            if next_x < 0 or next_y < 0 or next_x >= width or next_y >= height:
                continue
            if labels[next_y, next_x] or not foreground[next_y, next_x]:
                continue
            labels[next_y, next_x] = label
            queue.append((next_x, next_y, label))

    return counts


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--input', required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--columns', type=int, required=True)
    parser.add_argument('--rows', type=int, required=True)
    parser.add_argument('--cell-width', type=int, required=True)
    parser.add_argument('--cell-height', type=int, required=True)
    parser.add_argument('--padding', type=int, default=48)
    parser.add_argument('--report', required=True)
    args = parser.parse_args()

    source_image = Image.open(args.input).convert('RGBA')
    source = np.array(source_image)
    foreground = source[:, :, 3] > 0
    labels = np.zeros(foreground.shape, dtype=np.uint8)
    source_slot_width = source_image.width / args.columns
    source_slot_height = source_image.height / args.rows
    seeds = []

    for row in range(args.rows):
        for column in range(args.columns):
            label = row * args.columns + column + 1
            center_x = round((column + 0.5) * source_slot_width)
            center_y = round((row + 0.5) * source_slot_height)
            bounds = (
                round(column * source_slot_width),
                round(row * source_slot_height),
                round((column + 1) * source_slot_width),
                round((row + 1) * source_slot_height),
            )
            seed_x, seed_y = nearest_seed(foreground, labels, center_x, center_y, bounds)
            labels[seed_y, seed_x] = label
            seeds.append((seed_x, seed_y))

    labels.fill(0)
    component_sizes = flood(foreground, labels, seeds)

    main_centers = []
    for label in range(1, args.columns * args.rows + 1):
        ys, xs = np.nonzero(labels == label)
        if len(xs) < 1000:
            raise ValueError(f'pose component {label - 1} is too small')
        main_centers.append((float(xs.mean()), float(ys.mean())))

    unassigned_y, unassigned_x = np.nonzero(foreground & (labels == 0))
    if len(unassigned_x):
        centers = np.array(main_centers)
        distances = (
            (unassigned_x[:, None] - centers[None, :, 0]) ** 2
            + (unassigned_y[:, None] - centers[None, :, 1]) ** 2
        )
        labels[unassigned_y, unassigned_x] = np.argmin(distances, axis=1) + 1

    crops = []
    source_boxes = []
    max_width = 0
    max_height = 0

    for label in range(1, args.columns * args.rows + 1):
        ys, xs = np.nonzero(labels == label)
        left, top = int(xs.min()), int(ys.min())
        right, bottom = int(xs.max()) + 1, int(ys.max()) + 1
        crop = source[top:bottom, left:right].copy()
        crop[labels[top:bottom, left:right] != label] = 0
        crops.append(Image.fromarray(crop, 'RGBA'))
        source_boxes.append([left, top, right, bottom])
        max_width = max(max_width, right - left)
        max_height = max(max_height, bottom - top)

    scale = min(
        (args.cell_width - args.padding * 2) / max_width,
        (args.cell_height - args.padding * 2) / max_height,
        1,
    )
    output = Image.new(
        'RGBA',
        (args.cell_width * args.columns, args.cell_height * args.rows),
        (0, 0, 0, 0),
    )
    frames = []

    for index, crop in enumerate(crops):
        width = max(1, round(crop.width * scale))
        height = max(1, round(crop.height * scale))
        resized = crop.resize((width, height), Image.Resampling.LANCZOS)
        left = index % args.columns * args.cell_width + (args.cell_width - width) // 2
        top = index // args.columns * args.cell_height + args.cell_height - args.padding - height
        output.alpha_composite(resized, (left, top))
        frames.append({
            'index': index,
            'sourceBox': source_boxes[index],
            'outputBox': [left, top, left + width, top + height],
        })

    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output.save(output_path)
    report = {
        'ok': True,
        'input': str(Path(args.input).resolve()),
        'output': str(output_path.resolve()),
        'scale': scale,
        'padding': args.padding,
        'componentSizes': component_sizes,
        'reassignedPixels': int(len(unassigned_x)),
        'frames': frames,
    }
    report_path = Path(args.report)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()

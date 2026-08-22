import argparse
import json
import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


def alpha_bbox(image: Image.Image):
    return image.getchannel('A').getbbox()


def edge_pixels(image: Image.Image, inset: int = 2):
    alpha = image.getchannel('A')
    width, height = image.size
    bands = [
        alpha.crop((0, 0, width, inset)),
        alpha.crop((0, height - inset, width, height)),
        alpha.crop((0, 0, inset, height)),
        alpha.crop((width - inset, 0, width, height)),
    ]
    return sum(sum(1 for value in band.getdata() if value > 0) for band in bands)


def checkerboard(size, block=16):
    image = Image.new('RGBA', size, (34, 39, 49, 255))
    draw = ImageDraw.Draw(image)
    for y in range(0, size[1], block):
        for x in range(0, size[0], block):
            if (x // block + y // block) % 2:
                draw.rectangle((x, y, x + block - 1, y + block - 1), fill=(53, 60, 74, 255))
    return image


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--sheet', required=True)
    parser.add_argument('--frames', required=True, type=int)
    parser.add_argument('--columns', required=True, type=int)
    parser.add_argument('--report', required=True)
    parser.add_argument('--contact-sheet', required=True)
    parser.add_argument('--preview', required=True)
    parser.add_argument('--webp-output')
    args = parser.parse_args()

    source = Image.open(args.sheet).convert('RGBA')
    rows = math.ceil(args.frames / args.columns)
    errors = []
    warnings = []

    if source.width % args.columns or source.height % rows:
        errors.append('sheet dimensions are not divisible by the configured grid')

    frame_width = source.width // args.columns
    frame_height = source.height // rows
    frames = []
    stats = []

    for index in range(args.columns * rows):
        column = index % args.columns
        row = index // args.columns
        frame = source.crop((
            column * frame_width,
            row * frame_height,
            (column + 1) * frame_width,
            (row + 1) * frame_height,
        ))
        bbox = alpha_bbox(frame)

        if index >= args.frames:
            if bbox:
                errors.append(f'unused cell {index} is not transparent')
            continue

        if not bbox:
            errors.append(f'frame {index} is empty')
            stats.append({'index': index, 'bbox': None})
            frames.append(frame)
            continue

        opaque_count = sum(1 for value in frame.getchannel('A').getdata() if value > 0)
        coverage = opaque_count / (frame_width * frame_height)
        edge_count = edge_pixels(frame)

        if coverage < 0.08:
            errors.append(f'frame {index} content is too small')
        if coverage > 0.9:
            warnings.append(f'frame {index} fills most of the cell')
        if edge_count:
            errors.append(f'frame {index} touches a cell edge')

        stats.append({
            'index': index,
            'bbox': list(bbox),
            'coverage': round(coverage, 4),
            'edgePixels': edge_count,
        })
        frames.append(frame)

    widths = [item['bbox'][2] - item['bbox'][0] for item in stats if item['bbox']]
    heights = [item['bbox'][3] - item['bbox'][1] for item in stats if item['bbox']]
    if widths and min(widths) / max(widths) < 0.7:
        warnings.append('frame silhouette width varies by more than 30%')
    if heights and min(heights) / max(heights) < 0.8:
        warnings.append('frame silhouette height varies by more than 20%')

    report = {
        'ok': not errors,
        'sheet': str(Path(args.sheet).resolve()),
        'width': source.width,
        'height': source.height,
        'frameWidth': frame_width,
        'frameHeight': frame_height,
        'frames': args.frames,
        'columns': args.columns,
        'rows': rows,
        'errors': errors,
        'warnings': warnings,
        'frameStats': stats,
    }

    report_path = Path(args.report)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')

    scale = min(1, 320 / frame_width)
    preview_size = (round(frame_width * scale), round(frame_height * scale))
    label_height = 28
    contact = Image.new('RGBA', (preview_size[0] * args.columns, (preview_size[1] + label_height) * rows), (0, 0, 0, 255))
    draw = ImageDraw.Draw(contact)
    font = ImageFont.load_default()

    preview_frames = []
    for index, frame in enumerate(frames):
        background = checkerboard(frame.size)
        background.alpha_composite(frame)
        preview_frames.append(background)
        thumb = background.resize(preview_size, Image.Resampling.LANCZOS)
        column = index % args.columns
        row = index // args.columns
        x = column * preview_size[0]
        y = row * (preview_size[1] + label_height)
        contact.alpha_composite(thumb, (x, y))
        draw.text((x + 8, y + preview_size[1] + 7), f'frame {index}', fill=(255, 255, 255, 255), font=font)

    contact_path = Path(args.contact_sheet)
    contact_path.parent.mkdir(parents=True, exist_ok=True)
    contact.save(contact_path)

    preview_path = Path(args.preview)
    preview_path.parent.mkdir(parents=True, exist_ok=True)
    if preview_frames:
        preview_frames[0].save(
            preview_path,
            save_all=True,
            append_images=preview_frames[1:],
            duration=180,
            loop=0,
            disposal=2,
        )

    if args.webp_output:
        output_path = Path(args.webp_output)
        output_path.parent.mkdir(parents=True, exist_ok=True)
        source.save(output_path, format='WEBP', lossless=True, method=6)

    print(json.dumps(report, ensure_ascii=False))
    raise SystemExit(0 if report['ok'] else 1)


if __name__ == '__main__':
    main()

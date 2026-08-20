import argparse
import json
import math
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont


def split_frames(sheet, frame_width, frame_height, frames, columns):
    return [
        np.array(
            sheet.crop(
                (
                    index % columns * frame_width,
                    index // columns * frame_height,
                    (index % columns + 1) * frame_width,
                    (index // columns + 1) * frame_height,
                )
            ).convert('RGBA'),
            dtype=np.uint8,
        )
        for index in range(frames)
    ]


def compose_sheet(frames, columns):
    height, width = frames[0].shape[:2]
    rows = math.ceil(len(frames) / columns)
    sheet = Image.new('RGBA', (width * columns, height * rows), (0, 0, 0, 0))
    for index, frame in enumerate(frames):
        sheet.alpha_composite(Image.fromarray(frame, 'RGBA'), ((index % columns) * width, (index // columns) * height))
    return sheet


def shift_array(image, dx, dy):
    height, width = image.shape[:2]
    output = np.zeros_like(image)
    source_x0 = max(0, -dx)
    source_y0 = max(0, -dy)
    source_x1 = min(width, width - dx)
    source_y1 = min(height, height - dy)
    target_x0 = source_x0 + dx
    target_y0 = source_y0 + dy
    target_x1 = source_x1 + dx
    target_y1 = source_y1 + dy
    if source_x1 > source_x0 and source_y1 > source_y0:
        output[target_y0:target_y1, target_x0:target_x1] = image[source_y0:source_y1, source_x0:source_x1]
    return output


def registration_feature(frame):
    alpha = Image.fromarray(frame[:, :, 3], 'L').filter(ImageFilter.GaussianBlur(3))
    rgb = Image.fromarray(frame[:, :, :3], 'RGB').convert('L').filter(ImageFilter.GaussianBlur(4))
    alpha_array = np.asarray(alpha, dtype=np.float32) / 255
    gray_array = np.asarray(rgb, dtype=np.float32) / 255
    feature = alpha_array * (0.72 + 0.28 * gray_array)
    feature -= feature.mean()
    return feature


def phase_translation(reference, frame, max_shift):
    reference_feature = registration_feature(reference)
    frame_feature = registration_feature(frame)
    cross_power = np.fft.fft2(reference_feature) * np.conj(np.fft.fft2(frame_feature))
    cross_power /= np.maximum(np.abs(cross_power), 1e-8)
    correlation = np.abs(np.fft.ifft2(cross_power))
    peak_y, peak_x = np.unravel_index(np.argmax(correlation), correlation.shape)
    if peak_x > correlation.shape[1] // 2:
        peak_x -= correlation.shape[1]
    if peak_y > correlation.shape[0] // 2:
        peak_y -= correlation.shape[0]
    coarse_dx = int(np.clip(peak_x, -max_shift, max_shift))
    coarse_dy = int(np.clip(peak_y, -max_shift, max_shift))
    best = (float('inf'), coarse_dx, coarse_dy)
    reference_alpha = reference[:, :, 3].astype(np.float32) / 255
    reference_gray = reference_feature
    for dy in range(max(-max_shift, coarse_dy - 2), min(max_shift, coarse_dy + 2) + 1):
        for dx in range(max(-max_shift, coarse_dx - 2), min(max_shift, coarse_dx + 2) + 1):
            shifted_alpha = shift_array(frame[:, :, 3], dx, dy).astype(np.float32) / 255
            overlap = (reference_alpha > 0.05) | (shifted_alpha > 0.05)
            if not np.any(overlap):
                continue
            shifted_feature = shift_array(frame_feature, dx, dy)
            alpha_cost = np.mean(np.abs(reference_alpha[overlap] - shifted_alpha[overlap]))
            feature_cost = np.mean(np.abs(reference_gray[overlap] - shifted_feature[overlap]))
            cost = alpha_cost * 0.75 + feature_cost * 0.25
            if cost < best[0]:
                best = (float(cost), dx, dy)
    return best[1], best[2], best[0]


def match_color(reference, frame):
    overlap = (reference[:, :, 3] > 160) & (frame[:, :, 3] > 160)
    output = frame.copy()
    gains = []
    biases = []
    if np.count_nonzero(overlap) < 1024:
        return output, [1, 1, 1], [0, 0, 0]
    for channel in range(3):
        source_values = frame[:, :, channel][overlap].astype(np.float32)
        target_values = reference[:, :, channel][overlap].astype(np.float32)
        source_low, source_high = np.percentile(source_values, [10, 90])
        target_low, target_high = np.percentile(target_values, [10, 90])
        if source_high - source_low < 8:
            gain = 1
        else:
            gain = np.clip((target_high - target_low) / (source_high - source_low), 0.9, 1.1)
        bias = np.clip(np.median(target_values - source_values * gain), -16, 16)
        corrected = np.clip(frame[:, :, channel].astype(np.float32) * gain + bias, 0, 255)
        output[:, :, channel] = corrected.astype(np.uint8)
        gains.append(round(float(gain), 5))
        biases.append(round(float(bias), 4))
    output[output[:, :, 3] == 0, :3] = 0
    return output, gains, biases


def blurred_rgba(frame, radius):
    image = Image.fromarray(frame, 'RGBA')
    rgb = np.asarray(image.convert('RGB').filter(ImageFilter.GaussianBlur(radius)), dtype=np.float32)
    alpha = np.asarray(image.getchannel('A').filter(ImageFilter.GaussianBlur(radius)), dtype=np.float32)
    return rgb, alpha


def motion_mask(reference, frames):
    reference_rgb, reference_alpha = blurred_rgba(reference, 2.2)
    scores = []
    for frame in frames:
        rgb, alpha = blurred_rgba(frame, 2.2)
        foreground = np.maximum(reference_alpha, alpha) / 255
        color_diff = np.mean(np.abs(reference_rgb - rgb), axis=2) * foreground
        alpha_diff = np.abs(reference_alpha - alpha) * 0.55
        scores.append(color_diff + alpha_diff)
    score = np.max(np.stack(scores), axis=0)
    foreground = np.max(np.stack([frame[:, :, 3] for frame in [reference, *frames]]), axis=0) > 12
    values = score[foreground]
    if not len(values):
        return np.ones(reference.shape[:2], dtype=np.float32), 0
    median = float(np.median(values))
    mad = float(np.median(np.abs(values - median)))
    threshold = max(9, min(28, median + max(5, mad * 3.2)))
    hard = Image.fromarray(np.where(score >= threshold, 255, 0).astype(np.uint8), 'L')
    hard = hard.filter(ImageFilter.MaxFilter(25)).filter(ImageFilter.GaussianBlur(5))
    mask = np.asarray(hard, dtype=np.float32) / 255
    mask[~foreground] = np.maximum(mask[~foreground], 0)
    return mask, threshold


def composite_stable(reference, frame, moving):
    weight = moving[:, :, None].astype(np.float32)
    output = np.rint(reference.astype(np.float32) * (1 - weight) + frame.astype(np.float32) * weight)
    output = np.clip(output, 0, 255).astype(np.uint8)
    output[output[:, :, 3] == 0, :3] = 0
    return output


def ellipse_mask(size, boxes, feather):
    mask = Image.new('L', size, 0)
    draw = ImageDraw.Draw(mask)
    for box in boxes:
        draw.ellipse(tuple(box), fill=255)
    if feather:
        mask = mask.filter(ImageFilter.GaussianBlur(feather))
    return np.asarray(mask, dtype=np.float32) / 255


def stabilize_idle(frames, reference_index, open_index, eye_boxes):
    reference = frames[reference_index].copy()
    open_frame = frames[open_index].copy()
    eye_mask = ellipse_mask((reference.shape[1], reference.shape[0]), eye_boxes, 2.2)
    open_eye = composite_stable(reference, open_frame, eye_mask)
    outputs = [
        open_eye.copy(),
        open_eye.copy(),
        reference.copy(),
        reference.copy(),
        reference.copy(),
        open_eye.copy(),
    ]
    outside = eye_mask < 0.001
    outside_delta = max(
        int(np.max(np.abs(output[outside].astype(np.int16) - reference[outside].astype(np.int16))))
        for output in outputs
    )
    report = {
        'mode': 'idle-eye-only',
        'referenceFrame': reference_index,
        'openEyeSourceFrame': open_index,
        'openFrames': [0, 1, 5],
        'closedFrames': [2, 3, 4],
        'recommendedFrameDurations': [80, 80, 80, 2400, 80, 80],
        'eyeBoxes': eye_boxes,
        'outsideEyeMaxChannelDelta': outside_delta,
        'bodyAndHandsLocked': outside_delta == 0,
        'openFramesMaxDelta': int(np.max(np.abs(outputs[0].astype(np.int16) - outputs[1].astype(np.int16)))),
        'closedFramesMaxDelta': int(np.max(np.abs(outputs[2].astype(np.int16) - outputs[3].astype(np.int16)))),
    }
    return outputs, eye_mask, report


def preserved_idle_sequence(frames, eye_boxes):
    if len(frames) != 6:
        return None
    if not (
        np.array_equal(frames[0], frames[1])
        and np.array_equal(frames[0], frames[5])
        and np.array_equal(frames[2], frames[3])
        and np.array_equal(frames[2], frames[4])
    ):
        return None
    eye_mask = ellipse_mask((frames[0].shape[1], frames[0].shape[0]), eye_boxes, 2.2)
    outside = eye_mask < 0.001
    outside_delta = int(np.max(np.abs(frames[0][outside].astype(np.int16) - frames[2][outside].astype(np.int16))))
    eye_changes = []
    for eye_box in eye_boxes:
        single_eye = ellipse_mask((frames[0].shape[1], frames[0].shape[0]), [eye_box], 0) > 0.5
        eye_changes.append(int(np.count_nonzero(np.any(frames[0] != frames[2], axis=2) & single_eye)))
    if outside_delta or any(count == 0 for count in eye_changes):
        return None
    outputs = [frame.copy() for frame in frames]
    return outputs, eye_mask, {
        'mode': 'preserved-idle-eye-only',
        'referenceFrame': 3,
        'openEyeSourceFrame': 0,
        'openFrames': [0, 1, 5],
        'closedFrames': [2, 3, 4],
        'eyeBoxes': eye_boxes,
        'eyeChangedPixels': eye_changes,
        'outsideEyeMaxChannelDelta': outside_delta,
        'bodyAndHandsLocked': outside_delta == 0,
        'openFramesMaxDelta': 0,
        'closedFramesMaxDelta': 0,
    }


def action_region_mask(size):
    mask = Image.new('L', size, 0)
    draw = ImageDraw.Draw(mask)
    draw.ellipse((76, 178, 210, 340), fill=255)
    draw.ellipse((90, 278, 238, 372), fill=255)
    draw.ellipse((274, 278, 422, 372), fill=255)
    mask = mask.filter(ImageFilter.GaussianBlur(1.5))
    return np.asarray(mask, dtype=np.float32) / 255


def action_pose_score(neutral, frame, allowed):
    neutral_rgb, neutral_alpha = blurred_rgba(neutral, 1.8)
    frame_rgb, frame_alpha = blurred_rgba(frame, 1.8)
    score = np.mean(np.abs(neutral_rgb - frame_rgb), axis=2) + np.abs(neutral_alpha - frame_alpha) * 0.55
    selected = allowed > 0.5
    return float(np.mean(score[selected])) if np.any(selected) else 0


def limb_colors(frame):
    rgb = frame[:, :, :3].astype(np.int16)
    alpha = frame[:, :, 3] > 8
    channel_range = np.max(rgb, axis=2) - np.min(rgb, axis=2)
    skin = (
        alpha
        & (rgb[:, :, 0] > 128)
        & (rgb[:, :, 0] > rgb[:, :, 1] + 5)
        & (rgb[:, :, 0] > rgb[:, :, 2] + 10)
    )
    sleeve = (
        alpha
        & (np.mean(rgb, axis=2) > 142)
        & (channel_range < 88)
        & (rgb[:, :, 2] < rgb[:, :, 0] + 22)
    )
    return skin, sleeve


PLUCK_POLYGONS = {
    'pluck-01': [[(98, 298), (150, 292), (198, 311), (228, 341), (218, 374), (178, 390), (119, 383), (94, 350)]],
    'pluck-02': [[(112, 307), (174, 299), (218, 319), (250, 349), (251, 388), (196, 398), (152, 378), (115, 386), (105, 345)]],
    'pluck-03': [[(112, 309), (169, 300), (210, 320), (234, 349), (232, 391), (181, 399), (148, 378), (115, 386), (105, 345)]],
    'pluck-04': [[(80, 222), (115, 209), (151, 228), (190, 269), (220, 311), (220, 351), (185, 383), (123, 385), (91, 351), (74, 283)]],
    'pluck-05': [[(112, 307), (174, 299), (220, 320), (252, 348), (253, 390), (201, 400), (154, 379), (115, 386), (105, 345)]],
    'pluck-06': [[(68, 184), (107, 174), (148, 198), (184, 244), (216, 291), (225, 334), (198, 373), (150, 390), (105, 375), (77, 335), (66, 270)]],
    'pluck-07': [[(273, 300), (317, 303), (350, 334), (369, 377), (360, 414), (313, 418), (282, 390), (270, 345)]],
    'pluck-08': [[(70, 304), (120, 296), (170, 302), (213, 323), (215, 354), (179, 382), (120, 385), (79, 360), (65, 331)]],
    'pluck-09': [
        [(110, 306), (169, 298), (213, 319), (239, 349), (239, 399), (180, 405), (145, 378), (112, 386), (102, 345)],
        [(211, 306), (269, 303), (315, 323), (351, 347), (358, 397), (312, 419), (262, 405), (214, 397)],
    ],
    'pluck-10': [[(76, 306), (124, 297), (170, 302), (213, 323), (215, 354), (179, 382), (119, 385), (82, 359), (69, 331)]],
}


PLUCK_INTERMEDIATE_FRAMES = {
    'pluck-01': 4,
    'pluck-04': 3,
    'pluck-06': 3,
    'pluck-10': 4,
}


TWO_HAND_COMPANIONS = {
    'pluck-01': {'name': 'pluck-07', 'side': 'right'},
    'pluck-02': {'name': 'pluck-07', 'side': 'right'},
    'pluck-03': {'name': 'pluck-07', 'side': 'right'},
    'pluck-04': {'name': 'pluck-07', 'side': 'right'},
    'pluck-05': {'name': 'pluck-07', 'side': 'right'},
    'pluck-06': {'name': 'pluck-07', 'side': 'right'},
    'pluck-07': {'name': 'pluck-10', 'side': 'left', 'frameOrder': [0, 2, 1, 1, 2, 0]},
    'pluck-08': {'name': 'pluck-07', 'side': 'right'},
    'pluck-10': {'name': 'pluck-07', 'side': 'right'},
}


PLUCK_INTERMEDIATE_DONORS = {
    'pluck-02': [{'name': 'pluck-03', 'side': 'left', 'frame': 1}],
    'pluck-03': [{'name': 'pluck-05', 'side': 'left', 'frame': 1}],
    'pluck-05': [{'name': 'pluck-03', 'side': 'left', 'frame': 1}],
    'pluck-08': [{'name': 'pluck-10', 'side': 'left', 'frame': 1}],
    'pluck-09': [
        {'name': 'pluck-02', 'side': 'left', 'frame': 1},
        {'name': 'pluck-07', 'side': 'right', 'frame': 1},
    ],
}


def limb_patch_mask(canonical, peak, name):
    size = (canonical.shape[1], canonical.shape[0])
    geometry = Image.new('L', size, 0)
    draw = ImageDraw.Draw(geometry)
    for polygon in PLUCK_POLYGONS[name]:
        draw.polygon(polygon, fill=255)
    geometry_array = np.asarray(geometry, dtype=np.float32) / 255
    canonical_skin, canonical_sleeve = limb_colors(canonical)
    peak_skin, peak_sleeve = limb_colors(peak)
    limb = canonical_skin | canonical_sleeve | peak_skin | peak_sleeve
    limb_neighborhood = Image.fromarray(np.where(limb, 255, 0).astype(np.uint8), 'L').filter(ImageFilter.MaxFilter(13))
    skin = canonical_skin | peak_skin
    skin_neighborhood = Image.fromarray(np.where(skin, 255, 0).astype(np.uint8), 'L').filter(ImageFilter.MaxFilter(13))
    mask = np.asarray(limb_neighborhood, dtype=np.float32) / 255 * geometry_array
    mask[338:] *= np.asarray(skin_neighborhood, dtype=np.float32)[338:] / 255
    mask[194:330, 218:294] = 0
    feathered = Image.fromarray(np.clip(np.rint(mask * 255), 0, 255).astype(np.uint8), 'L').filter(ImageFilter.GaussianBlur(1.2))
    return np.asarray(feathered, dtype=np.float32) / 255 * geometry_array


def stabilize_pluck(name, canonical, frames, external_intermediate=None):
    allowed = action_region_mask((canonical.shape[1], canonical.shape[0]))
    neutral = frames[0]
    candidates = list(range(1, len(frames) - 1))
    pose_scores = {index: action_pose_score(neutral, frames[index], allowed) for index in candidates}
    peak_index = max(candidates, key=lambda index: pose_scores[index])
    peak = frames[peak_index]
    peak_mask = limb_patch_mask(canonical, peak, name)
    peak_patch = composite_stable(canonical, peak, peak_mask)
    intermediate_index = PLUCK_INTERMEDIATE_FRAMES.get(name)
    if intermediate_index is not None and np.array_equal(frames[intermediate_index], peak):
        intermediate_index = None
    if intermediate_index is None:
        distinct = [
            index
            for index in candidates
            if not np.array_equal(frames[index], peak) and not np.array_equal(frames[index], canonical)
        ]
        if distinct:
            target_score = pose_scores[peak_index] * 0.6
            intermediate_index = min(distinct, key=lambda index: abs(pose_scores[index] - target_score))
    if intermediate_index is None and external_intermediate is None:
        intermediate_patch = peak_patch
        intermediate_mask = peak_mask
        intermediate_source = peak_index
    elif external_intermediate is not None and intermediate_index is None:
        intermediate_patch = external_intermediate.copy()
        intermediate_mask = np.any(intermediate_patch != canonical, axis=2).astype(np.float32)
        intermediate_source = 'external-donor-pose'
    else:
        intermediate = frames[intermediate_index]
        intermediate_mask = limb_patch_mask(canonical, intermediate, name)
        intermediate_patch = composite_stable(canonical, intermediate, intermediate_mask)
        intermediate_source = intermediate_index
    outputs = [canonical.copy(), intermediate_patch.copy(), peak_patch.copy(), peak_patch.copy(), intermediate_patch.copy(), canonical.copy()]
    union = np.maximum(peak_mask, intermediate_mask)
    static = (union < 0.001) & (canonical[:, :, 3] > 12)
    stable_mae = max(
        float(np.mean(np.abs(output.astype(np.float32) - canonical.astype(np.float32))[static]))
        for output in outputs
    ) if np.any(static) else 0
    report = {
        'mode': 'single-peak-local-patch',
        'canonicalFrame': 'stabilized idle frame 0',
        'sourceFrameSequence': ['canonical', intermediate_source, peak_index, peak_index, intermediate_source, 'canonical'],
        'recommendedFrameDurations': [30, 70, 110, 110, 70, 30] if intermediate_source != peak_index else [30, 90, 90, 90, 90, 30],
        'peakSourceFrame': peak_index,
        'intermediateSourceFrame': intermediate_source,
        'sourcePoseScores': pose_scores,
        'motionPixelFraction': float(np.mean(union > 0.5)),
        'stablePixelFraction': float(np.mean(union <= 0.5)),
        'canonicalStaticMae': stable_mae,
        'firstFrameCanonicalMaxDelta': int(np.max(np.abs(outputs[0].astype(np.int16) - canonical.astype(np.int16)))),
        'lastFrameCanonicalMaxDelta': int(np.max(np.abs(outputs[-1].astype(np.int16) - canonical.astype(np.int16)))),
        'returnIntermediateMaxDelta': int(np.max(np.abs(outputs[1].astype(np.int16) - outputs[4].astype(np.int16)))),
        'peakHoldMaxDelta': int(np.max(np.abs(outputs[2].astype(np.int16) - outputs[3].astype(np.int16)))),
        'symmetricMaxDelta': max(
            int(np.max(np.abs(outputs[1].astype(np.int16) - outputs[4].astype(np.int16)))),
            int(np.max(np.abs(outputs[2].astype(np.int16) - outputs[3].astype(np.int16)))),
        ),
    }
    return outputs, union, report


def add_companion_hand(canonical, frames, companion_frames, side):
    outputs = []
    companion_union = np.zeros(canonical.shape[:2], dtype=np.float32)
    split = canonical.shape[1] // 2
    for frame, companion in zip(frames, companion_frames):
        changed = np.any(companion != canonical, axis=2)
        if side == 'left':
            changed[:, split:] = False
        else:
            changed[:, :split] = False
        output = frame.copy()
        output[changed] = companion[changed]
        output[output[:, :, 3] == 0, :3] = 0
        outputs.append(output)
        companion_union = np.maximum(companion_union, changed.astype(np.float32))
    target_union = np.max(
        np.stack([np.any(frame != canonical, axis=2) for frame in frames]),
        axis=0,
    ).astype(np.float32)
    union = np.maximum(target_union, companion_union)
    changed_counts = []
    for frame in outputs:
        changed = np.any(frame != canonical, axis=2)
        changed_counts.append({
            'left': int(np.count_nonzero(changed[:, :split])),
            'right': int(np.count_nonzero(changed[:, split:])),
        })
    static = union < 0.5
    static_max_delta = max(
        int(np.max(np.abs(frame[static].astype(np.int16) - canonical[static].astype(np.int16))))
        for frame in outputs
    ) if np.any(static) else 0
    return outputs, union, {
        'twoHandMotion': True,
        'companionSide': side,
        'changedPixelsByFrame': changed_counts,
        'twoHandStaticMaxDelta': static_max_delta,
        'bothHandsVisibleFrames': [
            index
            for index, counts in enumerate(changed_counts)
            if counts['left'] > 100 and counts['right'] > 100
        ],
    }


def two_hand_action_corridors(name, size):
    left_geometry = Image.new('L', size, 0)
    right_geometry = Image.new('L', size, 0)
    left_draw = ImageDraw.Draw(left_geometry)
    right_draw = ImageDraw.Draw(right_geometry)

    def add_source(source, side):
        polygons = PLUCK_POLYGONS[source]
        if source == 'pluck-09':
            if side in (None, 'left'):
                left_draw.polygon(polygons[0], fill=255)
            if side in (None, 'right'):
                right_draw.polygon(polygons[1], fill=255)
            return
        draw = right_draw if side == 'right' else left_draw
        for polygon in polygons:
            draw.polygon(polygon, fill=255)

    if name == 'pluck-09':
        add_source(name, None)
    else:
        add_source(name, 'right' if name == 'pluck-07' else 'left')
    companion = TWO_HAND_COMPANIONS.get(name)
    if companion is not None:
        add_source(companion['name'], companion['side'])
    for donor in PLUCK_INTERMEDIATE_DONORS.get(name, []):
        add_source(donor['name'], donor['side'])
    return (
        np.asarray(left_geometry, dtype=np.uint8) > 0,
        np.asarray(right_geometry, dtype=np.uint8) > 0,
    )


def two_hand_motion_report(name, canonical, frames):
    left_corridor, right_corridor = two_hand_action_corridors(name, (canonical.shape[1], canonical.shape[0]))
    corridor = left_corridor | right_corridor
    left_core = left_corridor & ~right_corridor
    right_core = right_corridor & ~left_corridor
    protected_face = np.zeros(canonical.shape[:2], dtype=bool)
    protected_face[194:260, 218:294] = True
    counts = []
    outside_counts = []
    protected_face_counts = []
    for frame in frames:
        changed = np.any(frame != canonical, axis=2)
        outside_counts.append(int(np.count_nonzero(changed & ~corridor)))
        protected_face_counts.append(int(np.count_nonzero(changed & protected_face)))
        counts.append({
            'left': int(np.count_nonzero(changed & left_core)),
            'right': int(np.count_nonzero(changed & right_core)),
        })
    visible = [
        index
        for index, value in enumerate(counts)
        if value['left'] > 100 and value['right'] > 100
    ]
    return {
        'twoHandMotion': len(visible) >= max(1, len(frames) - 2),
        'changedPixelsByFrame': counts,
        'bothHandsVisibleFrames': visible,
        'outsideActionCorridorChangedPixels': outside_counts,
        'protectedFaceChangedPixels': protected_face_counts,
    }


def preserved_two_hand_sequence(name, canonical, frames):
    if len(frames) != 6:
        return None
    if not (
        np.array_equal(frames[0], canonical)
        and np.array_equal(frames[5], canonical)
        and np.array_equal(frames[1], frames[4])
        and np.array_equal(frames[2], frames[3])
    ):
        return None
    hand_report = two_hand_motion_report(name, canonical, frames)
    active = {frame.tobytes() for frame in frames[1:5] if not np.array_equal(frame, canonical)}
    if (
        not hand_report['twoHandMotion']
        or len(active) < 2
        or any(hand_report['outsideActionCorridorChangedPixels'])
        or any(hand_report['protectedFaceChangedPixels'])
    ):
        return None
    outputs = [frame.copy() for frame in frames]
    moving = np.max(
        np.stack([np.any(frame != canonical, axis=2) for frame in outputs]),
        axis=0,
    ).astype(np.float32)
    left_corridor, right_corridor = two_hand_action_corridors(name, (canonical.shape[1], canonical.shape[0]))
    corridor = left_corridor | right_corridor
    static = ~corridor & (canonical[:, :, 3] > 12)
    stable_mae = max(
        float(np.mean(np.abs(frame.astype(np.float32) - canonical.astype(np.float32))[static]))
        for frame in outputs
    ) if np.any(static) else 0
    static_max_delta = max(
        int(np.max(np.abs(frame[static].astype(np.int16) - canonical[static].astype(np.int16))))
        for frame in outputs
    ) if np.any(static) else 0
    return outputs, moving, {
        'mode': 'preserved-two-hand-sequence',
        'canonicalFrame': 'idle frame 0',
        'sourceFrameSequence': [0, 1, 2, 3, 4, 5],
        'motionPixelFraction': float(np.mean(moving > 0.5)),
        'stablePixelFraction': float(np.mean(moving <= 0.5)),
        'canonicalStaticMae': stable_mae,
        'firstFrameCanonicalMaxDelta': 0,
        'lastFrameCanonicalMaxDelta': 0,
        'returnIntermediateMaxDelta': 0,
        'peakHoldMaxDelta': 0,
        'symmetricMaxDelta': 0,
        'twoHandStaticMaxDelta': static_max_delta,
        **hand_report,
    }


def cool_white_lut(canonical, progress):
    output = canonical.copy()
    rgb = canonical[:, :, :3].astype(np.float32)
    strengths = np.array([0.78, 0.84, 0.9], dtype=np.float32)
    corrected = rgb + (255 - rgb) * strengths[None, None, :] * progress
    output[:, :, :3] = np.clip(np.rint(corrected), 0, 255).astype(np.uint8)
    output[output[:, :, 3] == 0, :3] = 0
    return output


def smoothstep(low, high, values):
    scaled = np.clip((values - low) / (high - low), 0, 1)
    return scaled * scaled * (3 - 2 * scaled)


def original_white_lut(canonical):
    output = canonical.copy()
    rgb = canonical[:, :, :3].astype(np.float32)
    red, green, blue_channel = np.moveaxis(rgb, 2, 0)
    luma = red * 0.2126 + green * 0.7152 + blue_channel * 0.0722
    chroma = np.max(rgb, axis=2) - np.min(rgb, axis=2)
    y, x = np.indices(luma.shape)
    blue = smoothstep(5, 70, blue_channel - (red + green) / 2)
    cool = smoothstep(0, 55, np.maximum(blue_channel - red, green - red))
    dark = smoothstep(35, 135, luma)
    head = np.exp(-1.2 * (((x - 255) / 150) ** 2 + ((y - 205) / 135) ** 2))
    skin = smoothstep(8, 40, red - blue_channel) * smoothstep(0, 25, red - green)
    gold = smoothstep(8, 40, red - blue_channel) * smoothstep(3, 30, green - blue_channel)
    neutral = (1 - smoothstep(20, 75, chroma)) * smoothstep(95, 205, luma)
    strength = (
        blue * (0.1 + 0.8 * head) * (0.18 + 0.82 * dark)
        + 0.05 * cool * dark
        + 0.035 * neutral
    ) * (1 - 0.8 * np.maximum(skin, gold))
    strength = np.clip(strength, 0, 0.82)
    target = np.array([238, 248, 255], dtype=np.float32)
    output[:, :, :3] = np.clip(
        np.rint(rgb + (target - rgb) * strength[:, :, None]),
        0,
        255,
    ).astype(np.uint8)
    output[output[:, :, 3] == 0, :3] = 0
    return output


def original_white_effect(canonical, reference_frames):
    historical_alpha = np.maximum.reduce([frame[:, :, 3] for frame in reference_frames[:4]])
    protected = np.asarray(
        Image.fromarray(historical_alpha, 'L').filter(ImageFilter.MaxFilter(9)),
        dtype=np.float32,
    ) / 255
    source = reference_frames[-1]
    alpha = source[:, :, 3].astype(np.float32) / 255 * (1 - protected)
    y = np.indices(alpha.shape)[0]
    alpha *= np.clip((380 - y) / 35, 0, 1)
    alpha[90:130, 185:320] = 0
    alpha[canonical[:, :, 3] > 0] = 0
    effect = np.zeros_like(canonical)
    effect[:, :, :3] = source[:, :, :3]
    effect[:, :, 3] = np.clip(np.rint(alpha * 255), 0, 255).astype(np.uint8)
    effect[effect[:, :, 3] == 0, :3] = 0
    return effect


def stabilize_original_white_transform(canonical, reference_frames):
    progress_values = [0, 0.055, 0.198, 0.394, 0.606, 0.802, 0.945, 1, 1, 0.945, 0.802, 0.606, 0.394, 0.198, 0.055, 0]
    effect_opacities = [progress ** 1.6 for progress in progress_values]
    peak = original_white_lut(canonical)
    peak_effect = original_white_effect(canonical, reference_frames)
    outputs = []
    bases = []
    masks = []
    character_luma = []
    effect_pixel_counts = []
    character = canonical[:, :, 3] > 12
    character_weight = canonical[:, :, 3].astype(np.float32) / 255
    for progress, effect_opacity in zip(progress_values, effect_opacities):
        base = canonical.copy()
        base[:, :, :3] = np.clip(
            np.rint(
                canonical[:, :, :3].astype(np.float32) * (1 - progress)
                + peak[:, :, :3].astype(np.float32) * progress
            ),
            0,
            255,
        ).astype(np.uint8)
        effect = peak_effect.copy()
        effect[:, :, 3] = np.clip(
            np.rint(peak_effect[:, :, 3].astype(np.float32) * effect_opacity),
            0,
            255,
        ).astype(np.uint8)
        effect[effect[:, :, 3] == 0, :3] = 0
        output = np.asarray(
            Image.alpha_composite(Image.fromarray(effect, 'RGBA'), Image.fromarray(base, 'RGBA')),
            dtype=np.uint8,
        ).copy()
        output[output[:, :, 3] == 0, :3] = 0
        outputs.append(output)
        bases.append(base)
        masks.append(effect[:, :, 3].astype(np.float32) / 255)
        luma = base[:, :, 0] * 0.2126 + base[:, :, 1] * 0.7152 + base[:, :, 2] * 0.0722
        character_luma.append(float(np.sum(luma * character_weight) / np.sum(character_weight)))
        effect_pixel_counts.append(int(np.count_nonzero(effect[:, :, 3] > 8)))
    peak_index = int(np.argmax(progress_values))
    peak_rgb = bases[peak_index][:, :, :3].astype(np.float32)
    peak_luma = peak_rgb[:, :, 0] * 0.2126 + peak_rgb[:, :, 1] * 0.7152 + peak_rgb[:, :, 2] * 0.0722
    peak_max = np.max(peak_rgb, axis=2)
    peak_min = np.min(peak_rgb, axis=2)
    peak_saturation = np.zeros_like(peak_max)
    np.divide(peak_max - peak_min, peak_max, out=peak_saturation, where=peak_max > 0)
    peak_saturation *= 255
    y, x = np.indices(character.shape)
    head = character & (x >= 145) & (x < 370) & (y >= 105) & (y < 315)
    effect_union = np.max(np.stack(masks), axis=0)
    moving = np.max(
        np.stack([np.any(output != canonical, axis=2) for output in outputs]),
        axis=0,
    ).astype(np.float32)
    body_residual = max(
        int(np.max(np.abs(output[character].astype(np.int16) - base[character].astype(np.int16))))
        for output, base in zip(outputs, bases)
    )
    first_delta = int(np.max(np.abs(outputs[0].astype(np.int16) - canonical.astype(np.int16))))
    last_delta = int(np.max(np.abs(outputs[-1].astype(np.int16) - canonical.astype(np.int16))))
    symmetry_delta = max(
        int(np.max(np.abs(outputs[index].astype(np.int16) - outputs[-1 - index].astype(np.int16))))
        for index in range(len(outputs) // 2)
    )
    rising = all(right >= left for left, right in zip(character_luma[:peak_index], character_luma[1:peak_index + 1]))
    falling = all(right <= left for left, right in zip(character_luma[peak_index + 1:], character_luma[peak_index + 2:]))
    return outputs, moving, {
        'mode': 'canonical-v1-original-white',
        'canonicalFrame': 'stabilized idle frame 0',
        'referenceFrames': len(reference_frames),
        'lutProgress': progress_values,
        'effectOpacity': effect_opacities,
        'effectSource': 'V1 original exterior sword-ring pixels',
        'recommendedFrames': 16,
        'recommendedColumns': 4,
        'recommendedFrameDurations': [70, 60, 60, 60, 60, 60, 70, 180, 180, 70, 60, 60, 60, 60, 60, 90],
        'characterLuma': character_luma,
        'peakCharacterLuma': character_luma[peak_index],
        'peakCharacterSaturationP90': float(np.percentile(peak_saturation[character], 90)),
        'peakHeadLuma': float(np.mean(peak_luma[head])),
        'peakHeadSaturationP50': float(np.percentile(peak_saturation[head], 50)),
        'effectPixelCounts': effect_pixel_counts,
        'canonicalCharacterAlphaMaxDelta': max(
            int(np.max(np.abs(base[:, :, 3].astype(np.int16) - canonical[:, :, 3].astype(np.int16))))
            for base in bases
        ),
        'canonicalCharacterBodyResidual': body_residual,
        'firstFrameCanonicalMaxDelta': first_delta,
        'lastFrameCanonicalMaxDelta': last_delta,
        'symmetricMaxDelta': symmetry_delta,
        'peakHoldMaxDelta': int(np.max(np.abs(outputs[7].astype(np.int16) - outputs[8].astype(np.int16)))),
        'lumaEnvelopeMonotonic': rising and falling,
        'colorMatching': False,
        'stableLocking': True,
        'registrationApplied': False,
        'effectUnionPixelCount': int(np.count_nonzero(effect_union > 0.03)),
    }


def programmatic_transform_effect(canonical, opacity):
    height, width = canonical.shape[:2]
    alpha = Image.fromarray(canonical[:, :, 3], 'L')
    protected = np.asarray(alpha.filter(ImageFilter.MaxFilter(5)), dtype=np.float32) / 255
    near = np.asarray(alpha.filter(ImageFilter.MaxFilter(11)).filter(ImageFilter.GaussianBlur(2)), dtype=np.float32) / 255
    far = np.asarray(alpha.filter(ImageFilter.MaxFilter(17)).filter(ImageFilter.GaussianBlur(3)), dtype=np.float32) / 255
    weight = np.clip(near * 0.78 + far * 0.22 - protected, 0, 1) * opacity
    weight[canonical[:, :, 3] > 0] = 0
    effect_array = np.zeros((height, width, 4), dtype=np.uint8)
    effect_array[:, :, :3] = np.array([225, 248, 255], dtype=np.uint8)
    effect_array[:, :, 3] = np.clip(np.rint(weight * 150), 0, 255).astype(np.uint8)
    effect_array[effect_array[:, :, 3] == 0, :3] = 0
    return effect_array, weight


def stabilize_soft_white_transform(canonical, frames):
    outputs = []
    masks = []
    bases = []
    progress_values = []
    character_luma = []
    effect_pixel_counts = []
    character = canonical[:, :, 3] > 12
    character_weight = canonical[:, :, 3].astype(np.float32) / 255
    progress_values = [0, 0.055, 0.198, 0.394, 0.606, 0.802, 0.945, 1, 1, 0.945, 0.802, 0.606, 0.394, 0.198, 0.055, 0]
    effect_opacities = [progress ** 1.15 for progress in progress_values]
    for index, effect_opacity in enumerate(effect_opacities):
        progress = progress_values[index]
        base = cool_white_lut(canonical, progress)
        effect, effect_mask = programmatic_transform_effect(canonical, effect_opacity)
        output = np.asarray(
            Image.alpha_composite(Image.fromarray(base, 'RGBA'), Image.fromarray(effect, 'RGBA')),
            dtype=np.uint8,
        ).copy()
        output[output[:, :, 3] == 0, :3] = 0
        outputs.append(output)
        masks.append(effect_mask)
        bases.append(base)
        luma = base[:, :, 0] * 0.2126 + base[:, :, 1] * 0.7152 + base[:, :, 2] * 0.0722
        character_luma.append(float(np.sum(luma * character_weight) / np.sum(character_weight)))
        effect_pixel_counts.append(int(np.count_nonzero(effect_mask > 0.05)))
    effect_union = np.max(np.stack(masks), axis=0)
    body_residual = max(
        int(np.max(np.abs(output[character].astype(np.int16) - base[character].astype(np.int16))))
        for output, base in zip(outputs, bases)
    )
    alpha_delta = max(
        int(np.max(np.abs(base[:, :, 3].astype(np.int16) - canonical[:, :, 3].astype(np.int16))))
        for base in bases
    )
    first_delta = int(np.max(np.abs(outputs[0].astype(np.int16) - canonical.astype(np.int16))))
    last_delta = int(np.max(np.abs(outputs[-1].astype(np.int16) - canonical.astype(np.int16))))
    symmetry_delta = max(
        int(np.max(np.abs(outputs[index].astype(np.int16) - outputs[-1 - index].astype(np.int16))))
        for index in range(len(outputs) // 2)
    )
    peak_index = int(np.argmax(progress_values))
    peak_rgb = bases[peak_index][:, :, :3].astype(np.float32)[character]
    peak_max = np.max(peak_rgb, axis=1)
    peak_min = np.min(peak_rgb, axis=1)
    peak_saturation = np.where(peak_max > 0, (peak_max - peak_min) / peak_max * 255, 0)
    rising = all(right >= left for left, right in zip(character_luma[:peak_index], character_luma[1:peak_index + 1]))
    falling = all(right <= left for left, right in zip(character_luma[peak_index + 1:], character_luma[peak_index + 2:]))
    report = {
        'mode': 'canonical-character-lut-external-effects',
        'canonicalFrame': 'stabilized idle frame 0',
        'lutProgress': progress_values,
        'effectOpacity': effect_opacities,
        'effectSource': 'deterministic attached cool-white aura',
        'recommendedFrames': 16,
        'recommendedColumns': 4,
        'recommendedFrameDurations': [70, 60, 60, 60, 60, 60, 70, 120, 120, 70, 60, 60, 60, 60, 60, 70],
        'characterLuma': character_luma,
        'peakCharacterLuma': character_luma[peak_index],
        'peakCharacterSaturationP90': float(np.percentile(peak_saturation, 90)),
        'effectPixelCounts': effect_pixel_counts,
        'canonicalCharacterAlphaMaxDelta': alpha_delta,
        'canonicalCharacterBodyResidual': body_residual,
        'firstFrameCanonicalMaxDelta': first_delta,
        'lastFrameCanonicalMaxDelta': last_delta,
        'symmetricMaxDelta': symmetry_delta,
        'peakHoldMaxDelta': int(np.max(np.abs(outputs[7].astype(np.int16) - outputs[8].astype(np.int16)))),
        'lumaEnvelopeMonotonic': rising and falling,
        'colorMatching': False,
        'stableLocking': True,
        'registrationApplied': False,
    }
    return outputs, effect_union, report


def stabilize_transform(canonical, frames, reference_frames=None):
    if reference_frames:
        return stabilize_original_white_transform(canonical, reference_frames)
    return stabilize_soft_white_transform(canonical, frames)


def alpha_centroid(frame):
    alpha = frame[:, :, 3].astype(np.float64)
    total = alpha.sum()
    if total == 0:
        return [0, 0]
    y, x = np.indices(alpha.shape)
    return [float((x * alpha).sum() / total), float((y * alpha).sum() / total)]


def temporal_flicker(frames, mask):
    pixels = np.stack([frame.astype(np.float32) for frame in frames])
    alpha = pixels[:, :, :, 3:4] / 255
    premultiplied = pixels[:, :, :, :3] * alpha
    temporal_std = np.mean(np.std(premultiplied, axis=0), axis=2)
    selected = mask > 0.5
    return float(np.mean(temporal_std[selected])) if np.any(selected) else 0


def color_delta(reference, frames, mask):
    selected = (mask > 0.5) & (reference[:, :, 3] > 32)
    if not np.any(selected):
        return 0
    values = []
    for frame in frames:
        values.append(np.mean(np.abs(frame[:, :, :3].astype(np.float32) - reference[:, :, :3].astype(np.float32))[selected]))
    return float(np.mean(values))


def edge_alpha_pixels(frame, inset=2):
    alpha = frame[:, :, 3]
    return int(
        np.count_nonzero(alpha[:inset])
        + np.count_nonzero(alpha[-inset:])
        + np.count_nonzero(alpha[:, :inset])
        + np.count_nonzero(alpha[:, -inset:])
    )


def checkerboard(size, block=16):
    image = Image.new('RGBA', size, (29, 35, 45, 255))
    draw = ImageDraw.Draw(image)
    for y in range(0, size[1], block):
        for x in range(0, size[0], block):
            if (x // block + y // block) % 2:
                draw.rectangle((x, y, x + block - 1, y + block - 1), fill=(50, 59, 73, 255))
    return image


def visible_frame(frame):
    background = checkerboard((frame.shape[1], frame.shape[0]))
    background.alpha_composite(Image.fromarray(frame, 'RGBA'))
    return background.convert('RGB')


def save_gif(frames, path, durations):
    visible = [visible_frame(frame) for frame in frames]
    visible[0].save(path, save_all=True, append_images=visible[1:], duration=durations, loop=0, disposal=2)


def save_side_by_side(original, stabilized, path, durations):
    font = ImageFont.load_default()
    previews = []
    for original_frame, stabilized_frame in zip(original, stabilized):
        left = visible_frame(original_frame)
        right = visible_frame(stabilized_frame)
        image = Image.new('RGB', (left.width * 2, left.height + 24), (18, 22, 30))
        image.paste(left, (0, 24))
        image.paste(right, (left.width, 24))
        draw = ImageDraw.Draw(image)
        draw.text((8, 7), 'before', fill=(255, 255, 255), font=font)
        draw.text((left.width + 8, 7), 'stabilized', fill=(255, 255, 255), font=font)
        previews.append(image)
    previews[0].save(path, save_all=True, append_images=previews[1:], duration=durations, loop=0, disposal=2)


def save_contact_sheet(original, stabilized, path):
    frame_height, frame_width = original[0].shape[:2]
    scale = min(1, 320 / max(frame_width, frame_height))
    width = max(1, round(frame_width * scale))
    height = max(1, round(frame_height * scale))
    label_height = 24
    contact = Image.new('RGB', (width * len(original), (height + label_height) * 2), (18, 22, 30))
    draw = ImageDraw.Draw(contact)
    font = ImageFont.load_default()
    for row, (label, frames) in enumerate((('before', original), ('stabilized', stabilized))):
        for index, frame in enumerate(frames):
            preview = visible_frame(frame).resize((width, height), Image.Resampling.LANCZOS)
            x = index * width
            y = row * (height + label_height)
            contact.paste(preview, (x, y + label_height))
            draw.text((x + 7, y + 7), f'{label} {index}', fill=(255, 255, 255), font=font)
    contact.save(path)


def animation_durations(animation, frame_count):
    durations = animation.get('frameDurations')
    if isinstance(durations, list) and len(durations) == frame_count:
        return [max(20, int(value)) for value in durations]
    duration = round(1000 / max(float(animation.get('fps', 12)), 1))
    return [duration] * frame_count


def round_metrics(value):
    if isinstance(value, float):
        return round(value, 5)
    if isinstance(value, list):
        return [round_metrics(item) for item in value]
    return value


def process_animation(
    name,
    animation,
    model_dir,
    output_dir,
    args,
    canonical=None,
    companion=None,
    intermediate=None,
    transform_reference=None,
):
    source_path = model_dir / animation['file']
    sheet = Image.open(source_path).convert('RGBA')
    frame_width = int(animation['frameWidth'])
    frame_height = int(animation['frameHeight'])
    frame_count = int(animation['frames'])
    columns = int(animation['columns'])
    original = split_frames(sheet, frame_width, frame_height, frame_count, columns)
    reference_index = 3 if name == 'idle' and frame_count > 3 else 0
    reference = canonical if (name.startswith('pluck-') or name == 'transform') and canonical is not None else original[reference_index]
    preserved_idle = preserved_idle_sequence(original, args.eye_box) if name == 'idle' else None
    preserved_pluck = (
        preserved_two_hand_sequence(name, canonical, original)
        if args.two_hand and name.startswith('pluck-') and canonical is not None
        else None
    )
    registered = []
    registrations = []
    for index, frame in enumerate(original):
        if preserved_idle is not None or preserved_pluck is not None:
            dx, dy, cost = 0, 0, 0
            matched, gains, biases = frame.copy(), [1, 1, 1], [0, 0, 0]
            registered.append(matched)
            registrations.append({'frame': index, 'dx': dx, 'dy': dy, 'cost': cost, 'gains': gains, 'biases': biases})
            continue
        if name == 'transform' or (not name.startswith('pluck-') and index == reference_index):
            dx, dy, cost = 0, 0, 0
        else:
            dx, dy, cost = phase_translation(reference, frame, args.max_shift)
        shifted = shift_array(frame, dx, dy)
        if name == 'transform':
            matched, gains, biases = shifted, [1, 1, 1], [0, 0, 0]
        else:
            matched, gains, biases = match_color(reference, shifted)
        registered.append(matched)
        registrations.append({'frame': index, 'dx': dx, 'dy': dy, 'cost': cost, 'gains': gains, 'biases': biases})
    if name == 'idle':
        if preserved_idle is not None:
            stabilized, moving, mode_report = preserved_idle
        else:
            stabilized, moving, mode_report = stabilize_idle(
                registered,
                reference_index,
                args.idle_open_frame,
                args.eye_box,
            )
        stable_mask = 1 - moving
        threshold = None
    elif name.startswith('pluck-') and canonical is not None:
        if preserved_pluck is not None:
            stabilized, moving, mode_report = preserved_pluck
        else:
            stabilized, moving, mode_report = stabilize_pluck(name, canonical, registered, intermediate)
            if companion is not None:
                stabilized, moving, companion_report = add_companion_hand(
                    canonical,
                    stabilized,
                    companion['frames'],
                    companion['side'],
                )
                mode_report.update(companion_report)
                mode_report['companionAnimation'] = companion['name']
                mode_report['mode'] = 'two-hand-local-patches'
        if args.two_hand:
            mode_report.update(two_hand_motion_report(name, canonical, stabilized))
        unique_frames = {frame.tobytes() for frame in stabilized}
        active_frames = {
            frame.tobytes()
            for frame in stabilized
            if not np.array_equal(frame, canonical)
        }
        mode_report['uniqueFrameCount'] = len(unique_frames)
        mode_report['activePoseCount'] = len(active_frames)
        stable_mask = (moving < 0.001).astype(np.float32)
    elif name == 'transform' and canonical is not None:
        stabilized, moving, mode_report = stabilize_transform(canonical, registered, transform_reference)
        stable_mask = (canonical[:, :, 3] > 12).astype(np.float32)
    else:
        moving, threshold = motion_mask(reference, registered)
        stable_mask = 1 - moving
        stabilized = [composite_stable(reference, frame, moving) for frame in registered]
        foreground = np.max(np.stack([frame[:, :, 3] for frame in registered]), axis=0) > 12
        mode_report = {
            'mode': 'registered-color-matched-stable-lock',
            'motionThreshold': threshold,
            'motionPixelFraction': float(np.mean(moving > 0.5)),
            'stablePixelFraction': float(np.mean(stable_mask > 0.5)),
            'motionForegroundFraction': float(np.mean(moving[foreground] > 0.5)),
            'stableForegroundFraction': float(np.mean(stable_mask[foreground] > 0.5)),
        }
    stem = Path(animation['file']).stem
    original_dir = output_dir / 'frames' / stem / 'original'
    stabilized_dir = output_dir / 'frames' / stem / 'stabilized'
    original_dir.mkdir(parents=True, exist_ok=True)
    stabilized_dir.mkdir(parents=True, exist_ok=True)
    for index, source in enumerate(original):
        Image.fromarray(source, 'RGBA').save(original_dir / f'{index:02d}.png')
    for index, result in enumerate(stabilized):
        Image.fromarray(result, 'RGBA').save(stabilized_dir / f'{index:02d}.png')
    sheets_dir = output_dir / 'sheets'
    previews_dir = output_dir / 'previews'
    reports_dir = output_dir / 'reports'
    masks_dir = output_dir / 'masks'
    sheets_dir.mkdir(parents=True, exist_ok=True)
    previews_dir.mkdir(parents=True, exist_ok=True)
    reports_dir.mkdir(parents=True, exist_ok=True)
    masks_dir.mkdir(parents=True, exist_ok=True)
    Image.fromarray(np.rint(moving * 255).astype(np.uint8), 'L').save(masks_dir / f'{stem}-motion.png')
    output_sheet_path = sheets_dir / f'{stem}.webp'
    output_columns = int(mode_report.get('recommendedColumns', columns))
    output_frame_count = len(stabilized)
    compose_sheet(stabilized, output_columns).save(
        output_sheet_path,
        format='WEBP',
        lossless=True,
        quality=100,
        method=6,
        exact=True,
    )
    durations = animation_durations(animation, output_frame_count)
    if 'recommendedFrameDurations' in mode_report:
        durations = mode_report['recommendedFrameDurations']
    before_durations = animation_durations(animation, len(original))
    comparison_original = original
    if len(original) != output_frame_count:
        comparison_original = [reference.copy() for _ in stabilized]
    save_gif(original, previews_dir / f'{stem}-before.gif', before_durations)
    save_gif(stabilized, previews_dir / f'{stem}-after.gif', durations)
    save_side_by_side(comparison_original, stabilized, previews_dir / f'{stem}-comparison.gif', durations)
    save_contact_sheet(comparison_original, stabilized, previews_dir / f'{stem}-contact.png')
    centroids_before = [alpha_centroid(frame) for frame in original]
    centroids_after = [alpha_centroid(frame) for frame in stabilized]
    centroid_spread_before = float(np.mean(np.std(np.asarray(centroids_before), axis=0)))
    centroid_spread_after = float(np.mean(np.std(np.asarray(centroids_after), axis=0)))
    metrics = {
        'alphaCentroidsBefore': centroids_before,
        'alphaCentroidsAfter': centroids_after,
        'centroidSpreadBefore': centroid_spread_before,
        'centroidSpreadAfter': centroid_spread_after,
        'stableRegionColorDeltaBefore': color_delta(reference, registered, stable_mask),
        'stableRegionColorDeltaAfter': color_delta(reference, stabilized, stable_mask),
        'stableRegionFlickerBefore': temporal_flicker(registered, stable_mask),
        'stableRegionFlickerAfter': temporal_flicker(stabilized, stable_mask),
    }
    encoded_sheet = Image.open(output_sheet_path).convert('RGBA')
    encoded_array = np.asarray(encoded_sheet, dtype=np.uint8)
    encoded_frames = split_frames(encoded_sheet, frame_width, frame_height, output_frame_count, output_columns)
    transparent = encoded_array[:, :, 3] == 0
    hidden_rgb_max = int(np.max(encoded_array[:, :, :3][transparent])) if np.any(transparent) else 0
    edge_counts = [edge_alpha_pixels(frame) for frame in encoded_frames]
    empty_frames = [index for index, frame in enumerate(encoded_frames) if not np.any(frame[:, :, 3])]
    errors = []
    expected_size = (frame_width * output_columns, frame_height * math.ceil(output_frame_count / output_columns))
    if encoded_sheet.size != expected_size:
        errors.append('encoded sheet dimensions do not match configured grid')
    if hidden_rgb_max:
        errors.append(f'transparent pixels retain hidden RGB up to {hidden_rgb_max}')
    if empty_frames:
        errors.append(f'empty output frames: {empty_frames}')
    if any(edge_counts):
        errors.append(f'output alpha touches a frame edge: {edge_counts}')
    if name == 'idle':
        if mode_report['outsideEyeMaxChannelDelta'] != 0 or not mode_report['bodyAndHandsLocked']:
            errors.append('idle changes pixels outside the two eye regions')
        if mode_report['openFramesMaxDelta'] or mode_report['closedFramesMaxDelta']:
            errors.append('idle uses blended or nondeterministic eye transition frames')
    elif name.startswith('pluck-'):
        if mode_report['canonicalStaticMae'] > 0.5:
            errors.append(f"canonical static MAE exceeds 0.5: {mode_report['canonicalStaticMae']}")
        if metrics['stableRegionColorDeltaAfter'] > 0.5:
            errors.append(f"stable-region color delta exceeds 0.5: {metrics['stableRegionColorDeltaAfter']}")
        if metrics['stableRegionFlickerAfter'] > 0.5:
            errors.append(f"stable-region flicker exceeds 0.5: {metrics['stableRegionFlickerAfter']}")
        if mode_report['firstFrameCanonicalMaxDelta'] or mode_report['lastFrameCanonicalMaxDelta']:
            errors.append('pluck first or last frame does not exactly match the idle canonical frame')
        if mode_report['symmetricMaxDelta']:
            errors.append('pluck mirrored intermediate or repeated peak frames are not pixel-identical')
        if args.two_hand and not mode_report.get('twoHandMotion'):
            errors.append(f"pluck does not move both hands in every active frame: {mode_report.get('changedPixelsByFrame')}")
        if any(mode_report.get('outsideActionCorridorChangedPixels', [])):
            errors.append(f"pluck changes pixels outside the fixed hand corridor: {mode_report['outsideActionCorridorChangedPixels']}")
        if any(mode_report.get('protectedFaceChangedPixels', [])):
            errors.append(f"pluck changes protected face pixels: {mode_report['protectedFaceChangedPixels']}")
        if mode_report.get('twoHandStaticMaxDelta', 0):
            errors.append(f"two-hand composition changed static pixels: {mode_report['twoHandStaticMaxDelta']}")
        if mode_report.get('activePoseCount', 0) < 2:
            errors.append(f"pluck has fewer than two distinct active poses: {mode_report.get('activePoseCount')}")
    elif name == 'transform':
        progress = mode_report['lutProgress']
        if mode_report['canonicalCharacterAlphaMaxDelta']:
            errors.append('transform canonical character alpha geometry changed')
        if mode_report['canonicalCharacterBodyResidual']:
            errors.append('transform external effects overlap the canonical character body')
        if mode_report['firstFrameCanonicalMaxDelta'] or mode_report['lastFrameCanonicalMaxDelta']:
            errors.append('transform does not begin and end on the exact idle canonical frame')
        if progress[0] != 0 or progress[-1] != 0 or max(progress) < 0.9:
            errors.append(f'transform LUT envelope is incomplete: {progress}')
        if max(abs(right - left) for left, right in zip(progress, progress[1:])) > 0.55:
            errors.append(f'transform LUT envelope has an abrupt time step: {progress}')
        if max(mode_report['effectPixelCounts']) < 100:
            errors.append('transform external highlight extraction found too few effect pixels')
        if mode_report['symmetricMaxDelta']:
            errors.append('transform whitening and recovery frames are not pixel-symmetric')
        if mode_report['peakHoldMaxDelta']:
            errors.append('transform peak white frames do not hold exactly')
        if not mode_report['lumaEnvelopeMonotonic']:
            errors.append('transform luminance does not rise and fall monotonically')
        if mode_report['mode'] == 'canonical-v1-original-white':
            if not 165 <= mode_report['peakCharacterLuma'] <= 180:
                errors.append(f"transform peak luminance is outside 165..180: {mode_report['peakCharacterLuma']}")
            if not 185 <= mode_report['peakHeadLuma'] <= 198:
                errors.append(f"transform peak head luminance is outside 185..198: {mode_report['peakHeadLuma']}")
            if mode_report['peakHeadSaturationP50'] > 60:
                errors.append(f"transform peak head saturation p50 exceeds 60: {mode_report['peakHeadSaturationP50']}")
        else:
            if not 232 <= mode_report['peakCharacterLuma'] <= 242:
                errors.append(f"transform peak luminance is outside 232..242: {mode_report['peakCharacterLuma']}")
            if mode_report['peakCharacterSaturationP90'] > 45:
                errors.append(f"transform peak saturation p90 exceeds 45: {mode_report['peakCharacterSaturationP90']}")
    report = {
        'ok': not errors,
        'animation': name,
        'source': str(source_path.resolve()),
        'output': str(output_sheet_path.resolve()),
        'frameWidth': frame_width,
        'frameHeight': frame_height,
        'sourceFrames': frame_count,
        'frames': output_frame_count,
        'columns': output_columns,
        'registration': registrations,
        **mode_report,
        'acceptance': {
            'errors': errors,
            'hiddenRgbMax': hidden_rgb_max,
            'emptyFrames': empty_frames,
            'edgeAlphaPixels': edge_counts,
        },
        'metrics': metrics,
    }
    report = round_metrics(report)
    (reports_dir / f'{stem}.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    return report


def model_animation_frames(config, model_dir, name):
    animation = config['animations'][name]
    sheet = Image.open(model_dir / animation['file']).convert('RGBA')
    return split_frames(
        sheet,
        int(animation['frameWidth']),
        int(animation['frameHeight']),
        int(animation['frames']),
        int(animation['columns']),
    )


def compose_donor_pose(canonical, config, model_dir, donors):
    output = canonical.copy()
    split = canonical.shape[1] // 2
    for donor in donors:
        source = model_animation_frames(config, model_dir, donor['name'])[donor['frame']]
        changed = np.any(source != canonical, axis=2)
        if donor['side'] == 'left':
            changed[:, split:] = False
        else:
            changed[:, :split] = False
        output[changed] = source[changed]
    output[output[:, :, 3] == 0, :3] = 0
    return output


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--model-dir', required=True)
    parser.add_argument('--output-dir', required=True)
    parser.add_argument('--max-shift', type=int, default=12)
    parser.add_argument('--idle-open-frame', type=int, default=0)
    parser.add_argument('--eye-box', type=int, nargs=4, action='append')
    parser.add_argument('--two-hand', action='store_true')
    args = parser.parse_args()
    if not args.eye_box:
        args.eye_box = [[196, 214, 249, 263], [243, 214, 297, 263]]
    model_dir = Path(args.model_dir)
    output_dir = Path(args.output_dir)
    config = json.loads((model_dir / 'model.json').read_text())
    transform_reference = None
    transform_reference_path = model_dir / 'references' / 'transform-original-white.webp'
    if transform_reference_path.exists():
        transform_animation = config['animations']['transform']
        reference_sheet = Image.open(transform_reference_path).convert('RGBA')
        frame_width = int(transform_animation['frameWidth'])
        frame_height = int(transform_animation['frameHeight'])
        if reference_sheet.width % frame_width or reference_sheet.height % frame_height:
            raise ValueError('transform reference dimensions do not match the configured frame size')
        reference_columns = reference_sheet.width // frame_width
        reference_frames = reference_columns * (reference_sheet.height // frame_height)
        transform_reference = split_frames(
            reference_sheet,
            frame_width,
            frame_height,
            reference_frames,
            reference_columns,
        )
    reports = []
    idle = config['animations']['idle']
    reports.append(process_animation('idle', idle, model_dir, output_dir, args))
    if args.two_hand:
        canonical = model_animation_frames(config, model_dir, 'idle')[0]
    else:
        canonical = np.array(Image.open(output_dir / 'frames' / Path(idle['file']).stem / 'stabilized' / '00.png').convert('RGBA'))
    for name, animation in config['animations'].items():
        if name == 'idle':
            continue
        companion = None
        companion_spec = TWO_HAND_COMPANIONS.get(name) if args.two_hand else None
        if companion_spec is not None:
            companion_frames = model_animation_frames(config, model_dir, companion_spec['name'])
            frame_order = companion_spec.get('frameOrder', list(range(len(companion_frames))))
            companion = {
                'name': companion_spec['name'],
                'side': companion_spec['side'],
                'frames': [companion_frames[index] for index in frame_order],
            }
        intermediate = None
        if args.two_hand and name in PLUCK_INTERMEDIATE_DONORS:
            intermediate = compose_donor_pose(
                canonical,
                config,
                model_dir,
                PLUCK_INTERMEDIATE_DONORS[name],
            )
        reports.append(process_animation(
            name,
            animation,
            model_dir,
            output_dir,
            args,
            canonical,
            companion,
            intermediate,
            transform_reference if name == 'transform' else None,
        ))
    summary = {
        'ok': all(report['ok'] for report in reports),
        'model': str(model_dir.resolve()),
        'output': str(output_dir.resolve()),
        'animations': [report['animation'] for report in reports],
        'reports': [str((output_dir / 'reports' / f"{Path(config['animations'][report['animation']]['file']).stem}.json").resolve()) for report in reports],
    }
    (output_dir / 'summary.json').write_text(json.dumps(summary, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps(summary, ensure_ascii=False))
    raise SystemExit(0 if summary['ok'] else 1)


if __name__ == '__main__':
    main()

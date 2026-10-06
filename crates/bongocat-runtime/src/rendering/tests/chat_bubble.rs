//! The bubble's own clock and placement, without a font or a model.
//!
//! A system font is environment, not a test fixture, so these tests drive the
//! state machine with a hand-made texture: the fade, the expiry and the
//! placement are the parts a regression would actually break.

use super::*;

use bongocat_render::{ChatBubbleTexture, ModelBounds};
use std::sync::Arc;

fn texture(width: u32, height: u32) -> Arc<ChatBubbleTexture> {
    Arc::new(ChatBubbleTexture {
        width,
        height,
        rgba: vec![0; (width as usize) * (height as usize) * 4],
    })
}

fn state_with(
    texture: Arc<ChatBubbleTexture>,
    shown_at: Duration,
    hold: Duration,
) -> ChatBubbleState {
    let mut state = ChatBubbleState::new();
    state.active = Some(super::super::chat_bubble::ActiveBubble {
        texture,
        shown_at,
        hold,
    });
    state
}

fn bounds() -> ModelBounds {
    ModelBounds {
        min_x: 0.0,
        max_x: 2.0,
        min_y: 0.0,
        max_y: 3.0,
    }
}

#[test]
fn bubble_fades_in_holds_and_expires_cleanly() {
    let mut state = state_with(texture(100, 50), Duration::ZERO, Duration::from_secs(2));

    let early = state
        .snapshot(Duration::from_millis(90), bounds())
        .expect("fading in");
    assert!((early.opacity - 0.5).abs() < 0.01, "half of the fade in");

    let held = state
        .snapshot(FADE_IN + Duration::from_secs(1), bounds())
        .expect("holding");
    assert!((held.opacity - 1.0).abs() < 0.001, "full during the hold");

    let fading_out = state
        .snapshot(FADE_IN + Duration::from_secs(2) + FADE_OUT / 2, bounds())
        .expect("fading out");
    assert!(
        (fading_out.opacity - 0.5).abs() < 0.01,
        "half of the fade out"
    );

    let total = FADE_IN + Duration::from_secs(2) + FADE_OUT;
    assert!(state.snapshot(total, bounds()).is_none(), "expired");
    assert!(
        state.snapshot(Duration::from_secs(100), bounds()).is_none(),
        "an expired bubble stays gone"
    );
}

#[test]
fn bubble_replacement_restarts_the_clock() {
    let mut state = ChatBubbleState::new();
    let later = Duration::from_secs(30);
    state.show("小明", "大家好", later);
    let active = state.active.as_ref().expect("replacement");
    assert_eq!(active.shown_at, later);

    let even_later = later + Duration::from_secs(5);
    state.show("小红", "你好呀", even_later);
    let active = state.active.as_ref().expect("second replacement");
    assert_eq!(active.shown_at, even_later);
    assert_eq!(state.cache.len(), 2, "both lines rasterized");
}

#[test]
fn bubble_is_anchored_above_the_head_and_sized_by_its_texture() {
    let mut state = state_with(
        texture(REFERENCE_RASTER_WIDTH as u32, 100),
        Duration::ZERO,
        Duration::from_secs(2),
    );
    let bubble = state
        .snapshot(Duration::from_millis(50), bounds())
        .expect("bubble");

    let bounds = bounds();
    let expected_center_x = (bounds.min_x + bounds.max_x) / 2.0;
    assert!((bubble.anchor[0] - expected_center_x).abs() < 0.001);
    let expected_top = bounds.max_y - bounds.height() * TOP_INSET_FRACTION;
    assert!(
        (bubble.anchor[1] + bubble.size[1] - expected_top).abs() < 0.001,
        "the whole bubble, not just its bottom edge, sits below the canvas top"
    );

    let expected_width = (bounds.max_x - bounds.min_x) * DISPLAY_WIDTH_FRACTION;
    assert!((bubble.size[0] - expected_width).abs() < 0.001);
    assert!((bubble.size[1] - expected_width * 100.0 / REFERENCE_RASTER_WIDTH).abs() < 0.001);
    assert!(bubble.opacity > 0.0 && bubble.opacity <= 1.0);
}

#[test]
fn bubble_stays_inside_canvas_for_short_multiline_and_oversized_textures() {
    let canvases = [
        bounds(),
        ModelBounds {
            min_x: -2.0,
            max_x: 2.0,
            min_y: -1.0,
            max_y: 1.0,
        },
        ModelBounds {
            min_x: 10.0,
            max_x: 12.0,
            min_y: -7.0,
            max_y: -4.0,
        },
    ];
    for canvas in canvases {
        for (width, height) in [(160, 98), (776, 242), (1600, 300), (160, 800)] {
            let mut state = state_with(texture(width, height), Duration::ZERO, MINIMUM_HOLD);
            let bubble = state.snapshot(FADE_IN, canvas).expect("visible bubble");
            let left = bubble.anchor[0] - bubble.size[0] / 2.0;
            let right = bubble.anchor[0] + bubble.size[0] / 2.0;
            let top = bubble.anchor[1] + bubble.size[1];
            assert!(left >= canvas.min_x - 0.001 && right <= canvas.max_x + 0.001);
            assert!(bubble.anchor[1] >= canvas.min_y - 0.001);
            assert!(top <= canvas.max_y - canvas.height() * TOP_INSET_FRACTION + 0.001);
            assert!(bubble.size[0] > 0.0 && bubble.size[1] > 0.0);
            assert!(
                (bubble.size[0] / bubble.size[1] - width as f32 / height as f32).abs() < 0.001,
                "fitting the canvas must not stretch the texture"
            );
        }
    }
}

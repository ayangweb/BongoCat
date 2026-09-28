//! The input bindings one model derives from the key artwork it ships.
//!
//! A control the model cannot draw is left unbound on purpose: the runtime drops
//! a press without a hand assignment, so this one map decides both the key
//! overlay layer and whether the paws move. A paw pressing down for an image that
//! can never appear is feedback for something the user cannot see, so the check
//! runs here — before the press reaches the model — and not in the renderer,
//! which is not allowed to decide actions.

use bongocat_input::{GamepadButton, HandSide, InputBindings, PhysicalKey};
use bongocat_live2d_render::KeyImageInventory;
use bongocat_model::{CommittedModel, ModelOrigin};
use bongocat_render::{FUNCTION_KEY_USAGES, KeyIdentity, KeySide};
use std::collections::BTreeMap;

/// The input bindings of a committed model: the static hand table,
/// intersected with the key images the model actually ships.
pub(crate) fn input_bindings_for_committed_model(model: &CommittedModel) -> InputBindings {
    input_bindings_for_model(
        model.origin(),
        model.id().as_str(),
        &KeyImageInventory::read(model.root()),
    )
}

/// The key images decide which keys the model may react to at all.
///
/// A key the model cannot draw is left unbound on purpose:
/// `InputState::model_snapshot` drops a press without a hand assignment, so this
/// one map decides both the key overlay layer and whether
/// `CatParamLeftHandDown` / `CatParamRightHandDown` move. Pressing a key whose
/// image the model does not ship must do nothing at all — a paw pressing down
/// for an image that can never appear is feedback for something the user cannot
/// see. The check runs here, before the press reaches the model, instead of in
/// the renderer, because the renderer is not allowed to decide actions and
/// because the runtime is the single owner of pressed state.
///
/// `bongocat-live2d-render::KeyImageInventory` answers "can this key be drawn" with the
/// same directory scan and the same candidate fallbacks the renderer uses, so a
/// key that is bound here is exactly a key that draws.
pub(crate) fn input_bindings_for_model(
    origin: ModelOrigin,
    model_id: &str,
    key_images: &KeyImageInventory,
) -> InputBindings {
    pub(crate) const RIGHT_ARROW: PhysicalKey = PhysicalKey::from_hid_usage(0x4f);
    // Installed models have no per-model binding configuration yet. They must
    // not fall back to an empty map: `InputState::model_snapshot` drops any
    // key press without a hand assignment, which silently disabled key
    // overlays (and paw motion) for every imported third-party model. Default
    // them to the same keyboard mapping as the "standard"/"keyboard" presets;
    // each key still has to survive the artwork check below, so a model without
    // assets for a side simply reacts to nothing on that side.
    let keyboard_model =
        origin == ModelOrigin::Installed || matches!(model_id, "standard" | "keyboard");
    let mut key_hands = BTreeMap::new();
    if keyboard_model {
        // Every key of the standard 104/105-key layout belongs to the left hand
        // and the arrow cluster to the right: the model's left paw draws the
        // keyboard block and its right paw draws the arrows. One loop covers the
        // whole block, punctuation, PrintScreen and the navigation cluster
        // included, because naming and binding have to cover the same set.
        // `bongocat-live2d::key_name_candidates` names every one of these keys,
        // and `InputState::model_snapshot` drops any press without a hand
        // assignment before the key-image resolver ever sees it — so a key that
        // is named but not bound can never draw its artwork. The vocabulary is
        // deliberately not limited to the keys the shipped models happen to
        // draw: a model that ships `Dot.png`, `Minus.png` or `Insert.png` has to
        // work without a product change, and it does — the image it ships is
        // what makes the key bindable in the first place.
        for usage in 0x04..=0x65 {
            if (RIGHT_ARROW.hid_usage()..=0x52).contains(&usage) {
                continue;
            }
            bind_drawable_key(&mut key_hands, usage, HandSide::Left, key_images);
        }
        // F13 … F24 sit above the block; F1 … F12 are already covered by the
        // loop above. `FUNCTION_KEY_USAGES` is the same table the key-image
        // resolver names its assets from.
        for (first, last) in FUNCTION_KEY_USAGES {
            for usage in first..=last {
                bind_drawable_key(&mut key_hands, usage, HandSide::Left, key_images);
            }
        }
        // Keypad `=`, which macOS reports as a usage of its own.
        bind_drawable_key(&mut key_hands, 0x67, HandSide::Left, key_images);
        // The eight modifier usages. They sit above every range in this block,
        // which is exactly why they were dropped when the block was rewritten:
        // `0x04..=0x65` stops at `0x65`, the function table resumes at `0x68`,
        // and the keypad `=` is a single usage, so `0xe0..=0xe7` matched none of
        // them. Unbound modifiers never reach `InputState::model_snapshot`, so
        // no model could draw `ShiftLeft.png`, `AltLeft.png` or the shared
        // `Meta.png`, and no paw moved for Shift, Control, Alt or Meta — the
        // exact failure ADR-0038 renamed the Alt images to prevent.
        for usage in 0xe0..=0xe7 {
            bind_drawable_key(&mut key_hands, usage, HandSide::Left, key_images);
        }
        // The Apple Fn / globe key. It is the one key of this block that is not
        // on the HID Keyboard/Keypad page — its usage folds Apple's vendor page
        // into the same `u16` — so every range above misses it and it has to be
        // written out, exactly as it has to be named explicitly in
        // `bongocat-live2d::key_name_candidates`. Windows never reports the key
        // (the firmware owns it), so the binding is inert there. It is still
        // gated on the model's artwork like every other key: a model without a
        // `Globe.png` (or the legacy `Function.png`) gets no reaction at all.
        bind_drawable_key(
            &mut key_hands,
            bongocat_render::GLOBE_KEY_USAGE,
            HandSide::Left,
            key_images,
        );
    } else {
        bind_drawable_key(
            &mut key_hands,
            PhysicalKey::KEY_A.hid_usage(),
            HandSide::Left,
            key_images,
        );
    }
    if origin == ModelOrigin::Installed || matches!(model_id, "keyboard" | "gamepad") {
        for usage in RIGHT_ARROW.hid_usage()..=0x52 {
            bind_drawable_key(&mut key_hands, usage, HandSide::Right, key_images);
        }
    }
    InputBindings::with_gamepad_hands(key_hands, gamepad_hands_for_model(key_images))
}

/// The hand each gamepad button is drawn with, decided by the model's own
/// artwork.
///
/// The hand is not a product constant. The pre-rewrite application derived it
/// from the directory a pressed key's image lived in, and so does this: the
/// image a model keeps in `left-keys` is drawn by its left paw, the one in
/// `right-keys` by its right paw. That is the only rule that works for both the
/// bundled `gamepad` model (D-pad, left shoulder and left trigger on the left;
/// face buttons, right shoulder and right trigger on the right, which is the
/// physical layout) and for a converted BongoCatMver model, whose `lefthand`
/// and `righthand` lists decide the output directory of every overlay it
/// installs.
///
/// A button the model ships no artwork for is left unbound and therefore inert,
/// exactly like a keyboard key without its image (ADR-0042): the stick buttons
/// of the bundled model have no `LeftStick.png` / `RightStick.png`, so pressing
/// L3 or R3 moves the stick artwork and nothing else. The two sticks also keep
/// their own `StickLeftDown` / `StickRightDown` parameters, which
/// `InputState::model_snapshot_with_filter` sets independently of any hand.
///
/// A button whose artwork a model keeps in **both** directories is bound to the
/// left hand, the same side the keyboard path binds the whole main block to. The
/// product projects one hand per control, so a button cannot be resolved against
/// both of its images at once; no shipped model does this.
pub(crate) fn gamepad_hands_for_model(
    key_images: &KeyImageInventory,
) -> BTreeMap<GamepadButton, HandSide> {
    let mut hands = BTreeMap::new();
    for button in GamepadButton::ALL {
        let drawable = [HandSide::Left, HandSide::Right].into_iter().find(|side| {
            key_images.can_draw_key(
                match side {
                    HandSide::Left => KeySide::Left,
                    HandSide::Right => KeySide::Right,
                },
                KeyIdentity::Gamepad(button),
            )
        });
        if let Some(side) = drawable {
            hands.insert(button, side);
        }
    }
    hands
}

/// Bind one key to one hand, but only when the model ships an image that draws
/// it. The same key on a model with the artwork and on a model without it must
/// not produce the same reaction: without the image there is nothing to show, so
/// the press is dropped here rather than animated into an invisible key.
pub(crate) fn bind_drawable_key(
    key_hands: &mut BTreeMap<PhysicalKey, HandSide>,
    hid_usage: u16,
    side: HandSide,
    key_images: &KeyImageInventory,
) {
    let key_side = match side {
        HandSide::Left => KeySide::Left,
        HandSide::Right => KeySide::Right,
    };
    if key_images.can_draw(key_side, hid_usage) {
        key_hands.insert(PhysicalKey::from_hid_usage(hid_usage), side);
    }
}

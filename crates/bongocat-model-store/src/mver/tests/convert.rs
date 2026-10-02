//! One planned mode, written out as a package.

use super::*;

#[test]
fn every_configured_mode_is_inspected_and_converts_on_its_own() {
    let root = tempdir().expect("root");
    legacy_source(root.path(), &all_modes(), true);
    let plan = inspect_directory(root.path()).expect("legacy plan");
    assert_eq!(
        plan.modes().collect::<Vec<_>>(),
        MverInputMode::ALL.to_vec()
    );
    assert_eq!(
        plan_mode(&plan, MverInputMode::Standard).model,
        "img/standard/cat_model"
    );

    // Each mode reads the background under its own name.
    assert_eq!(
        plan_mode(&plan, MverInputMode::Standard)
            .background
            .as_deref(),
        Some("img/standard/mousebg.png")
    );
    assert_eq!(
        plan_mode(&plan, MverInputMode::Keyboard)
            .background
            .as_deref(),
        Some("img/keyboard/bg.png")
    );

    // Every mode converts into a package of its own.
    for mode in MverInputMode::ALL {
        let staging = tempdir().expect("staging");
        convert(root.path(), &plan_mode(&plan, mode), staging.path()).expect("convert");
        assert!(
            staging.path().join("cat.model3.json").is_file(),
            "{} must place its entry at the root",
            mode.as_str()
        );
        assert!(
            staging.path().join("resources/left-keys").is_dir(),
            "{} must expose left key images",
            mode.as_str()
        );
    }
}

#[test]
fn a_source_that_keeps_mode_folders_at_its_root_is_accepted() {
    let root = tempdir().expect("root");
    write(
        root.path(),
        LEGACY_CONFIG_FILE,
        &legacy_config(&[(
            MverInputMode::Standard,
            r#"{"hand":[[65]],"keyboard":[[65]]}"#,
        )]),
    );
    write(
        root.path(),
        "standard/cat_model/cat.model3.json",
        br#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
    );
    write(root.path(), "standard/cat_model/model.moc3", b"moc");
    write(
        root.path(),
        "standard/keyboard/0.png",
        &flat([0, 0, 255, 255]),
    );
    write(root.path(), "standard/hand/0.png", &flat([255, 0, 0, 255]));

    let plan = inspect_directory(root.path()).expect("legacy plan");
    let mode = plan_mode(&plan, MverInputMode::Standard);
    assert_eq!(mode.model, "standard/cat_model");
    assert_eq!(
        mode.slots,
        vec![MverSlot {
            reference: "left-keys/KeyA.png".to_owned(),
            image: MverSlotImage::Composite {
                hand: "standard/hand/0.png".to_owned(),
                keyboard: "standard/keyboard/0.png".to_owned(),
            },
        }]
    );
}

#[test]
fn a_mode_without_companion_key_images_copies_the_hand_image_unchanged() {
    let root = tempdir().expect("root");
    legacy_source(
        root.path(),
        &[(
            MverInputMode::Standard,
            r#"{"hand":[[65]],"keyboard":[[65]]}"#,
        )],
        false,
    );
    let plan = inspect_directory(root.path()).expect("legacy plan");
    let mode = plan_mode(&plan, MverInputMode::Standard);
    assert_eq!(
        mode.slots,
        vec![MverSlot {
            reference: "left-keys/KeyA.png".to_owned(),
            image: MverSlotImage::Verbatim("img/standard/hand/0.png".to_owned()),
        }]
    );

    let staging = tempdir().expect("staging");
    convert(root.path(), &mode, staging.path()).expect("convert");
    assert_eq!(
        fs::read(staging.path().join("resources/left-keys/KeyA.png")).expect("verbatim image"),
        fs::read(root.path().join("img/standard/hand/0.png")).expect("source image")
    );
}

#[test]
fn converted_mode_places_the_package_at_the_root_and_composes_key_images() {
    let root = tempdir().expect("root");
    legacy_source(
        root.path(),
        &[(
            MverInputMode::Standard,
            r#"{"hand":[[65]],"keyboard":[[65]]}"#,
        )],
        true,
    );
    let plan = inspect_directory(root.path()).expect("legacy plan");
    let mode = plan_mode(&plan, MverInputMode::Standard);
    let staging = tempdir().expect("staging");
    convert(root.path(), &mode, staging.path()).expect("convert");

    // The entry and the moc sit at the root, which is the only place entry
    // discovery looks, and the mode's own folder name is gone.
    assert!(staging.path().join("cat.model3.json").is_file());
    assert!(staging.path().join("model.moc3").is_file());
    assert!(!staging.path().join("cat_model").exists());
    assert!(staging.path().join("resources/cover.png").is_file());
    assert_eq!(
        fs::read(staging.path().join("resources/background.png")).expect("background"),
        fs::read(root.path().join("img/standard/mousebg.png")).expect("source background")
    );

    // The composed image is the paw over the key cap: the semi-transparent
    // red paw mixes with the opaque blue cap and ends fully opaque.
    let composed = image::open(staging.path().join("resources/left-keys/KeyA.png"))
        .expect("composed image")
        .to_rgba8();
    assert_eq!(composed.dimensions(), (4, 4));
    assert_eq!(composed.get_pixel(0, 0).0, [128, 0, 127, 255]);

    // Converting the same source twice produces the same bytes.
    let again = tempdir().expect("staging");
    convert(root.path(), &mode, again.path()).expect("convert again");
    assert_eq!(
        fs::read(staging.path().join("resources/left-keys/KeyA.png")).expect("first"),
        fs::read(again.path().join("resources/left-keys/KeyA.png")).expect("second")
    );
}

/// The planned output for one ambiguous Alt binding: both sides are
/// destinations for the same composed overlay, and each side-specific code
/// still produces exactly one image.
#[test]
fn one_shared_alt_binding_installs_the_same_overlay_for_both_sides() {
    let root = tempdir().expect("root");
    legacy_source(
        root.path(),
        &[(
            MverInputMode::Standard,
            r#"{"hand":[[18],[164],[165]],"keyboard":[[18],[164],[165]]}"#,
        )],
        true,
    );
    for index in 0..3 {
        write(
            root.path(),
            &format!("img/standard/hand/{index}.png"),
            &flat([255, 0, 0, 255]),
        );
        write(
            root.path(),
            &format!("img/standard/keyboard/{index}.png"),
            &flat([0, 0, 255, 255]),
        );
    }

    let plan = inspect_directory(root.path()).expect("legacy plan");
    let mode = plan_mode(&plan, MverInputMode::Standard);
    assert_eq!(
        mode.slots
            .iter()
            .map(|slot| slot.reference.as_str())
            .collect::<Vec<_>>(),
        vec!["left-keys/AltLeft.png", "left-keys/AltRight.png",]
    );
    // The shared code expands first, so the two side images carry the same
    // overlay bytes and the later per-side bindings are duplicates.
    assert_eq!(mode.slots[0].image, mode.slots[1].image);

    let staging = tempdir().expect("staging");
    convert(root.path(), &mode, staging.path()).expect("convert");
    for name in ["AltLeft", "AltRight"] {
        assert!(
            staging
                .path()
                .join(format!("resources/left-keys/{name}.png"))
                .is_file(),
            "{name} must be installed"
        );
    }
}

/// The planned output for the ambiguous Enter binding: both Enter keys are
/// destinations for the same composed overlay, so a converted model draws
/// it for the main key and for the keypad key alike — exactly what the
/// legacy application did with its one `GetKeyState` code.
#[test]
fn one_shared_enter_binding_installs_the_same_overlay_for_both_keys() {
    let root = tempdir().expect("root");
    legacy_source(
        root.path(),
        &[(
            MverInputMode::Standard,
            r#"{"hand":[[13]],"keyboard":[[13]]}"#,
        )],
        true,
    );
    write(
        root.path(),
        "img/standard/hand/0.png",
        &flat([255, 0, 0, 255]),
    );
    write(
        root.path(),
        "img/standard/keyboard/0.png",
        &flat([0, 0, 255, 255]),
    );

    let plan = inspect_directory(root.path()).expect("legacy plan");
    let mode = plan_mode(&plan, MverInputMode::Standard);
    assert_eq!(
        mode.slots
            .iter()
            .map(|slot| slot.reference.as_str())
            .collect::<Vec<_>>(),
        vec!["left-keys/Enter.png", "left-keys/KpEnter.png"]
    );
    assert_eq!(mode.slots[0].image, mode.slots[1].image);

    let staging = tempdir().expect("staging");
    convert(root.path(), &mode, staging.path()).expect("convert");
    assert_eq!(
        fs::read(staging.path().join("resources/left-keys/Enter.png")).expect("main enter"),
        fs::read(staging.path().join("resources/left-keys/KpEnter.png")).expect("keypad enter")
    );
}

/// All sixteen XInput buttons, through the real conversion, land on sixteen
/// distinct product-named files that carry their own button's artwork.
///
/// This is the exhaustive form of the pairing assertion: the previous test
/// covers the four buttons whose names the old table got wrong, and this one
/// covers the whole vocabulary, the two hand directories, and the fact that
/// no two buttons collide on one file name — the failure mode the old
/// `LeftTrigger`/`LeftTrigger2` pair had.
#[test]
fn every_xinput_button_converts_to_its_own_product_named_image() {
    let root = tempdir().expect("root");
    full_gamepad_source(root.path());
    let plan = inspect_directory(root.path()).expect("legacy plan");
    let mode = plan_mode(&plan, MverInputMode::Gamepad);

    // The whole point: every button lands in the directory its own hand list
    // chose, so the runtime's hand binding (which reads the directory) and
    // the button's name agree. A slot in the wrong directory is a model that
    // draws the button on the wrong paw, so both are collected here.
    let mut installed = BTreeMap::new();
    for slot in &mode.slots {
        let reference = slot.reference.as_str();
        let (directory, name) = reference
            .split_once('/')
            .expect("converted reference has a directory");
        assert!(
            matches!(directory, OUTPUT_LEFT_KEYS | OUTPUT_RIGHT_KEYS),
            "{reference} installs outside the key directories"
        );
        let stem = name.trim_end_matches(".png");
        assert!(
            installed.insert(stem.to_owned(), ()).is_none(),
            "two XInput buttons resolved to {stem}.png"
        );
    }

    for (button, expected, left_hand) in XINPUT_BUTTONS {
        let directory = if left_hand {
            OUTPUT_LEFT_KEYS
        } else {
            OUTPUT_RIGHT_KEYS
        };
        let reference = format!("{directory}/{expected}.png");
        assert!(
            installed.contains_key(expected),
            "XInput button {button} did not convert to {reference}; the plan installed {:?}",
            installed.keys().collect::<Vec<_>>()
        );
        assert!(
            mode.slots.iter().any(|slot| slot.reference == reference),
            "{reference} must be installed in the {directory} the hand list chose"
        );
        let slot = mode
            .slots
            .iter()
            .find(|slot| slot.reference == reference)
            .expect("planned slot");
        let MverSlotImage::Composite { hand, keyboard } = &slot.image else {
            panic!("{reference} is not a composed overlay");
        };
        // The hand layer names the hand directory the legacy table chose, and
        // its own colour, so the composition is that button's artwork.
        let hand_directory = if left_hand {
            LEGACY_LEFT_HAND_DIRECTORY
        } else {
            LEGACY_RIGHT_HAND_DIRECTORY
        };
        let position = if left_hand {
            XINPUT_LEFT_HAND
                .iter()
                .position(|candidate| *candidate == button)
                .expect("button is in the left hand list")
        } else {
            XINPUT_RIGHT_HAND
                .iter()
                .position(|candidate| *candidate == button)
                .expect("button is in the right hand list")
        };
        assert_eq!(
            hand.as_str(),
            format!("img/gamepad/{hand_directory}/{position}.png"),
            "{reference} must draw the {hand_directory} image at {position}"
        );
        // The keyboard atlas is shared and indexed by *position in the
        // keyboard list*, not by the button's XInput index. The left hand
        // starts at 0; the right hand continues from the left hand's length
        // (ADR-0037 §3). So the expected index is the button's position in
        // the keyboard list I wrote, which is exactly what the offset
        // arithmetic has to reproduce.
        let keyboard_index = if left_hand {
            XINPUT_LEFT_HAND
                .iter()
                .position(|candidate| *candidate == button)
                .expect("button is in the left hand list")
        } else {
            XINPUT_LEFT_HAND.len()
                + XINPUT_RIGHT_HAND
                    .iter()
                    .position(|candidate| *candidate == button)
                    .expect("button is in the right hand list")
        };
        assert!(
            keyboard_index < XINPUT_BUTTONS.len(),
            "the shared atlas must have an image at {keyboard_index}"
        );
        assert_eq!(
            keyboard.as_str(),
            format!("img/gamepad/{LEGACY_KEYBOARD_DIRECTORY}/{keyboard_index}.png"),
            "{reference} must composite the shared key cap at {keyboard_index}"
        );
    }
    assert_eq!(installed.len(), XINPUT_BUTTONS.len());
}

/// The composed bytes are the button's own: a name that resolves is not
/// enough, the artwork under it has to be the one that button's legacy
/// binding composes.
#[test]
fn a_converted_gamepad_image_carries_its_own_buttons_artwork() {
    let root = tempdir().expect("root");
    full_gamepad_source(root.path());
    let plan = inspect_directory(root.path()).expect("legacy plan");
    let mode = plan_mode(&plan, MverInputMode::Gamepad);
    let staging = tempdir().expect("staging");
    convert(root.path(), &mode, staging.path()).expect("convert");

    for (button, expected, left_hand) in XINPUT_BUTTONS {
        let directory = if left_hand {
            OUTPUT_LEFT_KEYS
        } else {
            OUTPUT_RIGHT_KEYS
        };
        let path = staging
            .path()
            .join("resources")
            .join(directory)
            .join(format!("{expected}.png"));
        let image = image::open(&path).expect("composed overlay").to_rgba8();
        // The hand layer is opaque, so the composite is exactly its colour.
        assert_eq!(
            *image.get_pixel(0, 0),
            image::Rgba(button_colour(button)),
            "{expected}.png must hold XInput button {button}'s own artwork"
        );
    }
}

/// One legacy mode whose entry names motion audio, and a real model.
///
/// `sound` is what the entry declares and `present` is what the mode's Live2D
/// package actually carries, so a caller can say "this mode ships this audio"
/// or "this mode names audio it never shipped".
fn motion_audio_source(root: &Path, sound: &str, present: &[&str]) {
    legacy_source(
        root,
        &[(
            MverInputMode::Standard,
            r#"{"hand":[[65]],"keyboard":[[65]]}"#,
        )],
        true,
    );
    let entry = format!(
        r#"{{
  "Version": 3,
  "FileReferences": {{
    "Moc": "model.moc3",
    "Textures": [],
    "Motions": {{
      "CAT_motion": [
        {{ "File": "one.motion3.json", "Sound": "{sound}", "FadeInTime": 0, "FadeOutTime": 0 }},
        {{ "File": "two.motion3.json", "FadeInTime": 0, "FadeOutTime": 0 }}
      ]
    }}
  }},
  "Groups": [{{ "Target": "Parameter", "Name": "EyeBlink", "Ids": ["ParamEyeLOpen"] }}]
}}"#
    );
    write(
        root,
        &format!("img/standard/{LEGACY_MODEL_DIRECTORY}/cat.model3.json"),
        entry.as_bytes(),
    );
    for motion in ["one.motion3.json", "two.motion3.json"] {
        write(
            root,
            &format!("img/standard/{LEGACY_MODEL_DIRECTORY}/{motion}"),
            br#"{"Version":3,"Meta":{"Duration":1.0,"Fps":30.0,"Loop":true,"AreBeziersRestricted":false,"CurveCount":1,"TotalSegmentCount":1,"TotalPointCount":1,"UserDataCount":0,"TotalUserDataSize":0},"Curves":[{"Target":"Parameter","Id":"ParamAngleX","Segments":[0,0,1,1,1,0]}]}"#,
        );
    }
    for audio in present {
        write(
            root,
            &format!("img/standard/{LEGACY_MODEL_DIRECTORY}/{audio}"),
            &flac_header(),
        );
    }
}

/// The 34 bytes a FLAC STREAMINFO block is: enough for the package parser's
/// audio check, without an encoder in the fixture.
fn flac_header() -> Vec<u8> {
    let mut bytes = Vec::from(*b"fLaC");
    bytes.extend_from_slice(&34_u32.to_be_bytes());
    bytes.extend_from_slice(b"STREAMINFO");
    bytes.extend_from_slice(&[0; 34]);
    bytes
}

/// A motion's audio is optional, and a real model leaves the name of audio it
/// never copied.
///
/// A model assembled from Cubism's own sample package keeps the sample's
/// `Sound` reference to a file it does not ship, and the legacy application
/// played its sounds from its own per-mode list instead — so nothing about the
/// model is lost by dropping the name, while keeping it makes the whole mode
/// fail package validation over audio that was never there.
///
/// This is the same call ADR-0037 §5 makes for a missing key-image layer: one
/// missing sound is not an unusable model.
#[test]
fn a_motion_naming_audio_the_source_never_carried_converts_without_it() {
    let root = tempdir().expect("root");
    motion_audio_source(root.path(), "live2d_motion1.flac", &[]);
    let plan = inspect_directory(root.path()).expect("legacy plan");
    let mode = plan_mode(&plan, MverInputMode::Standard);
    assert_eq!(mode.entry, "cat.model3.json");

    let staging = tempdir().expect("staging");
    convert(root.path(), &mode, staging.path()).expect("convert");
    let entry: serde_json::Value =
        serde_json::from_slice(&fs::read(staging.path().join("cat.model3.json")).expect("entry"))
            .expect("entry json");

    // Only the dangling name is gone. The motion that carried it, the motion
    // that did not, and every other declaration survive.
    let motions = entry["FileReferences"]["Motions"]["CAT_motion"]
        .as_array()
        .expect("motion group");
    assert_eq!(motions.len(), 2);
    assert_eq!(motions[0]["File"], "one.motion3.json");
    assert_eq!(
        motions[0].get("Sound"),
        None,
        "the dangling name is dropped"
    );
    assert_eq!(motions[1]["File"], "two.motion3.json");
    assert_eq!(entry["FileReferences"]["Moc"], "model.moc3");
    assert_eq!(entry["Groups"][0]["Name"], "EyeBlink");
    assert_eq!(entry["Version"], 3);
    // The motion files themselves are copied unchanged; only the entry that
    // names them is rewritten.
    assert!(staging.path().join("one.motion3.json").is_file());
    assert!(staging.path().join("two.motion3.json").is_file());
}

/// A model whose audio it really does ship keeps its entry byte for byte.
///
/// The rewrite is a repair, not a reformatting step: an entry with nothing
/// dangling must reach the store exactly as the model author wrote it, or
/// every conversion would quietly rewrite a third party's file.
#[test]
fn a_motion_whose_audio_the_source_carries_keeps_its_reference() {
    let root = tempdir().expect("root");
    motion_audio_source(root.path(), "one.flac", &["one.flac"]);
    let source_entry = fs::read(
        root.path()
            .join("img/standard")
            .join(LEGACY_MODEL_DIRECTORY)
            .join("cat.model3.json"),
    )
    .expect("source entry");

    let plan = inspect_directory(root.path()).expect("legacy plan");
    let staging = tempdir().expect("staging");
    convert(
        root.path(),
        &plan_mode(&plan, MverInputMode::Standard),
        staging.path(),
    )
    .expect("convert");

    assert_eq!(
        fs::read(staging.path().join("cat.model3.json")).expect("installed entry"),
        source_entry
    );
}

/// A reference that reaches outside the package is not a dangling name, and
/// dropping it would throw away a path-safety finding instead of reporting it.
///
/// So the entry is left exactly as the model author wrote it and the package
/// validator is the one that refuses it.
#[test]
fn a_sound_reference_that_escapes_the_package_is_left_for_the_validator() {
    let root = tempdir().expect("root");
    motion_audio_source(root.path(), "../outside.flac", &[]);
    let source_entry = fs::read(
        root.path()
            .join("img/standard")
            .join(LEGACY_MODEL_DIRECTORY)
            .join("cat.model3.json"),
    )
    .expect("source entry");

    let plan = inspect_directory(root.path()).expect("legacy plan");
    let staging = tempdir().expect("staging");
    convert(
        root.path(),
        &plan_mode(&plan, MverInputMode::Standard),
        staging.path(),
    )
    .expect("convert");

    assert_eq!(
        fs::read(staging.path().join("cat.model3.json")).expect("installed entry"),
        source_entry
    );
}

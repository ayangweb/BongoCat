//! Synthetic legacy sources, shared by this module's tests and the store's.
//!
//! The images are deliberately tiny so the tests stay fast. The paw is
//! semi-transparent red and the key cap opaque blue, so a composed result is
//! distinguishable from either input.

use super::*;

pub(crate) fn encode_png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            raw.extend_from_slice(&pixel(x, y));
        }
    }
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(&raw, width, height, image::ExtendedColorType::Rgba8)
        .expect("encode png");
    encoded
}

pub(crate) fn write(directory: &Path, reference: &str, bytes: &[u8]) {
    let path = directory.join(reference);
    fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
    fs::write(path, bytes).expect("write file");
}

pub(crate) fn flat(colour: [u8; 4]) -> Vec<u8> {
    encode_png(4, 4, move |_, _| colour)
}

pub(crate) fn legacy_config(sections: &[(MverInputMode, &str)]) -> Vec<u8> {
    let mut config = serde_json::Map::new();
    for (mode, body) in sections {
        let section: serde_json::Value = serde_json::from_str(body).expect("section json");
        config.insert(mode.as_str().to_owned(), section);
    }
    serde_json::to_vec(&serde_json::Value::Object(config)).expect("config json")
}

/// The config sections of a source that carries all three modes.
///
/// The gamepad section addresses buttons with XInput button indices, so its
/// overlays land on `LeftShoulder` / `LeftTrigger` / `South` /
/// `RightShoulder`.
pub(crate) fn all_modes() -> Vec<(MverInputMode, &'static str)> {
    vec![
        (
            MverInputMode::Standard,
            r#"{"hand":[[65],[66]],"keyboard":[[65],[66]]}"#,
        ),
        (
            MverInputMode::Keyboard,
            r#"{"lefthand":[[65]],"righthand":[[37]],"keyboard":[[65],[37]]}"#,
        ),
        (
            MverInputMode::Gamepad,
            r#"{"lefthand":[[4],[6]],"righthand":[[0],[5]],"keyboard":[[4],[6],[0],[5]]}"#,
        ),
    ]
}

/// Write a minimal but valid legacy source under `root`.
pub(crate) fn legacy_source(root: &Path, sections: &[(MverInputMode, &str)], keyboard_layer: bool) {
    write(root, LEGACY_CONFIG_FILE, &legacy_config(sections));
    for (mode, _) in sections {
        let base = format!("img/{}", mode.as_str());
        write(
            root,
            &format!("{base}/{LEGACY_MODEL_DIRECTORY}/cat.model3.json"),
            br#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":[]}}"#,
        );
        write(
            root,
            &format!("{base}/{LEGACY_MODEL_DIRECTORY}/model.moc3"),
            b"moc",
        );
        if keyboard_layer {
            // Enough key caps for the widest section any fixture uses; the
            // companion image a binding reads is picked by index, and a
            // missing one is what the skip tests remove on purpose.
            for (index, colour) in [
                [0, 0, 255, 255],
                [0, 255, 0, 255],
                [255, 255, 0, 255],
                [255, 0, 255, 255],
            ]
            .into_iter()
            .enumerate()
            {
                write(
                    root,
                    &format!("{base}/{LEGACY_KEYBOARD_DIRECTORY}/{index}.png"),
                    &flat(colour),
                );
            }
        }
        let hand_directories = match mode {
            MverInputMode::Standard => vec![LEGACY_HAND_DIRECTORY],
            MverInputMode::Keyboard | MverInputMode::Gamepad => {
                vec![LEGACY_LEFT_HAND_DIRECTORY, LEGACY_RIGHT_HAND_DIRECTORY]
            }
        };
        for hand_directory in hand_directories {
            for index in 0..2 {
                write(
                    root,
                    &format!("{base}/{hand_directory}/{index}.png"),
                    &flat([255, 0, 0, 128]),
                );
            }
        }
        write(
            root,
            &format!("{base}/{}", mode.background_file()),
            &encode_png(2, 2, |_, _| [10, 20, 30, 255]),
        );
        write(
            root,
            &format!("{base}/{LEGACY_COVER}"),
            &encode_png(2, 2, |_, _| [40, 50, 60, 255]),
        );
    }
}

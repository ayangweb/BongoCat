//! Refusing an audio or image file the renderer cannot decode.

use super::*;

#[test]
fn audio_contract_rejects_unsupported_or_malformed_flac_resources() {
    let package = tempdir().expect("package");
    let unsupported = package.path().join("sound.wav");
    fs::write(&unsupported, b"RIFF").expect("unsupported audio resource");
    let error = validate_flac_resource(&unsupported, "sound.wav")
        .expect_err("unsupported audio format must be rejected");
    assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);

    let malformed = package.path().join("sound.flac");
    fs::write(&malformed, b"not-a-flac").expect("malformed audio resource");
    let error = validate_flac_resource(&malformed, "sound.flac")
        .expect_err("malformed FLAC must be rejected");
    assert_eq!(error.code, ModelDiagnostic::ModelResourceInvalid);
}

proptest! {


    #[test]
    fn png_dimensions_are_parsed_and_limited_without_pixel_allocation(
        width in any::<u32>(),
        height in any::<u32>(),
        maximum_dimension in any::<u32>(),
    ) {
        let mut header = [0_u8; 24];
        header[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        header[12..16].copy_from_slice(b"IHDR");
        header[16..20].copy_from_slice(&width.to_be_bytes());
        header[20..24].copy_from_slice(&height.to_be_bytes());

        let parsed = parse_png_dimensions(&header, "generated.png");
        if width == 0 || height == 0 {
            prop_assert_eq!(parsed.expect_err("zero dimension").code, ModelDiagnostic::ModelTextureInvalidPng);
        } else {
            prop_assert_eq!(parsed?, (width, height));
            let limited = validate_texture_dimensions(
                width,
                height,
                maximum_dimension,
                "generated.png",
            );
            prop_assert_eq!(limited.is_ok(), width <= maximum_dimension && height <= maximum_dimension);
            if let Err(error) = limited {
                prop_assert_eq!(error.code, ModelDiagnostic::ModelTextureDimensionExceeded);
            }
        }
    }

    #[test]
    fn arbitrary_png_headers_cannot_produce_unrelated_dimensions(
        header in proptest::collection::vec(any::<u8>(), 0..64),
    ) {
        if let Ok((width, height)) = parse_png_dimensions(&header, "generated.png") {
            prop_assert!(header.len() >= 24);
            prop_assert_eq!(&header[..8], b"\x89PNG\r\n\x1a\n");
            prop_assert_eq!(&header[12..16], b"IHDR");
            prop_assert_eq!(width, u32::from_be_bytes(header[16..20].try_into().expect("width slice")));
            prop_assert_eq!(height, u32::from_be_bytes(header[20..24].try_into().expect("height slice")));
            prop_assert!(width > 0 && height > 0);
        }
    }
}

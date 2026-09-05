pub(crate) fn validate_icns(bytes: &[u8]) -> Result<(), &'static str> {
    if bytes.len() < 8 || &bytes[..4] != b"icns" {
        return Err("missing icns header");
    }
    let declared_length =
        u32::from_be_bytes(bytes[4..8].try_into().map_err(|_| "missing icns length")?) as usize;
    if declared_length != bytes.len() {
        return Err("icns length does not match file size");
    }
    Ok(())
}

pub(crate) fn validate_ico(bytes: &[u8]) -> Result<(), &'static str> {
    if bytes.len() < 6 || bytes[..4] != [0, 0, 1, 0] {
        return Err("missing ico header");
    }
    let image_count = u16::from_le_bytes(
        bytes[4..6]
            .try_into()
            .map_err(|_| "missing ico image count")?,
    ) as usize;
    if image_count == 0 {
        return Err("ico contains no images");
    }
    let directory_end = image_count
        .checked_mul(16)
        .and_then(|length| length.checked_add(6))
        .ok_or("ico directory length overflowed")?;
    if directory_end > bytes.len() {
        return Err("ico directory exceeds file size");
    }

    for entry in bytes[6..directory_end].chunks_exact(16) {
        let image_length = u32::from_le_bytes(
            entry[8..12]
                .try_into()
                .map_err(|_| "invalid ico image length")?,
        ) as usize;
        let image_offset = u32::from_le_bytes(
            entry[12..16]
                .try_into()
                .map_err(|_| "invalid ico image offset")?,
        ) as usize;
        let image_end = image_offset
            .checked_add(image_length)
            .ok_or("ico image range overflowed")?;
        if image_length == 0 || image_offset < directory_end || image_end > bytes.len() {
            return Err("ico image range exceeds file size");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_icns, validate_ico};

    const PRODUCT_ICNS: &[u8] = include_bytes!("../../../resources/icons/BongoCat.icns");
    const PRODUCT_ICO: &[u8] = include_bytes!("../../../resources/icons/BongoCat.ico");

    #[test]
    fn native_product_icons_have_valid_containers() {
        validate_icns(PRODUCT_ICNS).expect("valid Native macOS icon");
        validate_ico(PRODUCT_ICO).expect("valid Native Windows icon");
    }

    #[test]
    fn malformed_product_icons_are_rejected() {
        assert_eq!(validate_icns(b"icon"), Err("missing icns header"));
        assert_eq!(
            validate_ico(&[0, 0, 1, 0, 0, 0]),
            Err("ico contains no images")
        );
    }
}

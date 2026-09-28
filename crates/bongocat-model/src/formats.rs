//! Probing the file formats the schema names but does not define.
//!
//! A FLAC and a PNG are only "a texture" as far as the schema is concerned.
//! These read the header each format puts its dimensions in, so a package
//! cannot declare one image and ship another.

use super::*;

pub(crate) fn validate_flac_resource(path: &Path, reference: &str) -> Result<(), ModelError> {
    if !reference
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("flac"))
    {
        return invalid_resource(reference, "motion audio must use the supported FLAC format");
    }

    let mut file = File::open(path).map_err(|error| {
        ModelError::new(
            ModelDiagnostic::ModelIoError,
            Some(reference),
            format!("audio resource cannot be opened: {error}"),
        )
    })?;
    let mut signature = [0_u8; 4];
    file.read_exact(&mut signature).map_err(|_| {
        ModelError::new(
            ModelDiagnostic::ModelResourceInvalid,
            Some(reference),
            "FLAC resource is missing its signature",
        )
    })?;
    if signature != *b"fLaC" {
        return invalid_resource(reference, "motion audio does not have a FLAC signature");
    }

    let mut block_header = [0_u8; 4];
    file.read_exact(&mut block_header).map_err(|_| {
        ModelError::new(
            ModelDiagnostic::ModelResourceInvalid,
            Some(reference),
            "FLAC resource is missing a STREAMINFO block",
        )
    })?;
    let mut block_is_last = block_header[0] & 0x80 != 0;
    let block_kind = block_header[0] & 0x7f;
    let block_length = u32::from_be_bytes([0, block_header[1], block_header[2], block_header[3]]);
    if block_kind != 0 || block_length != 34 {
        return invalid_resource(
            reference,
            "FLAC resource must begin with a 34-byte STREAMINFO block",
        );
    }

    let mut stream_info = [0_u8; 34];
    file.read_exact(&mut stream_info).map_err(|_| {
        ModelError::new(
            ModelDiagnostic::ModelResourceInvalid,
            Some(reference),
            "FLAC STREAMINFO block is truncated",
        )
    })?;
    let audio_properties = u64::from_be_bytes([
        stream_info[10],
        stream_info[11],
        stream_info[12],
        stream_info[13],
        stream_info[14],
        stream_info[15],
        stream_info[16],
        stream_info[17],
    ]);
    let sample_rate = audio_properties >> 44;
    let channels = ((audio_properties >> 41) & 0x7) + 1;
    let bits_per_sample = ((audio_properties >> 36) & 0x1f) + 1;
    if sample_rate == 0 || channels > 8 || bits_per_sample > 32 {
        return invalid_resource(
            reference,
            "FLAC STREAMINFO contains invalid audio properties",
        );
    }
    while !block_is_last {
        file.read_exact(&mut block_header).map_err(|_| {
            ModelError::new(
                ModelDiagnostic::ModelResourceInvalid,
                Some(reference),
                "FLAC metadata block is truncated",
            )
        })?;
        block_is_last = block_header[0] & 0x80 != 0;
        let block_length =
            u32::from_be_bytes([0, block_header[1], block_header[2], block_header[3]]);
        consume_flac_metadata_block(&mut file, block_length, reference)?;
    }
    if file.read(&mut [0_u8; 1]).map_err(|error| {
        ModelError::new(
            ModelDiagnostic::ModelIoError,
            Some(reference),
            format!("FLAC resource cannot be read: {error}"),
        )
    })? == 0
    {
        return invalid_resource(reference, "FLAC resource has no audio frames");
    }
    Ok(())
}

pub(crate) fn consume_flac_metadata_block(
    file: &mut File,
    length: u32,
    reference: &str,
) -> Result<(), ModelError> {
    let mut remaining = length as usize;
    let mut buffer = [0_u8; 8 * 1024];
    while remaining > 0 {
        let read_length = remaining.min(buffer.len());
        let read = file.read(&mut buffer[..read_length]).map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelIoError,
                Some(reference),
                format!("FLAC metadata cannot be read: {error}"),
            )
        })?;
        if read == 0 {
            return invalid_resource(reference, "FLAC metadata block is truncated");
        }
        remaining -= read;
    }
    Ok(())
}

pub(crate) fn read_png_dimensions(path: &Path, reference: &str) -> Result<(u32, u32), ModelError> {
    let mut header = [0_u8; 24];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|error| {
            ModelError::new(
                ModelDiagnostic::ModelTextureInvalidPng,
                Some(reference),
                format!("PNG header cannot be read: {error}"),
            )
        })?;
    parse_png_dimensions(&header, reference)
}

pub(crate) fn parse_png_dimensions(
    header: &[u8],
    reference: &str,
) -> Result<(u32, u32), ModelError> {
    if header.len() < 24 || header[..8] != *b"\x89PNG\r\n\x1a\n" || header[12..16] != *b"IHDR" {
        return Err(ModelError::new(
            ModelDiagnostic::ModelTextureInvalidPng,
            Some(reference),
            "texture does not have a PNG IHDR header",
        ));
    }
    let width = u32::from_be_bytes(header[16..20].try_into().expect("fixed PNG width slice"));
    let height = u32::from_be_bytes(header[20..24].try_into().expect("fixed PNG height slice"));
    if width == 0 || height == 0 {
        return Err(ModelError::new(
            ModelDiagnostic::ModelTextureInvalidPng,
            Some(reference),
            "texture dimensions must be non-zero",
        ));
    }
    Ok((width, height))
}

pub(crate) fn validate_texture_dimensions(
    width: u32,
    height: u32,
    maximum_dimension: u32,
    reference: &str,
) -> Result<(), ModelError> {
    if width > maximum_dimension || height > maximum_dimension {
        return Err(ModelError::new(
            ModelDiagnostic::ModelTextureDimensionExceeded,
            Some(reference),
            format!("texture is {width}x{height}; maximum side is {maximum_dimension}"),
        ));
    }
    Ok(())
}

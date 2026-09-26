//! Turning one planned mode into a package on disk.
//!
//! The two differences from the legacy layout are applied here: the Live2D
//! package moves to the package root, because entry discovery only looks there,
//! and each key becomes one composed image named in the product's own
//! vocabulary. Nothing is shared between modes, because the three ship three
//! different models.

use super::*;

/// Convert one legacy mode into a BongoCat package inside `destination`.
///
/// The destination is the store's own staging directory, so a conversion that
/// fails leaves no trace and a successful one is still committed by a single
/// rename. Nothing is written outside that directory.
pub(crate) fn convert_mode<Observe, IsCancelled>(
    source: &MverSource,
    plan: &MverModePlan,
    destination: &Path,
    limits: ModelPackageLimits,
    statistics: &mut CopyStatistics,
    observation: &mut ImportObservation<'_, Observe, IsCancelled>,
) -> Result<(), ModelStoreError>
where
    Observe: FnMut(ModelImportProgress),
    IsCancelled: FnMut() -> bool,
{
    let mut created = BTreeSet::new();
    // The Live2D package moves out of `cat_model/` onto the package root,
    // because that is the only place entry discovery looks.
    for reference in source.files_below(&plan.model, limits)? {
        observation.check_cancelled()?;
        let Some(relative) = reference.strip_prefix(&plan.model) else {
            continue;
        };
        let target = relative.trim_start_matches('/');
        if target.is_empty() {
            continue;
        }
        let bytes = source.read(&reference)?;
        write_staging_file(
            destination,
            target,
            &bytes,
            &mut created,
            statistics,
            observation,
        )?;
    }

    // The background and the cover are installed byte for byte: they are the
    // model's own artwork, not something the conversion produces.
    for (reference, target) in [
        (plan.background.as_deref(), OUTPUT_BACKGROUND),
        (plan.cover.as_deref(), PACKAGE_COVER_FILE),
    ] {
        let Some(reference) = reference else {
            continue;
        };
        observation.check_cancelled()?;
        let bytes = source.read(reference)?;
        write_staging_file(
            destination,
            &format!("{}/{target}", PACKAGE_RESOURCES_DIRECTORY),
            &bytes,
            &mut created,
            statistics,
            observation,
        )?;
    }

    for slot in &plan.slots {
        observation.check_cancelled()?;
        let target = format!("{}/{}", PACKAGE_RESOURCES_DIRECTORY, slot.reference);
        let bytes = match &slot.image {
            MverSlotImage::Verbatim(hand) => source.read(hand)?,
            MverSlotImage::Composite { hand, keyboard } => {
                let hand = source.read(hand)?;
                let keyboard = source.read(keyboard)?;
                compose_key_image(&keyboard, &hand, &target)?
            }
        };
        write_staging_file(
            destination,
            &target,
            &bytes,
            &mut created,
            statistics,
            observation,
        )?;
    }
    Ok(())
}

/// Create one package directory below `destination`, reusing whatever an earlier
/// file already created.
///
/// Parents are created lazily rather than up front, so a conversion writes only
/// the directories the source actually names.
pub(crate) fn create_package_directory(
    destination: &Path,
    relative: &Path,
    created: &mut BTreeSet<PathBuf>,
) -> Result<(), ModelStoreError> {
    let mut current = PathBuf::new();
    for component in relative.components() {
        current.push(component);
        let path = destination.join(&current);
        if created.contains(&current) {
            continue;
        }
        match fs::create_dir(&path) {
            Ok(()) => {
                set_private_directory(&path).map_err(|error| {
                    ModelStoreError::new(
                        ModelStoreDiagnostic::IoError,
                        None,
                        format!("staging directory permissions cannot be set: {error}"),
                    )
                })?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if !path.is_dir() {
                    return Err(ModelStoreError::new(
                        ModelStoreDiagnostic::SourceChanged,
                        None,
                        "staging path is not a directory",
                    ));
                }
            }
            Err(error) => {
                return Err(ModelStoreError::new(
                    ModelStoreDiagnostic::IoError,
                    None,
                    format!("staging directory cannot be created: {error}"),
                ));
            }
        }
        created.insert(current.clone());
    }
    Ok(())
}

pub(crate) fn write_staging_file<Observe, IsCancelled>(
    destination: &Path,
    reference: &str,
    bytes: &[u8],
    created: &mut BTreeSet<PathBuf>,
    statistics: &mut CopyStatistics,
    observation: &mut ImportObservation<'_, Observe, IsCancelled>,
) -> Result<(), ModelStoreError>
where
    Observe: FnMut(ModelImportProgress),
    IsCancelled: FnMut() -> bool,
{
    observation.check_cancelled()?;
    let normalized = normalize_reference(reference)
        .map_err(|error| conversion_error(Some(reference), error.to_string()))?;
    if let Some((parent, _)) = normalized.rsplit_once('/') {
        create_package_directory(destination, &path_from_reference(parent), created)?;
    }
    let target = destination.join(path_from_reference(&normalized));
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let next_file_count = statistics.file_count.saturating_add(1);
    let next_total_bytes = statistics
        .total_bytes
        .checked_add(size)
        .ok_or_else(|| conversion_error(Some(&normalized), "converted package size overflowed"))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|error| {
            conversion_error(
                Some(&normalized),
                format!("converted file cannot be created: {error}"),
            )
        })?;
    set_private_file(&output).map_err(|error| {
        conversion_error(
            Some(&normalized),
            format!("converted file permissions cannot be set: {error}"),
        )
    })?;
    output
        .write_all(bytes)
        .and_then(|()| output.sync_all())
        .map_err(|error| {
            conversion_error(
                Some(&normalized),
                format!("converted file cannot be written: {error}"),
            )
        })?;
    statistics.file_count = next_file_count;
    statistics.total_bytes = next_total_bytes;
    observation.report(ModelImportProgress {
        stage: ModelImportStage::Copying,
        files_copied: file_count_for_progress(statistics.file_count),
        bytes_copied: statistics.total_bytes,
    });
    Ok(())
}

//! Atomic replacement, bounded backups, quarantines and interrupted-commit
//! recovery.
//!
//! A configuration file is never edited in place. Every write lands in a temporary
//! file beside the target, is flushed, and only then replaces it, so a crash leaves
//! either the previous file or the new one and never a half-written mix. What a
//! crash leaves behind is reconciled on the next load: a valid temporary file is
//! promoted, an invalid one is archived, and both archives and backups are bounded
//! and only ever touch files this build owns.

use super::*;

pub(crate) fn unix_time_millis() -> Result<u64, ConfigError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ConfigError::InvalidValue("backup.created_at_unix_ms"))?
        .as_millis()
        .try_into()
        .map_err(|_| ConfigError::InvalidValue("backup.created_at_unix_ms"))
}

pub(crate) fn validate_config_backup(
    bytes: &[u8],
) -> Result<(NativeConfig, ConfigRevision, u32), ConfigError> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_CONFIG_BACKUP_BYTES {
        return Err(ConfigError::BackupTooLarge);
    }
    let backup: ConfigBackup = serde_json::from_slice(bytes)?;
    if backup.backup_format_version != BACKUP_FORMAT_VERSION || backup.created_at_unix_ms == 0 {
        return Err(ConfigError::InvalidValue("backup.format"));
    }
    let actual_schema_version = backup
        .config
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .ok_or(ConfigError::InvalidValue("backup.source_schema_version"))?;
    if actual_schema_version != backup.source_schema_version {
        return Err(ConfigError::InvalidValue("backup.source_schema_version"));
    }
    let config_bytes = serde_json::to_vec(&backup.config)?;
    let (config, revision) = parse_config(&config_bytes)?;
    if backup.source_revision != format!("{:016x}", revision.value()) {
        return Err(ConfigError::InvalidValue("backup.source_revision"));
    }
    Ok((config, revision, backup.source_schema_version))
}

pub(crate) fn owned_config_backup_paths(backups: &Path) -> Result<Vec<PathBuf>, ConfigError> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(backups)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry.file_name().to_str().is_some_and(is_owned_backup_name)
        {
            paths.push(entry.path());
        }
    }
    Ok(paths)
}

pub(crate) fn next_backup_path(
    backups: &Path,
    created_at_unix_ms: u64,
) -> Result<PathBuf, ConfigError> {
    let mut newest = None;
    for entry in fs::read_dir(backups)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if let Some(order) = parse_owned_backup_name(&name) {
            newest = Some(newest.map_or(order, |current| std::cmp::max(current, order)));
        }
    }
    let (order_millis, first_sequence) = match newest {
        Some((newest_millis, newest_sequence)) if newest_millis >= created_at_unix_ms => {
            match newest_sequence.checked_add(1) {
                Some(sequence) => (newest_millis, sequence),
                None => (newest_millis.saturating_add(1), 0),
            }
        }
        _ => (created_at_unix_ms, 0),
    };
    for sequence in first_sequence..=u16::MAX {
        let path = backups.join(format!("config-{order_millis:020}-{sequence:05}.json"));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(ConfigError::Io(io::Error::new(
        ErrorKind::AlreadyExists,
        "config backup filename space exhausted",
    )))
}

pub(crate) fn next_quarantine_path(
    backups: &Path,
    created_at_unix_ms: u64,
) -> Result<PathBuf, ConfigError> {
    let mut newest = None;
    for entry in fs::read_dir(backups)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if let Some(order) = parse_owned_quarantine_name(&name) {
            newest = Some(newest.map_or(order, |current| std::cmp::max(current, order)));
        }
    }
    let (order_millis, first_sequence) = match newest {
        Some((newest_millis, newest_sequence)) if newest_millis >= created_at_unix_ms => {
            match newest_sequence.checked_add(1) {
                Some(sequence) => (newest_millis, sequence),
                None => (newest_millis.saturating_add(1), 0),
            }
        }
        _ => (created_at_unix_ms, 0),
    };
    for sequence in first_sequence..=u16::MAX {
        let path = backups.join(format!(
            "config-corrupt-{order_millis:020}-{sequence:05}.bin"
        ));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(ConfigError::Io(io::Error::new(
        ErrorKind::AlreadyExists,
        "config quarantine filename space exhausted",
    )))
}

pub(crate) fn next_interrupted_archive_path(
    backups: &Path,
    created_at_unix_ms: u64,
    kind: InterruptedArchiveKind,
) -> Result<PathBuf, ConfigError> {
    let mut newest = None;
    for entry in fs::read_dir(backups)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if let Some(order) = parse_owned_interrupted_archive_name(&name) {
            newest = Some(newest.map_or(order, |current| std::cmp::max(current, order)));
        }
    }
    let (order_millis, first_sequence) = match newest {
        Some((newest_millis, newest_sequence)) if newest_millis >= created_at_unix_ms => {
            match newest_sequence.checked_add(1) {
                Some(sequence) => (newest_millis, sequence),
                None => (newest_millis.saturating_add(1), 0),
            }
        }
        _ => (created_at_unix_ms, 0),
    };
    let kind = match kind {
        InterruptedArchiveKind::Stale => "stale",
        InterruptedArchiveKind::Invalid => "invalid",
    };
    for sequence in first_sequence..=u16::MAX {
        let path = backups.join(format!(
            "config-interrupted-{kind}-{order_millis:020}-{sequence:05}.bin"
        ));
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(ConfigError::Io(io::Error::new(
        ErrorKind::AlreadyExists,
        "interrupted config archive filename space exhausted",
    )))
}

pub(crate) fn prune_config_backups(backups: &Path) -> Result<(), ConfigError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(backups)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if is_owned_backup_name(&name) {
            entries.push((name, entry.path(), entry.metadata()?.len()));
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut total_bytes = entries.iter().map(|entry| entry.2).sum::<u64>();
    let remove_count = entries.len().saturating_sub(MAX_CONFIG_BACKUPS);
    let mut removed = 0_usize;
    for (_, path, size) in entries {
        if removed < remove_count || total_bytes > MAX_CONFIG_BACKUP_BYTES {
            fs::remove_file(path)?;
            total_bytes = total_bytes.saturating_sub(size);
            removed += 1;
        }
    }
    Ok(())
}

pub(crate) fn prune_config_quarantines(backups: &Path) -> Result<(), ConfigError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(backups)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if is_owned_quarantine_name(&name) {
            entries.push((name, entry.path(), entry.metadata()?.len()));
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut total_bytes = entries.iter().map(|entry| entry.2).sum::<u64>();
    let remove_count = entries.len().saturating_sub(MAX_CONFIG_QUARANTINES);
    let mut removed = 0_usize;
    for (_, path, size) in entries {
        if removed < remove_count || total_bytes > MAX_CONFIG_QUARANTINE_BYTES {
            fs::remove_file(path)?;
            total_bytes = total_bytes.saturating_sub(size);
            removed += 1;
        }
    }
    Ok(())
}

pub(crate) fn prune_interrupted_archives(backups: &Path) -> Result<(), ConfigError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(backups)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if let Some(order) = parse_owned_interrupted_archive_name(&name) {
            entries.push((order, name, entry.path(), entry.metadata()?.len()));
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let mut total_bytes = entries.iter().map(|entry| entry.3).sum::<u64>();
    let remove_count = entries.len().saturating_sub(MAX_INTERRUPTED_ARCHIVES);
    let mut removed = 0_usize;
    for (_, _, path, size) in entries {
        if removed < remove_count || total_bytes > MAX_INTERRUPTED_ARCHIVE_BYTES {
            fs::remove_file(path)?;
            total_bytes = total_bytes.saturating_sub(size);
            removed += 1;
        }
    }
    Ok(())
}

pub(crate) fn is_owned_backup_name(name: &str) -> bool {
    parse_owned_backup_name(name).is_some()
}

pub(crate) fn parse_owned_backup_name(name: &str) -> Option<(u64, u16)> {
    let stem = name
        .strip_prefix("config-")
        .and_then(|name| name.strip_suffix(".json"))?;
    let (timestamp, sequence) = stem.split_once('-')?;
    if timestamp.len() != 20
        || sequence.len() != 5
        || !timestamp.bytes().all(|byte| byte.is_ascii_digit())
        || !sequence.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some((timestamp.parse().ok()?, sequence.parse().ok()?))
}

pub(crate) fn is_owned_quarantine_name(name: &str) -> bool {
    parse_owned_quarantine_name(name).is_some()
}

pub(crate) fn parse_owned_quarantine_name(name: &str) -> Option<(u64, u16)> {
    let stem = name
        .strip_prefix("config-corrupt-")
        .and_then(|name| name.strip_suffix(".bin"))?;
    let (timestamp, sequence) = stem.split_once('-')?;
    if timestamp.len() != 20
        || sequence.len() != 5
        || !timestamp.bytes().all(|byte| byte.is_ascii_digit())
        || !sequence.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some((timestamp.parse().ok()?, sequence.parse().ok()?))
}

pub(crate) fn parse_owned_interrupted_archive_name(name: &str) -> Option<(u64, u16)> {
    let stem = name
        .strip_prefix("config-interrupted-stale-")
        .or_else(|| name.strip_prefix("config-interrupted-invalid-"))
        .and_then(|name| name.strip_suffix(".bin"))?;
    let (timestamp, sequence) = stem.split_once('-')?;
    if timestamp.len() != 20
        || sequence.len() != 5
        || !timestamp.bytes().all(|byte| byte.is_ascii_digit())
        || !sequence.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some((timestamp.parse().ok()?, sequence.parse().ok()?))
}

pub(crate) fn config_temp_path(config: &Path) -> PathBuf {
    config.with_extension("json.tmp")
}

pub(crate) fn inspect_config_file(path: &Path) -> Result<ConfigFileStatus, ConfigError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(ConfigFileStatus::Missing);
        }
        Err(error) => return Err(error.into()),
    };
    match parse_config(&bytes) {
        Ok(_) => Ok(ConfigFileStatus::Valid),
        Err(ConfigError::UnsupportedSchema(version)) => {
            Ok(ConfigFileStatus::UnsupportedSchema(version))
        }
        Err(_) => Ok(ConfigFileStatus::Invalid),
    }
}

pub(crate) fn write_config_atomic(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    write_config_atomic_with_hook(path, bytes, |_| Ok(()))
}

pub(crate) fn write_config_atomic_with_hook(
    path: &Path,
    bytes: &[u8],
    mut hook: impl FnMut(ConfigWriteStage) -> Result<(), ConfigError>,
) -> Result<(), ConfigError> {
    let previous = match fs::read(path) {
        Ok(previous) => Some(previous),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let temp_path = config_temp_path(path);
    let mut replaced = false;
    let mut temp_created = false;
    let result = (|| -> Result<(), ConfigError> {
        match fs::symlink_metadata(&temp_path) {
            Ok(_) => return Err(ConfigError::WriteTargetOccupied),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        hook(ConfigWriteStage::BeforeTempCreate)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        set_private_file(&file)?;
        temp_created = true;
        hook(ConfigWriteStage::AfterTempCreate)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        write_atomic(path, bytes)?;
        replaced = true;
        hook(ConfigWriteStage::AfterReplace)?;
        fs::remove_file(&temp_path)?;
        Ok(())
    })();
    if let Err(error) = result {
        if replaced {
            restore_config_bytes(path, previous.as_deref())?;
        } else if temp_created {
            match fs::remove_file(&temp_path) {
                Ok(()) => {}
                Err(remove_error) if remove_error.kind() == ErrorKind::NotFound => {}
                Err(remove_error) => return Err(remove_error.into()),
            }
        }
        return Err(error);
    }
    Ok(())
}

pub(crate) fn restore_config_bytes(
    path: &Path,
    previous: Option<&[u8]>,
) -> Result<(), ConfigError> {
    match previous {
        Some(bytes) => write_atomic(path, bytes),
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        },
    }
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    bongocat_storage::write_private_atomic(path, bytes).map_err(ConfigError::from)
}

pub(crate) fn parse_config(bytes: &[u8]) -> Result<(NativeConfig, ConfigRevision), ConfigError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .ok_or(ConfigError::InvalidValue("schema_version"))?;
    if schema_version != SCHEMA_VERSION {
        return Err(ConfigError::UnsupportedSchema(schema_version));
    }
    let model = value
        .get("model")
        .and_then(serde_json::Value::as_object)
        .ok_or(ConfigError::InvalidValue("model"))?;
    if !model.contains_key("selected_model") {
        return Err(ConfigError::InvalidValue("model.selected_model"));
    }
    let config: NativeConfig = serde_json::from_value(value)?;
    config.validate()?;
    let normalized = serde_json::to_vec_pretty(&config)?;
    let revision = revision_for_bytes(&normalized);
    Ok((config, revision))
}

pub(crate) fn revision_for_bytes(bytes: &[u8]) -> ConfigRevision {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    ConfigRevision::from_hash(hash)
}

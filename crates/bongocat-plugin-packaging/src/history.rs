use crate::{Error, Result, arg, catalog::read_state};
use std::path::Path;

pub fn verify_command(args: &[String]) -> Result<()> {
    crate::reject_unknown(args, &["--catalog", "--state", "--public-key"], &[])?;
    let catalog = Path::new(
        arg(args, "--catalog")?.ok_or_else(|| Error::Usage("verify requires --catalog".into()))?,
    );
    let state = Path::new(
        arg(args, "--state")?.ok_or_else(|| Error::Usage("verify requires --state".into()))?,
    );
    let public_key = std::fs::read_to_string(
        arg(args, "--public-key")?
            .ok_or_else(|| Error::Usage("verify requires --public-key".into()))?,
    )?;
    let bytes = std::fs::read(catalog)?;
    let parsed = crate::signing::verify_catalog(&bytes, &public_key)?;
    let release_state = read_state(&state.join(crate::catalog::STATE_NAME))?;
    let known = release_state
        .plugins
        .iter()
        .chain(release_state.retired.iter())
        .collect::<Vec<_>>();
    for plugin in known {
        let rows = parsed
            .plugins
            .iter()
            .filter(|entry| entry.id == plugin.id && entry.version == plugin.version);
        if rows.clone().count() != crate::PLATFORMS.len() {
            return Err(Error::Invalid(format!(
                "{} has an incomplete platform matrix",
                plugin.id
            )));
        }
        let mut platforms = std::collections::BTreeSet::new();
        for row in rows {
            if row.downloads.len() != 1
                || row.sha256.as_ref().is_none_or(|digest| {
                    digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            {
                return Err(Error::Invalid(format!(
                    "{} has an invalid catalog row",
                    plugin.id
                )));
            }
            let platform = crate::Platform::parse(
                row.downloads
                    .keys()
                    .next()
                    .ok_or_else(|| Error::Invalid("catalog row has no platform".into()))?,
            )?;
            if !platforms.insert(platform) {
                return Err(Error::Invalid("catalog has duplicate platform rows".into()));
            }
            let asset = plugin
                .packages
                .get(platform.key())
                .ok_or_else(|| Error::Invalid("state is missing a platform package".into()))?;
            let expected = row
                .sha256
                .as_ref()
                .ok_or_else(|| Error::Invalid("catalog row has no digest".into()))?;
            if expected != &asset.sha256 {
                return Err(Error::Invalid("catalog and state digests disagree".into()));
            }
            if row
                .downloads
                .values()
                .next()
                .is_none_or(|value| crate::catalog::catalog_asset_name(value) != asset.asset)
            {
                return Err(Error::Invalid("catalog and state assets disagree".into()));
            }
        }
        if platforms.len() != crate::PLATFORMS.len() {
            return Err(Error::Invalid(format!(
                "{} has an incomplete platform matrix",
                plugin.id
            )));
        }
    }
    if parsed.plugins.iter().any(|entry| {
        !release_state
            .plugins
            .iter()
            .any(|plugin| plugin.id == entry.id && plugin.version == entry.version)
    }) {
        return Err(Error::Invalid(
            "catalog contains rows outside the current release state".into(),
        ));
    }
    if release_state.plugins.is_empty() {
        return Err(Error::Invalid(
            "release state has no current plugins".into(),
        ));
    }
    Ok(())
}

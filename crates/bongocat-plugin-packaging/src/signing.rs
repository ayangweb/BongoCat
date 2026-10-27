use crate::{Error, Result, arg};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::io::Write;
use std::path::Path;

pub fn keygen(args: &[String]) -> Result<()> {
    crate::reject_unknown(args, &["--private", "--public"], &[])?;
    let private = Path::new(
        arg(args, "--private")?.ok_or_else(|| Error::Usage("keygen requires --private".into()))?,
    );
    let public = Path::new(
        arg(args, "--public")?.ok_or_else(|| Error::Usage("keygen requires --public".into()))?,
    );
    if private.exists() || public.exists() {
        return Err(Error::Usage("refusing to overwrite an existing key".into()));
    }
    let keys = minisign::KeyPair::generate_encrypted_keypair(Some(
        std::env::var("PLUGIN_SIGNING_KEY_PASSWORD").unwrap_or_default(),
    ))?;
    let public_box = keys.pk.to_box()?.to_string();
    let private_box = keys
        .sk
        .to_box(Some("BongoCat plugin release key"))?
        .to_string();
    write_private(private, STANDARD.encode(private_box).as_bytes())?;
    fs_write(public, STANDARD.encode(public_box).as_bytes())?;
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn fs_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub fn sign_catalog(canonical: &[u8], private_key: &str) -> Result<String> {
    let decoded = STANDARD
        .decode(private_key.trim())
        .map_err(|_| Error::Invalid("plugin signing key is not valid base64".into()))?;
    let boxed = minisign::SecretKeyBox::from_string(std::str::from_utf8(&decoded)?)?;
    let secret = boxed.into_secret_key(Some(
        std::env::var("PLUGIN_SIGNING_KEY_PASSWORD").unwrap_or_default(),
    ))?;
    let signature = minisign::sign(
        None,
        &secret,
        std::io::Cursor::new(canonical),
        Some("BongoCat plugin catalog"),
        Some("signature from BongoCat plugin release key"),
    )?;
    Ok(STANDARD.encode(signature.to_string()))
}

pub fn verify_catalog(bytes: &[u8], public_key: &str) -> Result<crate::catalog::Catalog> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Signed {
        catalog: crate::catalog::Catalog,
        signature: String,
    }
    let signed: Signed = serde_json::from_slice(bytes)?;
    let decoded_key = STANDARD
        .decode(public_key.trim())
        .map_err(|_| Error::Invalid("plugin public key is not valid base64".into()))?;
    let key = minisign::PublicKey::from_box(minisign::PublicKeyBox::from_string(
        std::str::from_utf8(&decoded_key)?,
    )?)?;
    let decoded_signature = STANDARD
        .decode(&signed.signature)
        .map_err(|_| Error::Invalid("plugin catalog signature is not valid base64".into()))?;
    let signature = minisign::SignatureBox::from_string(std::str::from_utf8(&decoded_signature)?)?;
    let catalog_bytes = serde_json::to_vec(&signed.catalog)?;
    minisign::verify(
        &key,
        &signature,
        std::io::Cursor::new(catalog_bytes),
        true,
        false,
        false,
    )?;
    Ok(signed.catalog)
}

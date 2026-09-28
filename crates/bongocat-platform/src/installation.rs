use std::path::{Path, PathBuf};

/// The product-file root managed by a platform installer or update helper.
///
/// This intentionally has no build environment: installers must never derive
/// their product-file paths from, or operate on, application user data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallationLayout {
    product_files_directory: PathBuf,
}

impl InstallationLayout {
    pub fn new(product_files_directory: impl Into<PathBuf>) -> Self {
        Self {
            product_files_directory: product_files_directory.into(),
        }
    }

    pub fn product_files_directory(&self) -> &Path {
        &self.product_files_directory
    }
}

#[cfg(test)]
mod tests {
    use super::InstallationLayout;
    use std::path::Path;

    #[test]
    fn installation_layout_only_exposes_the_product_file_root() {
        let layout = InstallationLayout::new("/opt/BongoCat");

        assert_eq!(layout.product_files_directory(), Path::new("/opt/BongoCat"));
    }
}

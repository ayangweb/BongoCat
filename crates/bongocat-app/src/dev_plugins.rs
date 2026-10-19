//! The source of available plugins is fixed by the build environment.
use bongocat_config::BuildEnvironment;
use bongocat_plugin_service::PluginSource;
use std::path::Path;

pub(crate) fn source() -> PluginSource {
    match bongocat_app::BUILD_ENVIRONMENT {
        BuildEnvironment::Development => {
            let packages = Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .expect("workspace root")
                .join("target/plugins");
            PluginSource::Development {
                catalog: packages.join("catalog.json"),
                packages,
            }
        }
        BuildEnvironment::Production => PluginSource::Production {
            owner: bongocat_update::RELEASE_REPOSITORY_OWNER.into(),
            repository: bongocat_update::RELEASE_REPOSITORY_NAME.into(),
            public_key: bongocat_update::RELEASE_SIGNING_KEY
                .unwrap_or_default()
                .into(),
            platform: bongocat_update::HOST_TARGET_TRIPLE
                .manifest_platform()
                .into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sources_are_fixed_and_environment_isolated() {
        match source() {
            PluginSource::Development { catalog, packages } => {
                assert!(catalog.ends_with(std::path::Path::new("target/plugins/catalog.json")));
                assert!(packages.ends_with("target/plugins"));
                assert!(
                    packages
                        .parent()
                        .unwrap()
                        .parent()
                        .unwrap()
                        .join("Cargo.toml")
                        .is_file()
                );
            }
            PluginSource::Production {
                platform,
                public_key,
                ..
            } => {
                assert!(!platform.is_empty());
                assert!(!public_key.is_empty());
            }
            PluginSource::InstalledOnly => panic!("product must discover available plugins"),
        }
    }
}

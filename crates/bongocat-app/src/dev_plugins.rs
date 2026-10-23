//! The source of available plugins is fixed by the build environment.
use bongocat_config::BuildEnvironment;
use bongocat_plugin_service::PluginSource;
use std::path::Path;

/// Development-only directory containing the independent repository's packages.
///
/// Production never reads a local directory. Development may name the output of
/// `BongoCat-Plugins`' packaging tool. With no override, `just dev` prepares the
/// repository clone and catalog under `target/plugin-dev`.
const DEVELOPMENT_DIRECTORY_VARIABLE: &str = "BONGOCAT_PLUGIN_DEV_DIRECTORY";

pub(crate) fn source() -> PluginSource {
    match bongocat_app::BUILD_ENVIRONMENT {
        BuildEnvironment::Development => {
            let packages = std::env::var_os(DEVELOPMENT_DIRECTORY_VARIABLE)
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| {
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .ancestors()
                        .nth(2)
                        .expect("workspace root")
                        .join("target/plugin-dev/plugins")
                });
            PluginSource::Development {
                catalog: packages.join("catalog.json"),
                packages,
            }
        }
        BuildEnvironment::Production => PluginSource::Production {
            owner: bongocat_update::PLUGIN_REPOSITORY_OWNER.into(),
            repository: bongocat_update::PLUGIN_REPOSITORY_NAME.into(),
            public_key: bongocat_update::PLUGIN_RELEASE_SIGNING_KEY.into(),
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
                assert!(catalog.ends_with(std::path::Path::new(
                    "target/plugin-dev/plugins/catalog.json"
                )));
                assert!(packages.ends_with("target/plugin-dev/plugins"));
            }
            PluginSource::Production {
                owner,
                repository,
                platform,
                public_key,
            } => {
                assert_eq!(owner, "ayangweb");
                assert_eq!(repository, "BongoCat-Plugins");
                assert!(!platform.is_empty());
                assert!(!public_key.is_empty());
                assert_ne!(
                    public_key,
                    bongocat_update::RELEASE_SIGNING_KEY.unwrap_or_default()
                );
            }
            PluginSource::InstalledOnly => panic!("product must discover available plugins"),
        }
    }

    #[test]
    fn the_development_plugin_directory_is_an_explicit_build_input() {
        assert_eq!(
            DEVELOPMENT_DIRECTORY_VARIABLE,
            "BONGOCAT_PLUGIN_DEV_DIRECTORY"
        );
        assert!(!DEVELOPMENT_DIRECTORY_VARIABLE.starts_with("BONGOCAT_APP_"));
    }
}

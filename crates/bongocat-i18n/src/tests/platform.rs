//! A per-platform key wins, and a key without one gets the base.

use super::*;

#[test]
fn current_platform_id_is_one_of_the_supported_platforms() {
    assert!(
        matches!(current_platform_id(), "macos" | "windows"),
        "platform id must be a known suffix, got {:?}",
        current_platform_id()
    );
}

#[test]
fn platform_text_picks_the_current_platform_override() {
    // The catalog always carries every supported override, so resolving
    // the platform-relative key returns the platform-specific copy on the
    // build host and the fallback copy on every other platform.
    let expected_override = match current_platform_id() {
        "macos" => text("en-US", "settings.app_system.status_icon.label.macos"),
        "windows" => text("en-US", "settings.app_system.status_icon.label.windows"),
        _ => text("en-US", "settings.app_system.status_icon.label"),
    };
    assert_eq!(
        platform_text("en-US", "settings.app_system.status_icon.label"),
        expected_override
    );
    assert_eq!(
        platform_text("zh-CN", "settings.app_system.status_icon.label"),
        match current_platform_id() {
            "macos" => text("zh-CN", "settings.app_system.status_icon.label.macos"),
            "windows" => text("zh-CN", "settings.app_system.status_icon.label.windows"),
            _ => text("zh-CN", "settings.app_system.status_icon.label"),
        }
    );
}

#[test]
fn platform_text_falls_back_to_the_base_key_when_no_override_exists() {
    // `navigation.settings.title` carries no `.macos` or `.windows`
    // override, so the helper must always return the shared string for
    // every supported platform id.
    let base_key = "navigation.settings.title";
    let platform_key = format!("{base_key}.{}", current_platform_id());
    assert_eq!(
        text("en-US", &platform_key),
        platform_key,
        "test premise: no platform override exists for {base_key}"
    );
    assert_eq!(platform_text("en-US", base_key), text("en-US", base_key));
    assert_eq!(platform_text("zh-CN", base_key), text("zh-CN", base_key));
}

#[test]
fn platform_text_is_safe_for_keys_without_a_platform_override_present() {
    // The fallback path is reachable even on supported platforms, so the
    // helper must keep returning the shared string rather than the
    // augmented key. This guards against a regression where the lookup
    // would start to return the suffixed key by accident.
    let base_key = "navigation.about.title";
    let resolved = platform_text("zh-CN", base_key);
    assert!(!resolved.ends_with(&format!(".{}", current_platform_id())));
}

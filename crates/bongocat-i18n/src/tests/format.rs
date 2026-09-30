//! Interpolation is the one thing a caller reaches that is not a lookup.

use super::*;

#[test]
fn format_text_interpolation_is_available() {
    assert_eq!(
        format_text(
            "en-US",
            "update.current_version",
            &[("version", "1.2.3".to_string())],
        ),
        "Current version 1.2.3"
    );
    assert_eq!(
        format_text(
            "zh-CN",
            "update.current_version",
            &[("version", "1.2.3".to_string())],
        ),
        "当前版本 1.2.3"
    );
    assert_eq!(
        format_text(
            "zh-TW",
            "update.current_version",
            &[("version", "1.2.3".to_string())],
        ),
        "目前版本 1.2.3"
    );
    assert_eq!(
        format_text(
            "ar-SA",
            "update.current_version",
            &[("version", "1.2.3".to_string())],
        ),
        "الإصدار الحالي 1.2.3"
    );
    assert_eq!(
        format_text(
            "vi-VN",
            "update.current_version",
            &[("version", "1.2.3".to_string())],
        ),
        "Phiên bản hiện tại 1.2.3"
    );
    assert_eq!(
        format_text(
            "pt-BR",
            "update.current_version",
            &[("version", "1.2.3".to_string())],
        ),
        "Versão atual 1.2.3"
    );
    assert_eq!(
        format_text(
            "ko-KR",
            "update.current_version",
            &[("version", "1.2.3".to_string())],
        ),
        "현재 버전 1.2.3"
    );
}

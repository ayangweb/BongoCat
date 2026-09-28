//! The window level the overlay presents at.

use super::*;

#[test]
fn main_window_level_tracks_always_on_top() {
    assert_eq!(main_window_level(true), NSMainMenuWindowLevel);
    assert_eq!(main_window_level(false), NSNormalWindowLevel);
}

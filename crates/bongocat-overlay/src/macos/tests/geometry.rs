//! The window level the overlay presents at.

use super::*;
use objc2_app_kit::{NSMainMenuWindowLevel, NSPopUpMenuWindowLevel};

#[test]
fn main_window_level_tracks_always_on_top() {
    assert_eq!(main_window_level(true), NSStatusWindowLevel);
    assert_eq!(main_window_level(false), NSNormalWindowLevel);
}

/// The always-on-top level has a neighbour on either side it must stay between.
///
/// Above the menu bar: `NSMainMenuWindowLevel` is the level macOS gives its own
/// menu bar, and the menu bar wins a tie, so an overlay at that level is drawn
/// behind it — the window can be dragged into the menu bar strip, but the part
/// inside it is invisible, which is issue #1155's "the cat cannot reach the top
/// of the screen". Below popup menus: AppKit opens `NSMenu` popups at
/// `NSPopUpMenuWindowLevel`, and the tray menu and the model window's own context
/// menu are both `NSMenu` popups that have to stay above the model window.
#[test]
fn the_always_on_top_level_is_above_the_menu_bar_and_below_popup_menus() {
    assert!(main_window_level(true) > NSMainMenuWindowLevel);
    assert!(main_window_level(true) < NSPopUpMenuWindowLevel);
}

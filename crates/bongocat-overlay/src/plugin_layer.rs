//! The plugin layer's place in the model window.
//!
//! Plugins draw inside the model window rather than in windows of their own, so
//! the window has to composite their panel and route a click that lands on one of
//! their controls back to the plugin that drew it. This module owns the portable
//! half of that: the interaction the window reports, and the hit test that decides
//! whether a click belongs to a plugin or to the window itself.
//!
//! The two native adapters own the rest — which window message or AppKit event
//! carries the click, and how to stop that click from also starting a window
//! drag.

use bongocat_plugin_hud::{HudInteraction, HudLayer, HudTarget};

/// A click the model window resolved to one of a plugin's controls.
///
/// It carries the plugin id and the widget id and nothing else: the plugin
/// decides what its own button means, so the application never learns what any
/// control does.
pub type OverlayPluginInteraction = HudInteraction;

/// The plugin control at a device-pixel point inside the model window, if any.
pub fn plugin_interaction_at(layer: &HudLayer, x: f32, y: f32) -> Option<OverlayPluginInteraction> {
    layer.target_at(x, y).map(|target| HudInteraction {
        plugin: target.plugin.clone(),
        id: target.id.clone(),
    })
}

/// The interactive controls of the current plugin layer.
///
/// A native callback needs this on its own thread and without the layer's pixels,
/// so the session hands it a copy of just the hit-test data whenever a new layer
/// arrives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OverlayPluginTargets {
    targets: Vec<HudTarget>,
    /// The window size the targets were resolved against. A point outside it
    /// never hits a control, and a window that has since been resized has no
    /// valid targets at all.
    pub(crate) window_width: u32,
    pub(crate) window_height: u32,
}

impl OverlayPluginTargets {
    /// Build the hit-test data from a layer, dropping one that no longer matches
    /// the window it was composed for.
    pub fn from_layer(layer: &HudLayer, window_width: u32, window_height: u32) -> Self {
        if layer.window_width != window_width || layer.window_height != window_height {
            return Self::default();
        }
        Self {
            targets: layer.targets.clone(),
            window_width,
            window_height,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// The plugin control at a device-pixel point inside the model window.
    pub fn interaction_at(&self, x: f32, y: f32) -> Option<OverlayPluginInteraction> {
        if x < 0.0 || y < 0.0 || x >= self.window_width as f32 || y >= self.window_height as f32 {
            return None;
        }
        self.targets
            .iter()
            .rev()
            .find(|target| target.rect.contains(x, y))
            .map(|target| HudInteraction {
                plugin: target.plugin.clone(),
                id: target.id.clone(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_plugin_hud::HudRect;

    fn layer_with(rect: HudRect) -> HudLayer {
        HudLayer {
            width: 200,
            height: 100,
            pixels: vec![0; 200 * 100 * 4],
            origin_x: 0.0,
            origin_y: 0.0,
            window_width: 350,
            window_height: 350,
            targets: vec![HudTarget {
                plugin: "com.bongocat.countdown".to_owned(),
                id: "toggle".to_owned(),
                rect,
            }],
        }
    }

    #[test]
    fn a_click_inside_a_control_belongs_to_the_plugin_that_drew_it() {
        let layer = layer_with(HudRect::new(10.0, 10.0, 72.0, 24.0));
        let interaction = plugin_interaction_at(&layer, 20.0, 20.0).expect("hits the button");
        assert_eq!(interaction.plugin, "com.bongocat.countdown");
        assert_eq!(interaction.id, "toggle");
    }

    #[test]
    fn a_click_outside_every_control_stays_with_the_window() {
        let layer = layer_with(HudRect::new(10.0, 10.0, 72.0, 24.0));
        assert!(plugin_interaction_at(&layer, 200.0, 200.0).is_none());
        assert!(plugin_interaction_at(&layer, 9.0, 20.0).is_none());
    }

    #[test]
    fn a_layer_composed_for_another_window_size_has_no_targets() {
        let layer = layer_with(HudRect::new(10.0, 10.0, 72.0, 24.0));
        let matching = OverlayPluginTargets::from_layer(&layer, 350, 350);
        assert!(!matching.is_empty());
        assert!(matching.interaction_at(20.0, 20.0).is_some());

        let stale = OverlayPluginTargets::from_layer(&layer, 700, 350);
        assert!(stale.is_empty());
        assert!(stale.interaction_at(20.0, 20.0).is_none());
    }

    #[test]
    fn a_point_outside_the_window_never_hits_a_control() {
        let layer = layer_with(HudRect::new(340.0, 10.0, 72.0, 24.0));
        let targets = OverlayPluginTargets::from_layer(&layer, 350, 350);
        assert!(targets.interaction_at(360.0, 20.0).is_none());
        assert!(targets.interaction_at(-1.0, 20.0).is_none());
    }
}

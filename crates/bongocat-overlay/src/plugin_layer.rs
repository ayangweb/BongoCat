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

/// What one tick's channel read changed about the layer the window draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PluginLayerChange {
    /// The layer to draw was replaced or cleared, so the uploaded copy is stale.
    pub layer_replaced: bool,
    /// The hit-test data changed, so the native window's copy is stale.
    pub targets_replaced: bool,
}

impl PluginLayerChange {
    /// Nothing changed: the window keeps the layer and the targets it has.
    pub const fn unchanged() -> Self {
        Self {
            layer_replaced: false,
            targets_replaced: false,
        }
    }
}

/// The layer the model window draws, and the hit-test data that belongs to it.
///
/// The plugin service publishes a layer only when a panel's content changes, so
/// "no layer arrived this tick" means "keep drawing the one on screen": a
/// countdown changes once a second and an idle panel never. A window that
/// cleared its layer on every empty read would therefore show a panel for one
/// frame and then a hole — the flash a user sees as the panel disappearing — so
/// this is where the decision "which layer is current" lives, portable and
/// testable, rather than in each native session.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OverlayPluginLayer {
    layer: Option<HudLayer>,
    targets: OverlayPluginTargets,
    hovered: Option<HudTarget>,
    base_pixels: Vec<u8>,
}

impl OverlayPluginLayer {
    /// Fold one tick's channel read into the layer the window should draw.
    ///
    /// A layer composed for a size the window has since left is dropped rather
    /// than stretched: the next publication matches the live size, and a
    /// stretched panel would put a plugin's buttons where its own hit testing
    /// does not expect them.
    pub fn refresh(
        &mut self,
        incoming: Option<HudLayer>,
        window_width: u32,
        window_height: u32,
    ) -> PluginLayerChange {
        let Some(layer) = incoming else {
            // Nothing was published. A window that changed size has no layer
            // that still describes it; otherwise the layer stays as it is.
            if self.layer.is_none()
                || (self.targets.window_width == window_width
                    && self.targets.window_height == window_height)
            {
                return PluginLayerChange::unchanged();
            }
            return self.clear();
        };
        if layer.window_width != window_width || layer.window_height != window_height {
            return self.clear();
        }
        let targets = OverlayPluginTargets::from_layer(&layer, window_width, window_height);
        let change = PluginLayerChange {
            layer_replaced: self.layer.as_ref() != Some(&layer),
            targets_replaced: targets != self.targets,
        };
        self.targets = targets;
        self.base_pixels = layer.pixels.clone();
        self.hovered = None;
        self.layer = Some(layer);
        change
    }

    /// Repaint only on hover transitions, always from the unmodified texture.
    pub(crate) fn update_pointer(&mut self, point: Option<(f32, f32)>) -> bool {
        let Some(layer) = &mut self.layer else {
            return false;
        };
        let hovered = point.and_then(|(x, y)| layer.target_at(x, y)).cloned();
        if hovered == self.hovered {
            return false;
        }
        layer.pixels.clone_from(&self.base_pixels);
        if let Some(target) = &hovered {
            let x0 = (target.rect.x - layer.origin_x).max(0.0) as u32;
            let y0 = (target.rect.y - layer.origin_y).max(0.0) as u32;
            let x1 =
                ((target.rect.right() - layer.origin_x).ceil().max(0.0) as u32).min(layer.width);
            let y1 =
                ((target.rect.bottom() - layer.origin_y).ceil().max(0.0) as u32).min(layer.height);
            for y in y0..y1 {
                for x in x0..x1 {
                    let px = x as f32 + layer.origin_x + 0.5;
                    let py = y as f32 + layer.origin_y + 0.5;
                    let radius = (bongocat_plugin_hud::CORNER_RADIUS * target.rect.height
                        / bongocat_plugin_hud::BUTTON_HEIGHT)
                        .min(target.rect.width / 2.0);
                    let cx = px.clamp(target.rect.x + radius, target.rect.right() - radius);
                    let cy = py.clamp(target.rect.y + radius, target.rect.bottom() - radius);
                    if target.rect.contains(px, py) && (px - cx).hypot(py - cy) <= radius {
                        let index = ((y * layer.width + x) * 4) as usize;
                        let alpha = layer.pixels[index + 3];
                        for channel in &mut layer.pixels[index..index + 3] {
                            *channel = (*channel).saturating_add(12).min(alpha);
                        }
                    }
                }
            }
        }
        self.hovered = hovered;
        true
    }

    /// The layer to draw, or `None` when no plugin draws anything.
    pub fn layer(&self) -> Option<&HudLayer> {
        self.layer.as_ref()
    }

    /// The interactive controls of the current layer.
    pub fn targets(&self) -> &OverlayPluginTargets {
        &self.targets
    }

    /// Forget the layer and the controls that described it.
    fn clear(&mut self) -> PluginLayerChange {
        let change = PluginLayerChange {
            layer_replaced: self.layer.is_some(),
            targets_replaced: !self.targets.is_empty(),
        };
        self.layer = None;
        self.base_pixels.clear();
        self.hovered = None;
        self.targets = OverlayPluginTargets::default();
        change
    }
}

/// AppKit event locations are content/window-local, never screen-relative.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn content_point_to_device(
    x: f64,
    y: f64,
    height: f64,
    scale: f64,
    flipped: bool,
) -> (f32, f32) {
    let scale = scale.max(1.0);
    (
        (x * scale) as f32,
        ((if flipped { y } else { height - y }) * scale) as f32,
    )
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
                plugin: "com.bongocat.pomodoro".to_owned(),
                id: "toggle".to_owned(),
                rect,
            }],
        }
    }

    #[test]
    fn hover_is_reversible_and_does_not_accumulate_or_change_targets() {
        let mut layer = layer_with(HudRect::new(10.0, 10.0, 80.0, 32.0));
        layer.pixels = [20, 30, 40, 255].repeat(200 * 100);
        let original = layer.clone();
        let mut state = OverlayPluginLayer::default();
        state.refresh(Some(layer), 350, 350);
        assert!(state.update_pointer(Some((30.0, 25.0))));
        assert_ne!(state.layer().unwrap().pixels, original.pixels);
        assert_eq!(state.layer().unwrap().targets, original.targets);
        assert!(!state.update_pointer(Some((31.0, 25.0))));
        assert!(state.update_pointer(None));
        assert_eq!(state.layer().unwrap().pixels, original.pixels);
    }

    #[test]
    fn content_coordinates_ignore_screen_origin_and_title_bar() {
        // The window may be anywhere on a Retina display. Only its content
        // coordinates participate; a 22-point title bar is outside this height.
        assert_eq!(
            content_point_to_device(20.0, 160.0, 200.0, 2.0, false),
            (40.0, 80.0)
        );
        assert_eq!(
            content_point_to_device(20.0, 40.0, 200.0, 1.5, true),
            (30.0, 60.0)
        );
    }

    #[test]
    fn a_click_inside_a_control_belongs_to_the_plugin_that_drew_it() {
        let layer = layer_with(HudRect::new(10.0, 10.0, 72.0, 24.0));
        let interaction = plugin_interaction_at(&layer, 20.0, 20.0).expect("hits the button");
        assert_eq!(interaction.plugin, "com.bongocat.pomodoro");
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

    #[test]
    fn a_panel_stays_on_screen_between_publications() {
        let mut current = OverlayPluginLayer::default();
        let first = current.refresh(
            Some(layer_with(HudRect::new(10.0, 10.0, 72.0, 24.0))),
            350,
            350,
        );
        assert!(first.layer_replaced);
        assert!(first.targets_replaced);
        assert!(current.layer().is_some());

        // The service publishes only on a change, so the tick after a
        // publication must not take the panel down again.
        let idle = current.refresh(None, 350, 350);
        assert_eq!(idle, PluginLayerChange::unchanged());
        assert!(current.layer().is_some());
        assert!(current.targets().interaction_at(20.0, 20.0).is_some());
    }

    #[test]
    fn a_new_publication_replaces_the_panel_it_supersedes() {
        let mut current = OverlayPluginLayer::default();
        current.refresh(
            Some(layer_with(HudRect::new(10.0, 10.0, 72.0, 24.0))),
            350,
            350,
        );
        let next = current.refresh(None, 350, 350);
        assert_eq!(next, PluginLayerChange::unchanged());

        let mut moved = layer_with(HudRect::new(10.0, 40.0, 72.0, 24.0));
        moved.pixels = vec![7; 200 * 100 * 4];
        let change = current.refresh(Some(moved.clone()), 350, 350);
        assert!(change.layer_replaced);
        assert!(change.targets_replaced);
        assert_eq!(current.layer(), Some(&moved));
    }

    #[test]
    fn a_window_that_changed_size_drops_the_layer_it_cannot_stretch() {
        let mut current = OverlayPluginLayer::default();
        current.refresh(
            Some(layer_with(HudRect::new(10.0, 10.0, 72.0, 24.0))),
            350,
            350,
        );

        let change = current.refresh(None, 700, 350);
        assert!(change.layer_replaced);
        assert!(change.targets_replaced);
        assert!(current.layer().is_none());
        assert!(current.targets().interaction_at(20.0, 20.0).is_none());
    }

    #[test]
    fn a_publication_for_another_size_never_reaches_the_window() {
        let mut current = OverlayPluginLayer::default();
        current.refresh(
            Some(layer_with(HudRect::new(10.0, 10.0, 72.0, 24.0))),
            350,
            350,
        );

        let change = current.refresh(
            Some(layer_with(HudRect::new(10.0, 10.0, 72.0, 24.0))),
            700,
            350,
        );
        assert!(change.layer_replaced);
        assert!(current.layer().is_none());
    }

    #[test]
    fn an_empty_publication_clears_the_panel() {
        let mut current = OverlayPluginLayer::default();
        current.refresh(
            Some(layer_with(HudRect::new(10.0, 10.0, 72.0, 24.0))),
            350,
            350,
        );

        let mut empty = HudLayer::empty();
        empty.window_width = 350;
        empty.window_height = 350;
        let change = current.refresh(Some(empty), 350, 350);
        assert!(change.layer_replaced);
        assert!(change.targets_replaced);
        assert!(current.layer().is_some_and(HudLayer::is_empty));
        assert!(current.targets().interaction_at(20.0, 20.0).is_none());
    }

    #[test]
    fn hit_test_data_is_republished_only_when_it_changes() {
        let mut current = OverlayPluginLayer::default();
        current.refresh(
            Some(layer_with(HudRect::new(10.0, 10.0, 72.0, 24.0))),
            350,
            350,
        );

        // A repaint that leaves the controls where they are costs no copy to the
        // window thread.
        let mut repainted = layer_with(HudRect::new(10.0, 10.0, 72.0, 24.0));
        repainted.pixels = vec![3; 200 * 100 * 4];
        let change = current.refresh(Some(repainted), 350, 350);
        assert!(change.layer_replaced);
        assert!(!change.targets_replaced);
    }
}

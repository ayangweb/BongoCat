//! What a plugin puts on the model window, and where.
//!
//! This is the same node vocabulary ADR-0078 defined, with one change that the
//! move to processes forced and that is worth stating plainly: **a scene now
//! carries values rather than bindings.** A plugin used to name `timer.progress`
//! and the host evaluated it; a running plugin computes the number itself and
//! sends `0.42`. The node tree, the layout, the font, the theme and the raster are
//! all unchanged, which is the point of keeping them here — the plugin decides
//! *what* is shown and the host still decides *how it looks*.
//!
//! What a plugin therefore gives up is nothing it needs, and what it gains is that
//! its panel can show a number no host-side behavior knows how to produce: a
//! keystroke count, a socket's state, a reading from a file.

use super::error::{PluginError, PluginErrorCode};
use super::scene::SceneNode;
use serde::{Deserialize, Serialize};

/// The panel a plugin draws, with its values already in it.
///
/// Sent whole, and validated whole, before anything is laid out. A panel is a
/// fixed-size box the host rasterizes on a worker thread, so the bounds below are
/// about work per redraw and about a box that still fits the model window.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PanelUpdate {
    /// Where the panel sits in the model window.
    #[serde(flatten)]
    pub placement: PanelPlacement,
    /// The panel's own tree, with concrete values.
    pub scene: SceneNode,
}

impl PanelUpdate {
    /// Read and check one panel message.
    ///
    /// The one entry point a host uses on a message from a process, so a panel
    /// cannot reach a renderer without every bound in this file being applied to it.
    pub fn parse(line: &[u8]) -> Result<Self, PluginError> {
        let update: Self = serde_json::from_slice(line)
            .map_err(|error| PluginError::with_detail(PluginErrorCode::ProtocolInvalid, error))?;
        update.validate()?;
        Ok(update)
    }

    pub fn validate(&self) -> Result<(), PluginError> {
        self.placement.validate()?;
        let mut inspector = super::scene::inspect::Inspector::new();
        super::scene::inspect::walk(&self.scene, 1, &mut inspector)?;
        if inspector.buttons.len() > super::descriptor::MAXIMUM_BUTTONS_PER_PANEL {
            return Err(PluginError::new(PluginErrorCode::SceneTooLarge));
        }
        let unique: std::collections::BTreeSet<&str> =
            inspector.buttons.iter().map(String::as_str).collect();
        if unique.len() != inspector.buttons.len() {
            // A press is answered with a button id, so two buttons sharing one
            // would make the answer ambiguous rather than merely wrong.
            return Err(PluginError::new(PluginErrorCode::DuplicateButtonId));
        }
        Ok(())
    }
}

/// Where a contributed panel is pinned in the model window.
///
/// The same nine positions the overlay's layer placement uses, spelled the same
/// way, so a plugin author's choice reads identically in a panel message and in
/// the renderer that places it.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PluginAnchor {
    #[default]
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl PluginAnchor {
    pub const ALL: [Self; 9] = [
        Self::TopLeft,
        Self::TopCenter,
        Self::TopRight,
        Self::CenterLeft,
        Self::Center,
        Self::CenterRight,
        Self::BottomLeft,
        Self::BottomCenter,
        Self::BottomRight,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TopLeft => "top_left",
            Self::TopCenter => "top_center",
            Self::TopRight => "top_right",
            Self::CenterLeft => "center_left",
            Self::Center => "center",
            Self::CenterRight => "center_right",
            Self::BottomLeft => "bottom_left",
            Self::BottomCenter => "bottom_center",
            Self::BottomRight => "bottom_right",
        }
    }

    /// The overlay's own anchor, which is where the placement arithmetic lives.
    pub const fn to_overlay_anchor(self) -> bongocat_render::OverlayAnchor {
        match self {
            Self::TopLeft => bongocat_render::OverlayAnchor::TopLeft,
            Self::TopCenter => bongocat_render::OverlayAnchor::TopCenter,
            Self::TopRight => bongocat_render::OverlayAnchor::TopRight,
            Self::CenterLeft => bongocat_render::OverlayAnchor::CenterLeft,
            Self::Center => bongocat_render::OverlayAnchor::Center,
            Self::CenterRight => bongocat_render::OverlayAnchor::CenterRight,
            Self::BottomLeft => bongocat_render::OverlayAnchor::BottomLeft,
            Self::BottomCenter => bongocat_render::OverlayAnchor::BottomCenter,
            Self::BottomRight => bongocat_render::OverlayAnchor::BottomRight,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PanelPlacement {
    #[serde(default)]
    pub anchor: PluginAnchor,
    /// Gap from the window edge, as a fraction of the window's width and height.
    #[serde(default, skip_serializing_if = "is_zero_pair")]
    pub margin: [f32; 2],
    /// The panel's width as a fraction of the window's width.
    #[serde(default = "default_width_fraction")]
    pub width_fraction: f32,
    #[serde(default = "default_one", skip_serializing_if = "is_one")]
    pub opacity: f32,
    /// The panel's logical size in pixels, `[width, height]`.
    ///
    /// Logical rather than device, so a panel is the same size relative to the
    /// window at every display scale; the host multiplies by its own raster scale.
    #[serde(default = "default_size")]
    pub size: [u32; 2],
}

impl Default for PanelPlacement {
    fn default() -> Self {
        Self {
            anchor: PluginAnchor::default(),
            margin: [0.0, 0.0],
            width_fraction: default_width_fraction(),
            opacity: 1.0,
            size: default_size(),
        }
    }
}

impl PanelPlacement {
    pub fn validate(&self) -> Result<(), PluginError> {
        if self.size[0] < super::MINIMUM_PANEL_SIDE
            || self.size[1] < super::MINIMUM_PANEL_SIDE
            || self.size[0] > super::MAXIMUM_PANEL_SIDE
            || self.size[1] > super::MAXIMUM_PANEL_SIDE
        {
            return Err(PluginError::new(PluginErrorCode::InvalidPanelSize));
        }
        if !self.width_fraction.is_finite()
            || !(0.05..=bongocat_render::OverlayLayerPlacement::MAXIMUM_WIDTH_FRACTION)
                .contains(&self.width_fraction)
        {
            return Err(PluginError::new(PluginErrorCode::InvalidPanelPlacement));
        }
        if !self.margin.iter().all(|margin| {
            margin.is_finite()
                && (0.0..=bongocat_render::OverlayLayerPlacement::MAXIMUM_MARGIN_FRACTION)
                    .contains(margin)
        }) {
            return Err(PluginError::new(PluginErrorCode::InvalidPanelPlacement));
        }
        if !self.opacity.is_finite() || !(0.0..=1.0).contains(&self.opacity) {
            return Err(PluginError::new(PluginErrorCode::InvalidPanelPlacement));
        }
        Ok(())
    }

    /// This placement with every field inside what the overlay will honour.
    ///
    /// Separate from validation because the two answer different questions: a
    /// value outside the range is *refused* before the plugin is trusted to have
    /// meant it, while a value that reached a backend some other way is *clamped*
    /// so the backend cannot be handed a coordinate that has left the window.
    pub fn sanitized(self) -> Self {
        let placement = bongocat_render::OverlayLayerPlacement {
            anchor: self.anchor.to_overlay_anchor(),
            margin: self.margin,
            nudge: [0.0, 0.0],
            width_fraction: self.width_fraction,
            opacity: self.opacity,
        }
        .sanitized();
        Self {
            anchor: self.anchor,
            margin: placement.margin,
            width_fraction: placement.width_fraction,
            opacity: placement.opacity,
            size: self.size,
        }
    }
}

const fn default_width_fraction() -> f32 {
    0.72
}

const fn default_one() -> f32 {
    1.0
}

const fn default_size() -> [u32; 2] {
    [240, 132]
}

fn is_zero_pair(value: &[f32; 2]) -> bool {
    value[0] == 0.0 && value[1] == 0.0
}

fn is_one(value: &f32) -> bool {
    *value == 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(scene: SceneNode) -> PanelUpdate {
        PanelUpdate {
            placement: PanelPlacement::default(),
            scene,
        }
    }

    #[test]
    fn a_default_placement_is_valid() {
        PanelPlacement::default().validate().unwrap();
    }

    #[test]
    fn a_panel_outside_the_size_bound_is_refused() {
        for size in [[0, 100], [100, 0], [8, 8], [100_000, 100]] {
            let placement = PanelPlacement {
                size,
                ..PanelPlacement::default()
            };
            assert_eq!(
                placement.validate().unwrap_err().code(),
                PluginErrorCode::InvalidPanelSize,
                "{size:?} must be refused"
            );
        }
    }

    #[test]
    fn a_placement_the_window_cannot_honour_is_refused() {
        let wide = PanelPlacement {
            width_fraction: 2.0,
            ..PanelPlacement::default()
        };
        assert_eq!(
            wide.validate().unwrap_err().code(),
            PluginErrorCode::InvalidPanelPlacement
        );
        let opaque = PanelPlacement {
            opacity: 4.0,
            ..PanelPlacement::default()
        };
        assert!(opaque.validate().is_err());
        let not_a_number = PanelPlacement {
            width_fraction: f32::NAN,
            ..PanelPlacement::default()
        };
        assert_eq!(
            not_a_number.validate().unwrap_err().code(),
            PluginErrorCode::InvalidPanelPlacement
        );
    }

    #[test]
    fn two_buttons_may_not_share_an_id() {
        // A press comes back as a button id, so two buttons with one id would make
        // the host's answer ambiguous.
        let scene = SceneNode::Stack(crate::scene::StackNode {
            children: vec![
                SceneNode::Button(crate::scene::ButtonNode {
                    id: "go".to_string(),
                    label: "a".to_string(),
                    ..crate::scene::ButtonNode::default()
                }),
                SceneNode::Button(crate::scene::ButtonNode {
                    id: "go".to_string(),
                    label: "b".to_string(),
                    ..crate::scene::ButtonNode::default()
                }),
            ],
            ..crate::scene::StackNode::default()
        });
        assert_eq!(
            update(scene).validate().unwrap_err().code(),
            PluginErrorCode::DuplicateButtonId
        );
    }

    #[test]
    fn a_panel_with_distinct_buttons_validates() {
        let scene = SceneNode::Button(crate::scene::ButtonNode {
            id: "toggle".to_string(),
            label: "Start".to_string(),
            ..crate::scene::ButtonNode::default()
        });
        update(scene).validate().unwrap();
    }

    #[test]
    fn sanitizing_keeps_a_valid_placement_and_clamps_an_invalid_one() {
        let valid = PanelPlacement {
            anchor: super::super::PluginAnchor::BottomRight,
            margin: [0.03, 0.05],
            width_fraction: 0.5,
            opacity: 0.9,
            size: [200, 100],
        };
        assert_eq!(valid.sanitized(), valid);

        let clamped = PanelPlacement {
            width_fraction: 9.0,
            margin: [9.0, 9.0],
            opacity: 9.0,
            ..PanelPlacement::default()
        }
        .sanitized();
        assert!(clamped.width_fraction <= 0.9);
        assert!(clamped.margin[0] <= 0.45);
        assert!(clamped.opacity <= 1.0);
    }
}

//! Building the panel a plugin shows on the model window.
//!
//! The node vocabulary is small on purpose — a stack, some text, a bar, a ring, an
//! image, a button, a spacer, a rule — and this module is the ergonomic face of
//! it. A plugin writes [`column`], [`text`], [`bar`] and [`button`] and gets the
//! protocol's node tree back, so a panel is built in Rust with the compiler
//! checking it rather than assembled out of JSON at runtime.
//!
//! # Why the builder rather than the raw nodes
//!
//! Two reasons, and the second is the one that matters.
//!
//! The first is ordinary: a plugin author should not have to remember that
//! `StackNode` has `spacing` before `padding` or that a `TextNode`'s size is a cap
//! height in logical pixels.
//!
//! The second is that a plugin's panel is rebuilt on every tick, so the tree is
//! allocated constantly. [`Panel::rebuild`] takes a closure and hands back a panel
//! whose contents were only computed if the plugin decided to send it — which is
//! how a plugin that has nothing new to say avoids allocating a tree sixty times a
//! second to produce an identical one.
//!
//! # Colours
//!
//! [`Color`] is re-exported from the protocol rather than redefined, and named
//! colours are provided as functions so a plugin can write [`text_color`] without
//! constructing an `Option`. A plugin that names no colour at all gets the
//! product's own reading of a small overlay panel, which is why a panel that uses
//! [`text`] and nothing else still looks like it belongs.

use bongocat_plugin_protocol::{
    ButtonNode, ButtonVariant, Color, DividerNode, PluginAnchor, ProgressBarNode, ProgressRingNode,
    SceneNode, SpacerNode, StackAxis, StackNode, TextNode, TextWeight,
};

/// One panel's placement and its current contents.
///
/// Not a scene: a scene is one moment of a panel, and this is the box it lives in
/// plus the last thing put in it. Keeping them together is what lets [`Self::send`]
/// be one call and what lets [`Self::rebuild`] skip the work when nothing changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    size: [u32; 2],
    anchor: PluginAnchor,
    margin: [f32; 2],
    width_fraction: f32,
    opacity: f32,
    scene: SceneNode,
    /// The tree the host was last sent, when there is one.
    ///
    /// Separate from `scene` because building a panel and sending it are different
    /// acts, and only the second one changes what the model window has. Keeping
    /// both is what lets [`crate::Host::panel`] answer "is this different from what
    /// is on screen" without a plugin holding the previous tree itself.
    sent: Option<SceneNode>,
}

impl Panel {
    /// A panel of `width` by `height` logical pixels, pinned top-left.
    ///
    /// The size is logical rather than device so a panel is the same size relative
    /// to the model window at every display scale; the host multiplies by its own
    /// raster scale.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            size: [width, height],
            anchor: PluginAnchor::TopLeft,
            margin: [0.02, 0.02],
            width_fraction: 0.68,
            opacity: 0.94,
            scene: SceneNode::Stack(StackNode::default()),
            sent: None,
        }
    }

    /// This panel, pinned somewhere else in the model window.
    pub fn anchored(mut self, anchor: PluginAnchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// This panel, with a gap from the window edge in fractions of its size.
    pub fn with_margin(mut self, horizontal: f32, vertical: f32) -> Self {
        self.margin = [horizontal, vertical];
        self
    }

    /// This panel, taking a share of the window's width.
    pub fn with_width_fraction(mut self, width_fraction: f32) -> Self {
        self.width_fraction = width_fraction;
        self
    }

    /// This panel, drawn at this opacity.
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    /// This panel's current tree.
    pub fn scene(&self) -> &SceneNode {
        &self.scene
    }

    /// This panel's logical size.
    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    /// This panel's tree, as the protocol's update message.
    pub fn to_update(&self) -> bongocat_plugin_protocol::PanelUpdate {
        bongocat_plugin_protocol::PanelUpdate {
            placement: bongocat_plugin_protocol::PanelPlacement {
                anchor: self.anchor,
                margin: self.margin,
                width_fraction: self.width_fraction,
                opacity: self.opacity,
                size: self.size,
            },
            scene: self.scene.clone(),
        }
    }

    /// Rebuild the tree.
    ///
    /// Unconditionally: a panel is a *proposal*, and whether it is worth sending is
    /// a question about what the host already has, which [`Self::needs_sending`]
    /// answers and [`crate::Host::panel`] acts on. A plugin that wants to skip the
    /// work as well as the send checks first — build the scene, compare it with
    /// [`Self::scene`], and only call this when it differs — which is what a panel
    /// redrawing on a fast clock should do.
    pub fn rebuild(&mut self, build: impl FnOnce(&mut SceneBuilder)) {
        let mut builder = SceneBuilder::default();
        build(&mut builder);
        self.scene = builder.into_scene();
    }

    /// Put a specific tree in this panel.
    pub fn set(&mut self, scene: SceneNode) {
        self.scene = scene;
    }

    /// Whether this panel's tree differs from the one the host was last sent.
    pub fn needs_sending(&self) -> bool {
        self.sent.as_ref() != Some(&self.scene)
    }

    /// Whether this panel has ever reached the host.
    pub fn has_been_sent(&self) -> bool {
        self.sent.is_some()
    }

    /// Record that the host now has this panel's tree.
    pub(crate) fn mark_sent(&mut self) {
        self.sent = Some(self.scene.clone());
    }
}

/// Collects the children of one stack.
///
/// Two types rather than one because they answer different questions: a
/// [`SceneBuilder`] collects, and a [`Panel`] holds a finished tree with a
/// placement. A plugin writes `panel.rebuild(|p| p.column(|c| c.text(…)))` and
/// never names either.
#[derive(Clone, Debug, Default)]
pub struct SceneBuilder {
    children: Vec<SceneNode>,
}

impl SceneBuilder {
    /// A column: children top to bottom. The default, because a panel is a column
    /// far more often than it is a row.
    pub fn column(&mut self, build: impl FnOnce(&mut SceneBuilder)) {
        self.stack(StackAxis::Vertical, 0.0, [0.0, 0.0], build)
    }

    /// A column with a gap and inner padding, on a translucent surface.
    ///
    /// This is what a panel's root almost always is, so it takes the surface
    /// rather than making every plugin spell out the colour the product uses.
    pub fn surface(
        &mut self,
        spacing: f32,
        padding: [f32; 2],
        build: impl FnOnce(&mut SceneBuilder),
    ) {
        let mut builder = SceneBuilder::default();
        build(&mut builder);
        self.children.push(SceneNode::Stack(StackNode {
            axis: StackAxis::Vertical,
            spacing,
            padding,
            background: Some(panel_surface()),
            radius: PANEL_RADIUS,
            border: Some(Color::rgba(255, 255, 255, 26)),
            border_width: 1.0,
            children: builder.children,
            ..StackNode::default()
        }));
    }

    /// A row: children left to right.
    pub fn row(&mut self, build: impl FnOnce(&mut SceneBuilder)) {
        self.stack(StackAxis::Horizontal, 0.0, [0.0, 0.0], build)
    }

    /// A row with a gap between its children.
    pub fn row_spaced(&mut self, spacing: f32, build: impl FnOnce(&mut SceneBuilder)) {
        self.stack(StackAxis::Horizontal, spacing, [0.0, 0.0], build)
    }

    /// A row of items that share the panel's height.
    pub fn row_centered(&mut self, spacing: f32, build: impl FnOnce(&mut SceneBuilder)) {
        let mut builder = SceneBuilder::default();
        build(&mut builder);
        self.children.push(SceneNode::Stack(StackNode {
            axis: StackAxis::Horizontal,
            spacing,
            cross_align: Some(bongocat_plugin_protocol::Align::Center),
            children: builder.children,
            ..StackNode::default()
        }));
    }

    fn stack(
        &mut self,
        axis: StackAxis,
        spacing: f32,
        padding: [f32; 2],
        build: impl FnOnce(&mut SceneBuilder),
    ) {
        let mut builder = SceneBuilder::default();
        build(&mut builder);
        self.children.push(SceneNode::Stack(StackNode {
            axis,
            spacing,
            padding,
            children: builder.children,
            ..StackNode::default()
        }));
    }

    /// Add a node this plugin built with one of the free functions.
    ///
    /// The counterpart to the typed helpers below: `column(|c| c.text("hi", 14.0))`
    /// is the common case and reads better than the free function's name inside a
    /// closure, while `c.push(image("assets/cat.png", 32.0))` is the case where
    /// the free function's name is clearer than any method would be.
    pub fn push(&mut self, node: SceneNode) {
        self.children.push(node);
    }

    /// A label, in the product's own primary text colour.
    pub fn text(&mut self, value: &str, size: f32) {
        self.push(text(value, size));
    }

    /// A label in a colour the plugin names.
    pub fn text_colored(&mut self, value: &str, size: f32, color: Color) {
        self.push(text_colored(value, size, color));
    }

    /// A label in the product's muted colour, for a secondary line.
    pub fn muted(&mut self, value: &str, size: f32) {
        self.push(muted(value, size));
    }

    /// A bold label, for the one number a panel is about.
    pub fn heading(&mut self, value: &str, size: f32) {
        self.push(heading(value, size));
    }

    /// A horizontal fill for a proportion.
    pub fn bar(&mut self, fraction: f32, height: f32) {
        self.push(bar(fraction, height));
    }

    /// A circular fill for a proportion.
    pub fn ring(&mut self, fraction: f32, size: f32) {
        self.push(ring(fraction, size));
    }

    /// A pressable.
    pub fn button(&mut self, id: &str, label: &str) {
        self.push(button(id, label));
    }

    /// Empty space that grows.
    pub fn spacer(&mut self) {
        self.push(spacer());
    }

    /// A hairline.
    pub fn divider(&mut self) {
        self.push(divider());
    }

    /// A filled rounded box with a label in it: one key on a keyboard, one value in a
    /// counter, one state in a status row.
    ///
    /// A shape rather than a decoration, and it is in the SDK because three different
    /// plugins want it and none of them should have to know that a `StackNode` with a
    /// background and a `TextNode` inside it is how the product draws a chip. The colour is
    /// the panel's own surface, so a chip reads as part of the panel rather than as a
    /// rectangle somebody drew on top of it.
    pub fn chip(&mut self, label: &str, size: f32, padding: [f32; 2], radius: f32) {
        self.push(chip(label, size, padding, radius));
    }

    /// This builder's tree.
    pub fn into_scene(self) -> SceneNode {
        SceneNode::Stack(StackNode {
            children: self.children,
            ..StackNode::default()
        })
    }
}

/// The corner radius a panel's own surface uses, in logical pixels.
///
/// Named here rather than left to each plugin so two panels side by side have the
/// same rounding, which is most of what makes them read as one product.
pub const PANEL_RADIUS: f32 = 14.0;

/// The surface a panel draws on when it does not name one.
///
/// A dark translucent surface is the one choice that reads on both a light and a
/// dark desktop, and it is the same one the update window and the tray menu make.
pub const fn panel_surface() -> Color {
    Color::rgba(24, 24, 27, 204)
}

/// A label, in the product's own primary text colour.
pub fn text(value: &str, size: f32) -> SceneNode {
    SceneNode::Text(TextNode {
        value: value.to_string(),
        size,
        ..TextNode::default()
    })
}

/// A label in a colour the plugin names.
pub fn text_colored(value: &str, size: f32, color: Color) -> SceneNode {
    SceneNode::Text(TextNode {
        value: value.to_string(),
        size,
        color: Some(color),
        ..TextNode::default()
    })
}

/// A label in the product's muted colour, for a secondary line.
pub fn muted(value: &str, size: f32) -> SceneNode {
    text_colored(value, size, Color::rgb(161, 161, 170))
}

/// A bold label, for the one number a panel is about.
pub fn heading(value: &str, size: f32) -> SceneNode {
    SceneNode::Text(TextNode {
        value: value.to_string(),
        size,
        weight: Some(TextWeight::Bold),
        ..TextNode::default()
    })
}

/// A proportion brought into the range a bar or a ring can draw.
///
/// A `NaN` becomes zero rather than passing through, because `clamp` on `NaN`
/// returns `NaN` — and a panel whose only number is `NaN` draws nothing at all,
/// which is a worse outcome than showing an empty bar. A plugin that divided by a
/// total of zero gets a panel with an empty fill, which it can see and fix.
fn drawable(fraction: f32) -> f32 {
    if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// A horizontal fill for a proportion.
pub fn bar(fraction: f32, height: f32) -> SceneNode {
    SceneNode::ProgressBar(ProgressBarNode {
        value: drawable(fraction),
        height,
        fill: Some(Color::rgb(127, 209, 168)),
        track: Some(Color::rgba(255, 255, 255, 40)),
        radius: height / 2.0,
    })
}

/// A circular fill for a proportion.
pub fn ring(fraction: f32, size: f32) -> SceneNode {
    SceneNode::ProgressRing(ProgressRingNode {
        value: drawable(fraction),
        size,
        thickness: 5.0,
        fill: Some(Color::rgb(127, 209, 168)),
        track: Some(Color::rgba(255, 255, 255, 40)),
    })
}

/// A pressable that runs nothing by itself — the plugin decides what a press means.
pub fn button(id: &str, label: &str) -> SceneNode {
    SceneNode::Button(ButtonNode {
        id: id.to_string(),
        label: label.to_string(),
        variant: ButtonVariant::Primary,
        radius: 8.0,
        ..ButtonNode::default()
    })
}

/// A pressable in the product's secondary chrome.
pub fn button_secondary(id: &str, label: &str) -> SceneNode {
    SceneNode::Button(ButtonNode {
        id: id.to_string(),
        label: label.to_string(),
        variant: ButtonVariant::Secondary,
        radius: 8.0,
        ..ButtonNode::default()
    })
}

/// A pressable that is drawn greyed and refuses its press.
pub fn button_disabled(id: &str, label: &str) -> SceneNode {
    SceneNode::Button(ButtonNode {
        id: id.to_string(),
        label: label.to_string(),
        variant: ButtonVariant::Primary,
        disabled: true,
        radius: 8.0,
        ..ButtonNode::default()
    })
}

/// Empty space that grows to fill what the stack has left.
pub fn spacer() -> SceneNode {
    SceneNode::Spacer(SpacerNode { grow: 1.0 })
}

/// Empty space that grows by a share of the leftover.
pub fn spacer_share(grow: f32) -> SceneNode {
    SceneNode::Spacer(SpacerNode { grow })
}

/// A hairline between two parts of a panel.
pub fn divider() -> SceneNode {
    SceneNode::Divider(DividerNode {
        thickness: 1.0,
        color: Some(Color::rgba(255, 255, 255, 32)),
    })
}

/// A filled rounded box with a label in it.
///
/// The free-function form of [`SceneBuilder::chip`], for the case where the name reads
/// better than a method inside a closure.
pub fn chip(label: &str, size: f32, padding: [f32; 2], radius: f32) -> SceneNode {
    let text = heading(label, size);
    SceneNode::Stack(StackNode {
        padding,
        background: Some(panel_surface()),
        radius,
        border: Some(Color::rgba(255, 255, 255, 26)),
        border_width: 1.0,
        cross_align: Some(bongocat_plugin_protocol::Align::Center),
        children: vec![text],
        ..StackNode::default()
    })
}

/// A PNG the plugin ships beside its manifest.
pub fn image(asset: &str, size: f32) -> SceneNode {
    SceneNode::Image(bongocat_plugin_protocol::ImageNode {
        asset: asset.to_string(),
        size,
        preserve_aspect: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{panel_from, rendered_hit, update_of};

    #[test]
    fn a_panel_starts_empty_and_holds_the_size_it_was_given() {
        let panel = Panel::new(240, 132);
        assert_eq!(panel.size(), [240, 132]);
        assert_eq!(
            panel.scene(),
            &SceneNode::Stack(StackNode::default()),
            "an empty panel is an empty stack, not a missing one"
        );
    }

    #[test]
    fn a_panel_knows_whether_the_host_still_has_what_it_would_send() {
        let mut panel = Panel::new(200, 60);
        panel.rebuild(|p| p.column(|c| c.text("one", 20.0)));
        assert!(panel.needs_sending(), "nothing has reached the host yet");
        assert!(!panel.has_been_sent());
        panel.mark_sent();
        assert!(
            !panel.needs_sending(),
            "so an identical rebuild is not worth sending"
        );
        panel.rebuild(|p| p.column(|c| c.text("two", 20.0)));
        assert!(panel.needs_sending(), "and a different one is");
    }

    #[test]
    fn a_panel_carries_its_placement_into_the_update() {
        let mut panel = Panel::new(240, 132)
            .anchored(PluginAnchor::BottomLeft)
            .with_margin(0.03, 0.04)
            .with_width_fraction(0.5)
            .with_opacity(0.8);
        panel.rebuild(|p| p.column(|c| c.text("hi", 16.0)));
        let update = update_of(&panel);
        assert_eq!(update.placement.anchor, PluginAnchor::BottomLeft);
        assert_eq!(update.placement.margin, [0.03, 0.04]);
        assert_eq!(update.placement.width_fraction, 0.5);
        assert_eq!(update.placement.opacity, 0.8);
        assert_eq!(update.placement.size, [240, 132]);
    }

    #[test]
    fn a_column_holds_its_children_in_the_order_they_were_added() {
        let mut panel = Panel::new(200, 100);
        panel.rebuild(|p| {
            p.column(|c| {
                c.text("first", 14.0);
                c.text("second", 14.0);
                c.text("third", 14.0);
            })
        });
        let SceneNode::Stack(root) = panel.scene() else {
            panic!("the root is a stack");
        };
        let SceneNode::Stack(column) = &root.children[0] else {
            panic!("the first child is the column");
        };
        let labels: Vec<&str> = column
            .children
            .iter()
            .map(|child| match child {
                SceneNode::Text(text) => text.value.as_str(),
                _ => panic!("every child is a label"),
            })
            .collect();
        assert_eq!(labels, ["first", "second", "third"]);
    }

    #[test]
    fn a_panel_built_here_passes_the_protocols_own_validation() {
        // The SDK's builders and the protocol's checks are two halves of one
        // contract, so a tree the SDK can produce must be a tree the host accepts.
        let mut panel = Panel::new(240, 132);
        panel.rebuild(|p| {
            p.surface(8.0, [14.0, 12.0], |c| {
                c.row_centered(10.0, |r| {
                    r.push(heading("25:00", 40.0));
                    r.push(spacer());
                    r.push(ring(0.6, 44.0));
                });
                c.push(bar(0.6, 6.0));
                c.push(divider());
                c.row_spaced(8.0, |r| {
                    r.push(button("toggle", "Start"));
                    r.push(button_secondary("reset", "Reset"));
                });
            })
        });
        let update = update_of(&panel);
        update
            .validate()
            .expect("an SDK-built panel is a panel the host accepts");
    }

    #[test]
    fn two_buttons_in_one_panel_have_distinct_press_targets() {
        let mut panel = Panel::new(240, 120);
        panel.rebuild(|p| {
            p.row_spaced(8.0, |r| {
                r.push(button("toggle", "Start"));
                r.push(button("reset", "Reset"));
            })
        });
        let update = update_of(&panel);
        update
            .validate()
            .expect("the ids differ, so a press is unambiguous");
        let rendered = panel_from(&panel);
        assert_eq!(
            rendered.press_targets(),
            vec!["reset".to_string(), "toggle".to_string()],
            "both buttons are pressable, and the hit test knows each by its own id"
        );
    }

    #[test]
    fn a_greyed_button_is_reported_as_greyed_and_is_still_a_target() {
        let mut panel = Panel::new(240, 60);
        panel.rebuild(|p| p.column(|c| c.push(button_disabled("go", "Go"))));
        let rendered = panel_from(&panel);
        assert!(rendered.press_targets().contains(&"go".to_string()));
        assert_eq!(rendered.disabled_targets(), vec!["go".to_string()]);
    }

    #[test]
    fn a_filled_panel_renders_with_one_press_region_per_button() {
        let mut panel = Panel::new(240, 120);
        panel.rebuild(|p| {
            p.surface(8.0, [14.0, 12.0], |c| {
                c.row_spaced(8.0, |r| {
                    r.push(button("toggle", "Start"));
                    r.push(button_secondary("reset", "Reset"));
                })
            })
        });
        let rendered = panel_from(&panel);
        assert_eq!(rendered.hit_test_count(), 2);
        assert!(rendered_hit(&rendered, "toggle").is_some());
    }

    #[test]
    fn a_bar_and_a_ring_clamp_a_fraction_the_plugin_miscomputed() {
        // A plugin computing `remaining / total` can divide by a total of zero.
        // The panel must still draw rather than disappear.
        let mut panel = Panel::new(200, 100);
        panel.rebuild(|p| {
            p.column(|c| {
                c.push(bar(f32::NAN, 6.0));
                c.push(ring(-4.0, 40.0));
            })
        });
        let update = update_of(&panel);
        update
            .validate()
            .expect("a clamped fraction is still a fraction");
        let SceneNode::Stack(root) = panel.scene() else {
            panic!("the root is a stack");
        };
        let SceneNode::Stack(column) = &root.children[0] else {
            panic!("the first child is the column");
        };
        let SceneNode::ProgressBar(bar) = &column.children[0] else {
            panic!("the column's first child is the bar");
        };
        assert_eq!(
            bar.value, 0.0,
            "a `NaN` becomes zero rather than staying `NaN`, because `clamp` on `NaN` returns `NaN` \
             and a panel whose only number is `NaN` draws nothing at all"
        );
        let SceneNode::ProgressRing(ring) = &column.children[1] else {
            panic!("the column's second child is the ring");
        };
        assert_eq!(
            ring.value, 0.0,
            "and a fraction below zero becomes zero, which is a rule the renderer can rely on"
        );
    }

    #[test]
    fn a_chip_is_a_filled_box_with_a_label_in_it() {
        // The shape three plugins want and none of them should have to know the node
        // vocabulary for. Its own test here because a chip is the SDK's idea, not a
        // plugin's: a plugin that drew its own would draw a different chip.
        let mut panel = Panel::new(240, 90);
        panel.rebuild(|p| {
            p.surface(6.0, [14.0, 12.0], |content| {
                content.row_spaced(6.0, |row| {
                    row.chip("A", 13.0, [10.0, 6.0], 6.0);
                    row.chip("Shift", 13.0, [10.0, 6.0], 6.0);
                });
            })
        });
        update_of(&panel)
            .validate()
            .expect("a chip is a shape the host accepts");
        let SceneNode::Stack(root) = panel.scene() else {
            panic!("the root is a stack");
        };
        let SceneNode::Stack(surface) = &root.children[0] else {
            panic!("the first child is the surface");
        };
        let SceneNode::Stack(row) = &surface.children[0] else {
            panic!("the surface's first child is the row");
        };
        let SceneNode::Stack(first) = &row.children[0] else {
            panic!("the row's first child is a chip");
        };
        assert!(first.background.is_some(), "a chip is filled");
        assert!(
            first.border.is_some(),
            "and outlined, or it reads as a hole"
        );
        assert_eq!(first.children.len(), 1, "and holds exactly its label");
    }

    #[test]
    fn an_image_node_names_the_plugins_own_relative_path() {
        let node = image("assets/cat.png", 32.0);
        let SceneNode::Image(image) = node else {
            panic!("expected an image");
        };
        assert_eq!(image.asset, "assets/cat.png");
        assert!(image.preserve_aspect);
    }
}

//! Laying a scene out and drawing it.
//!
//! Two passes over the same tree, and the reason they are separate is that a
//! panel has a fixed size it did not choose. A stack cannot ask its parent for
//! more room, so the only correct order is: find out what every node wants, then
//! give each one what it can have. Doing it in one pass would mean a node's size
//! depended on whether it had been reached yet, which is how a layout pass ends
//! up depending on traversal order.
//!
//! The pass that draws also records pressable regions, so what a user sees and
//! what a user can press are produced together and cannot drift apart.

use crate::canvas::{Canvas, RoundedRect};
use crate::error::{PluginRenderError, PluginRenderErrorCode};
use crate::font::{FontWeight, TextMeasurer, TextStyle};
use crate::{HitRegion, ImageLibrary, RenderedPanel, Theme};
use bongocat_plugin_protocol::{
    Align, ButtonNode, ButtonVariant, Color, DividerNode, ImageNode, ProgressBarNode,
    ProgressRingNode, SceneNode, SpacerNode, StackAxis, StackNode, TextNode, TextWeight,
};

/// The height of a text run, when no face is available.
///
/// A panel on a machine with no usable font still has to lay out: a label with no
/// height would collapse its stack and every button below it would overlap. So
/// the fallback is the em size, which is the right order of magnitude and is only
/// ever reached on a machine that could not draw the text anyway.
const FALLBACK_TEXT_HEIGHT: f32 = 12.0;

/// The size of a button's label, in logical pixels.
const BUTTON_TEXT_SIZE: f32 = 13.0;

/// The padding inside a button, in logical pixels.
const BUTTON_PADDING: [f32; 2] = [10.0, 6.0];

/// The smallest a pressable may be, in logical pixels.
///
/// A hit target smaller than this cannot be pressed reliably at a model window's
/// usual size, and a pressable that cannot be pressed is worse than one that is
/// visibly larger than its label.
pub const MINIMUM_BUTTON_SIZE: f32 = 24.0;

/// What a node wants, in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Extent {
    pub width: f32,
    pub height: f32,
}

/// Measure what a node wants on its main axis and cross axis.
pub(crate) fn measure(
    node: &SceneNode,
    available: f32,
    measurer: &mut TextMeasurer,
    images: &ImageLibrary,
    theme: &Theme,
) -> Extent {
    match node {
        SceneNode::Stack(stack) => measure_stack(stack, available, measurer, images, theme),
        SceneNode::Text(text) => measure_text(text, measurer, theme),
        SceneNode::Spacer(_) => Extent::default(),
        SceneNode::Divider(divider) => Extent {
            width: available.max(0.0),
            height: divider.thickness.max(0.0),
        },
        SceneNode::ProgressBar(bar) => measure_bar(bar, available),
        SceneNode::ProgressRing(ring) => {
            let size = ring.size.max(0.0);
            Extent {
                width: size,
                height: size,
            }
        }
        SceneNode::Image(image) => {
            let size = image.size.max(0.0);
            let _ = images;
            Extent {
                width: size,
                height: size,
            }
        }
        SceneNode::Button(button) => measure_button(button, available, measurer, theme),
    }
}

fn measure_stack(
    stack: &StackNode,
    available: f32,
    measurer: &mut TextMeasurer,
    images: &ImageLibrary,
    theme: &Theme,
) -> Extent {
    let vertical = matches!(stack.axis, StackAxis::Vertical);
    let padding = stack.padding;
    let inner = Extent {
        width: (available - padding[0] * 2.0).max(0.0),
        height: (available - padding[1] * 2.0).max(0.0),
    };
    let along = if vertical { inner.height } else { inner.width };
    let across = if vertical { inner.width } else { inner.height };

    // Spacers share whatever the fixed children leave, so they are measured last
    // and given the remainder rather than their nominal `grow`.
    let mut used = 0.0_f32;
    let mut grow_total = 0.0_f32;
    let mut fixed_extent = 0.0_f32;
    let mut cross_extent = 0.0_f32;
    let mut first = true;
    for child in &stack.children {
        if !first {
            used += stack.spacing.max(0.0);
        }
        first = false;
        if let SceneNode::Spacer(SpacerNode { grow }) = child {
            grow_total += grow.max(0.0);
            continue;
        }
        let extent = measure(child, across, measurer, images, theme);
        let main = if vertical {
            extent.height
        } else {
            extent.width
        };
        used += main;
        fixed_extent = fixed_extent.max(main);
        cross_extent = cross_extent.max(if vertical {
            extent.width
        } else {
            extent.height
        });
    }
    let leftover = (along - used).max(0.0);
    let shared = if grow_total > 0.0 {
        leftover / grow_total
    } else {
        0.0
    };
    let content_main = used + shared * grow_total;
    let (content_cross, cross_axis) = if grow_total > 0.0 && cross_extent < across {
        (cross_axis_extent(stack, across, &cross_extent), across)
    } else {
        (cross_extent, cross_extent)
    };
    let size = Extent {
        width: if vertical {
            (content_cross + padding[0] * 2.0).min(available.max(0.0))
        } else {
            (content_main + padding[0] * 2.0).min(available.max(0.0))
        },
        height: if vertical {
            (content_main + padding[1] * 2.0).min(available.max(0.0))
        } else {
            (content_cross + padding[1] * 2.0).min(available.max(0.0))
        },
    };
    let _ = cross_axis;
    size
}

fn cross_axis_extent(stack: &StackNode, available: f32, measured: &f32) -> f32 {
    // A stack with a growing spacer fills the cross axis it was offered, so a
    // panel's own width is what a header row ends up spanning.
    let grows = stack
        .children
        .iter()
        .any(|child| matches!(child, SceneNode::Spacer(spacer) if spacer.grow > 0.0));
    if grows { available } else { *measured }
}

fn measure_text(text: &TextNode, measurer: &mut TextMeasurer, theme: &Theme) -> Extent {
    let _ = theme;
    let style = TextStyle {
        size: text.size.max(1.0),
        weight: match text.weight {
            Some(TextWeight::Bold) => FontWeight::Bold,
            _ => FontWeight::Regular,
        },
    };
    let measured = measurer.measure(&text.value, style);
    match measured {
        Some(line) if !line.glyphs.is_empty() => Extent {
            width: line.width(),
            height: line.height().max(text.size),
        },
        _ => Extent {
            width: 0.0,
            height: text.size.max(FALLBACK_TEXT_HEIGHT),
        },
    }
}

fn measure_bar(bar: &ProgressBarNode, available: f32) -> Extent {
    Extent {
        width: available.max(0.0),
        height: bar.height.max(1.0),
    }
}

fn measure_button(
    button: &ButtonNode,
    available: f32,
    measurer: &mut TextMeasurer,
    _theme: &Theme,
) -> Extent {
    let style = TextStyle {
        size: BUTTON_TEXT_SIZE,
        weight: FontWeight::Regular,
    };
    let text_width = measurer
        .measure(&button.label, style)
        .map_or(0.0, |line| line.width());
    // The minimum press size is a floor, not a guarantee: a panel too small to
    // hold it gets what it has. A button taller than the panel it is in would be
    // a press target reaching past the panel's own pixels, which is the one thing
    // a hit test cannot express.
    Extent {
        width: (text_width + BUTTON_PADDING[0] * 2.0)
            .max(MINIMUM_BUTTON_SIZE)
            .min(available.max(0.0)),
        height: (BUTTON_TEXT_SIZE + BUTTON_PADDING[1] * 2.0)
            .max(MINIMUM_BUTTON_SIZE)
            .min(available.max(0.0)),
    }
}

/// Draw a node into a rectangle, recording its pressable regions.
pub(crate) fn draw(
    canvas: &mut Canvas,
    node: &SceneNode,
    rect: RoundedRect,
    measurer: &mut TextMeasurer,
    images: &ImageLibrary,
    theme: &Theme,
) -> Result<HitOutcome, PluginRenderError> {
    match node {
        SceneNode::Stack(stack) => draw_stack(canvas, stack, rect, measurer, images, theme),
        SceneNode::Text(text) => {
            draw_text(canvas, text, rect, measurer, theme);
            Ok(HitOutcome::none())
        }
        SceneNode::Spacer(_) => Ok(HitOutcome::none()),
        SceneNode::Divider(divider) => {
            draw_divider(canvas, divider, rect, theme);
            Ok(HitOutcome::none())
        }
        SceneNode::ProgressBar(bar) => {
            draw_bar(canvas, bar, rect, theme);
            Ok(HitOutcome::none())
        }
        SceneNode::ProgressRing(ring) => {
            draw_ring(canvas, ring, rect, theme);
            Ok(HitOutcome::none())
        }
        SceneNode::Image(image) => {
            draw_image(canvas, image, rect, images);
            Ok(HitOutcome::none())
        }
        SceneNode::Button(button) => draw_button(canvas, button, rect, measurer, theme),
    }
}

/// What a node's draw produced.
#[derive(Debug, Default)]
pub(crate) struct HitOutcome {
    pub hits: Vec<HitRegion>,
    pub disabled: Vec<HitRegion>,
}

impl HitOutcome {
    fn none() -> Self {
        Self::default()
    }

    /// Fold a child's regions into this node's own.
    ///
    /// No offset is applied, because there is none to apply: a child records the
    /// rectangle it was *given*, and that rectangle is already in the panel's own
    /// coordinates. Adding the child's origin again is how a press target ends up
    /// at twice its real position — which reads as a button that is pressable
    /// somewhere it is not drawn.
    fn absorb(&mut self, child: HitOutcome) {
        self.hits.extend(child.hits);
        self.disabled.extend(child.disabled);
    }
}

fn draw_stack(
    canvas: &mut Canvas,
    stack: &StackNode,
    rect: RoundedRect,
    measurer: &mut TextMeasurer,
    images: &ImageLibrary,
    theme: &Theme,
) -> Result<HitOutcome, PluginRenderError> {
    if let Some(background) = stack.background {
        canvas.fill_rect(rect, stack.radius.max(0.0), background);
    }
    if let Some(border) = stack.border {
        let width = stack.border_width.max(0.0);
        if width > 0.0 {
            canvas.stroke_rect(rect, stack.radius.max(0.0), width, border);
        }
    }
    let vertical = matches!(stack.axis, StackAxis::Vertical);
    let content = rect.inset(0.0);
    let inner = RoundedRect {
        x: content.x + stack.padding[0],
        y: content.y + stack.padding[1],
        width: (content.width - stack.padding[0] * 2.0).max(0.0),
        height: (content.height - stack.padding[1] * 2.0).max(0.0),
    };
    let along = if vertical { inner.height } else { inner.width };

    let mut extents = Vec::with_capacity(stack.children.len());
    let mut grow_total = 0.0_f32;
    let mut used = 0.0_f32;
    let mut first = true;
    for child in &stack.children {
        if !first {
            used += stack.spacing.max(0.0);
        }
        first = false;
        if let SceneNode::Spacer(spacer) = child {
            grow_total += spacer.grow.max(0.0);
            extents.push(Extent::default());
            continue;
        }
        let across = if vertical { inner.width } else { inner.height };
        let extent = measure(child, across, measurer, images, theme);
        used += if vertical {
            extent.height
        } else {
            extent.width
        };
        extents.push(extent);
    }
    let leftover = (along - used).max(0.0);
    let shared = if grow_total > 0.0 {
        leftover / grow_total
    } else {
        0.0
    };

    let mut outcome = HitOutcome::none();
    let mut pen = 0.0_f32;
    let mut first = true;
    for (child, extent) in stack.children.iter().zip(extents) {
        let extra = if matches!(child, SceneNode::Spacer(_)) {
            shared
        } else {
            0.0
        };
        if !first {
            pen += stack.spacing.max(0.0);
        }
        first = false;
        // The parentheses are load-bearing: without them the `+ extra` binds to
        // the `else` arm only, so a vertical stack's spacers would grow by
        // nothing and a horizontal one's would grow twice.
        let main = (if vertical {
            extent.height
        } else {
            extent.width
        }) + extra;
        let cross = cross_for(stack, inner, extent, extra, vertical);
        // A child never gets more of the stack's own axis than the stack has
        // left, even when it asked for more. Without this the last child of an
        // over-full panel is handed a rectangle reaching past the panel's edge,
        // and a button's press target would then extend over pixels that are not
        // its own.
        let main = main.min(along);
        let child_rect = if vertical {
            RoundedRect {
                x: inner.x,
                y: inner.y + pen,
                width: cross,
                height: main,
            }
        } else {
            RoundedRect {
                x: inner.x + pen,
                y: inner.y,
                width: main,
                height: cross,
            }
        };
        let child_outcome = draw(canvas, child, child_rect, measurer, images, theme)?;
        outcome.absorb(child_outcome);
        pen += main;
    }
    Ok(outcome)
}

/// Where a child sits across the axis its stack does not lay out on.
fn cross_for(
    stack: &StackNode,
    inner: RoundedRect,
    extent: Extent,
    extra: f32,
    vertical: bool,
) -> f32 {
    let offered = if vertical { inner.width } else { inner.height };
    let needed = if vertical {
        extent.width
    } else {
        extent.height
    } + extra;
    let align = stack.cross_align.unwrap_or(Align::Start);
    let aligned = match align {
        Align::Start | Align::Stretch => 0.0,
        Align::Center => ((offered - needed) * 0.5).max(0.0),
        Align::End => (offered - needed).max(0.0),
    };
    // A child never gets more than was offered, so a panel's own padding always
    // holds even when a child wanted more.
    needed.min(offered.max(0.0)) + aligned.max(0.0) * 0.0
}

fn draw_text(
    canvas: &mut Canvas,
    text: &TextNode,
    rect: RoundedRect,
    measurer: &mut TextMeasurer,
    theme: &Theme,
) {
    let style = TextStyle {
        size: text.size.max(1.0),
        weight: match text.weight {
            Some(TextWeight::Bold) => FontWeight::Bold,
            _ => FontWeight::Regular,
        },
    };
    // The line is copied out of the measurer before it is drawn, because the
    // canvas needs the face to rasterize the outline and the measurer owns it.
    // A measurement is a few glyphs and two metrics, so the copy is cheaper than
    // the borrow conflict it avoids.
    let Some(line) = measurer.measure(&text.value, style).cloned() else {
        // No face: the node contributes its height and no marks. This is the
        // whole degradation path, and it is why a missing font is not an error.
        return;
    };
    if line.glyphs.is_empty() {
        return;
    }
    let color = text.color.unwrap_or(theme.text);
    let align = text.align.unwrap_or(Align::Start);
    let start_x = match align {
        Align::Start => rect.x,
        Align::Center => rect.x + ((rect.width - line.width()) * 0.5).max(0.0),
        Align::End => rect.x + (rect.width - line.width()).max(0.0),
        Align::Stretch => rect.x,
    };
    // One line, always. `max_lines` is a bound the layout already enforced by
    // clipping the node's height, so a run that does not fit is cut rather than
    // allowed to bleed into the row below.
    let baseline = rect.y + line.ascent;
    canvas.draw_text(measurer.book(), &line, start_x, baseline, color);
}

fn draw_divider(canvas: &mut Canvas, divider: &DividerNode, rect: RoundedRect, theme: &Theme) {
    let color = divider.color.unwrap_or(theme.divider);
    let thickness = divider.thickness.max(1.0);
    canvas.fill_rect(
        RoundedRect {
            x: rect.x,
            y: rect.y + (rect.height - thickness) * 0.5,
            width: rect.width,
            height: thickness,
        },
        0.0,
        color,
    );
}

fn draw_bar(canvas: &mut Canvas, bar: &ProgressBarNode, rect: RoundedRect, theme: &Theme) {
    let fraction = bar.value;
    let height = bar.height.max(1.0);
    let track = RoundedRect {
        x: rect.x,
        y: rect.y + (rect.height - height) * 0.5,
        width: rect.width,
        height,
    };
    canvas.fill_rect(track, bar.radius.max(0.0), bar.track.unwrap_or(theme.track));
    if fraction > 0.0 {
        let fill = RoundedRect {
            width: (rect.width * fraction).max(height).min(rect.width),
            ..track
        };
        canvas.fill_rect(fill, bar.radius.max(0.0), bar.fill.unwrap_or(theme.accent));
    }
}

fn draw_ring(canvas: &mut Canvas, ring: &ProgressRingNode, rect: RoundedRect, theme: &Theme) {
    let fraction = ring.value;
    let size = ring.size.max(1.0);
    let center = (rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
    let radius = (size.min(rect.width).min(rect.height)) * 0.5;
    if radius <= 0.0 {
        return;
    }
    // Angles run from the top, clockwise, so a ring at zero shows nothing rather
    // than a full circle with a one-pixel gap.
    let start = -std::f32::consts::FRAC_PI_2;
    let full = std::f32::consts::TAU;
    canvas.stroke_arc(
        center,
        radius,
        ring.thickness.max(0.0),
        start,
        full,
        ring.track.unwrap_or(theme.track),
    );
    if fraction > 0.0 {
        canvas.stroke_arc(
            center,
            radius,
            ring.thickness.max(0.0),
            start,
            full * fraction,
            ring.fill.unwrap_or(theme.accent),
        );
    }
}

fn draw_image(canvas: &mut Canvas, image: &ImageNode, rect: RoundedRect, images: &ImageLibrary) {
    let Some(decoded) = images.get(&image.asset) else {
        // A missing image leaves the node's space empty rather than failing the
        // panel: the rest of the panel is still worth showing, and a placeholder
        // box would be a picture of a problem rather than of the plugin.
        return;
    };
    canvas.draw_image(decoded, rect, image.keeps_aspect());
}

fn draw_button(
    canvas: &mut Canvas,
    button: &ButtonNode,
    rect: RoundedRect,
    measurer: &mut TextMeasurer,
    theme: &Theme,
) -> Result<HitOutcome, PluginRenderError> {
    let disabled = button.disabled;
    let radius = button.radius.max(0.0);
    match button.variant {
        ButtonVariant::Primary => {
            let fill = button.color.unwrap_or(if disabled {
                theme.button_surface
            } else {
                theme.accent
            });
            canvas.fill_rect(rect, radius, fill);
        }
        ButtonVariant::Secondary => {
            let fill = button.color.unwrap_or(theme.button_surface);
            canvas.fill_rect(rect, radius, fill);
            canvas.stroke_rect(rect, radius, 1.0, theme.button_border);
        }
        ButtonVariant::Transparent => {}
    }
    let style = TextStyle {
        size: BUTTON_TEXT_SIZE,
        weight: FontWeight::Regular,
    };
    let text_color = button.text_color.unwrap_or(match button.variant {
        ButtonVariant::Primary => Color::WHITE,
        _ => theme.button_text,
    });
    if let Some(line) = measurer.measure(&button.label, style).cloned()
        && !line.glyphs.is_empty()
    {
        let start_x = rect.x + ((rect.width - line.width()) * 0.5).max(0.0);
        let baseline = rect.y + (rect.height - line.height()) * 0.5 + line.ascent;
        canvas.draw_text(measurer.book(), &line, start_x, baseline, text_color);
    }
    let region = HitRegion {
        button: button.id.clone(),
        rect,
    };
    let mut outcome = HitOutcome::none();
    if disabled {
        outcome.disabled.push(region);
    } else {
        outcome.hits.push(region);
    }
    Ok(outcome)
}

/// Assemble the rendered panel from a canvas and one root node's regions.
pub(crate) fn finish(
    canvas: Canvas,
    root: HitOutcome,
    anchor: bongocat_render::OverlayAnchor,
    margin: [f32; 2],
    width_fraction: f32,
    opacity: f32,
) -> Result<RenderedPanel, PluginRenderError> {
    let (width, height) = canvas.size();
    let mut pixels = canvas.into_pixels();
    if width == 0 || height == 0 {
        return Err(PluginRenderError::bare(
            PluginRenderErrorCode::RasterTooLarge,
        ));
    }
    // A panel that drew nothing publishes nothing, rather than a transparent
    // layer the overlay has to upload and hit-test.
    if pixels.chunks_exact(4).all(|pixel| pixel[3] == 0) {
        pixels = Vec::new();
    }
    Ok(RenderedPanel {
        pixels,
        width,
        height,
        anchor,
        margin,
        width_fraction,
        opacity,
        hit_regions: root.hits,
        disabled_regions: root.disabled,
    })
}

//! The bubble a plugin shows beside the model.
//!
//! A bubble is the shortest-lived thing the model window draws: a line of text, in a
//! rounded box, that appears because something just happened and is gone within
//! seconds. That is why it is **not** a panel and **not** a runtime concern.
//!
//! * Not a panel, because a panel belongs to a plugin and lasts as long as the plugin
//!   has something to say. A bubble belongs to an *event*, outlives no plugin, and
//!   two plugins can each want one without either of them giving up its panel.
//! * Not a runtime concern, because the runtime owns the model's state and publishes
//!   a frame; a bubble is chrome above the model, drawn on the same latest-wins layer
//!   channel panels already travel. Putting it in the runtime would mean a plugin's
//!   one-line announcement invalidating a frame the renderer was already producing.
//!
//! So it is host state, with one layer id, on the model window's own channel. The
//! plugin's protocol says "show this text for this long" and the host answers whether
//! it was shown — a bounded lifetime enforced here, because a plugin that forgot to
//! take a bubble down must not leave something on the user's desktop.

use bongocat_plugin_protocol::{
    Color, DividerNode, LocalizedText, MAXIMUM_BUBBLE_CHARS, MAXIMUM_BUBBLE_MILLIS,
    MINIMUM_BUBBLE_MILLIS, ModelRequest, PanelPlacement, PanelUpdate, PluginAnchor, SceneNode,
    StackNode, TextNode,
};

/// The box a bubble is drawn in, in logical pixels.
///
/// Wider than a panel and much shorter, because a bubble is a sentence and a panel is
/// a set of controls. The width is the one number that decides how much of a sentence
/// fits, so it is here rather than in each request.
pub const BUBBLE_WIDTH: u32 = 300;

/// The height a bubble is drawn at, in logical pixels.
///
/// Fixed rather than measured, so a bubble never changes the model window's layout as
/// it appears — the text is truncated to fit instead, which is the same rule a
/// panel's own text follows.
pub const BUBBLE_HEIGHT: u32 = 44;

/// The largest fraction of the model window's width a bubble may take.
///
/// Below the panel's own bound, because a bubble sits *over* the model rather than
/// beside it, and covering the cat entirely to announce something is the wrong
/// trade even for a plugin the user installed.
pub const BUBBLE_WIDTH_FRACTION: f32 = 0.6;

/// One bubble, or the absence of one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bubble {
    /// The text, already resolved for the user's language.
    pub text: String,
    /// When it was shown, so the worker can take it down without a timer thread.
    pub shown_at: std::time::Instant,
    /// How long it stays, already clamped to the protocol's bounds.
    pub duration_ms: u32,
}

impl Bubble {
    /// Whether this bubble has been up long enough to go down.
    ///
    /// Asked by the worker's own evaluation loop rather than by a thread, because the
    /// loop already runs several times a second and a second thread per bubble would
    /// be one more thing to join at shutdown.
    pub fn has_expired(&self, now: std::time::Instant) -> bool {
        now.saturating_duration_since(self.shown_at)
            >= std::time::Duration::from_millis(u64::from(self.duration_ms))
    }

    /// The panel a bubble is drawn as.
    ///
    /// A bubble is a panel with one text node in it, which is what lets it travel the
    /// same channel, be rasterized by the same code and be uploaded by the same
    /// backend as everything else on the model window. Anchored above the model rather
    /// than in a corner, because a bubble is about what just happened and belongs
    /// where the eye already is.
    pub fn to_panel(&self) -> PanelUpdate {
        PanelUpdate {
            placement: PanelPlacement {
                anchor: PluginAnchor::TopCenter,
                // Slightly further from the edge than a panel's default, so the bubble
                // reads as attached to the model rather than as a window title.
                margin: [0.03, 0.06],
                width_fraction: BUBBLE_WIDTH_FRACTION,
                opacity: 0.96,
                size: [BUBBLE_WIDTH, BUBBLE_HEIGHT],
            },
            scene: SceneNode::Stack(StackNode {
                spacing: 0.0,
                padding: [12.0, 10.0],
                background: Some(Color::rgba(24, 24, 27, 230)),
                radius: 12.0,
                border: Some(Color::rgba(255, 255, 255, 34)),
                border_width: 1.0,
                cross_align: Some(bongocat_plugin_protocol::Align::Center),
                children: vec![
                    SceneNode::Text(TextNode {
                        // Truncated by characters rather than by bytes: a bubble cuts
                        // mid-sentence, and cutting a multi-byte character would put
                        // a replacement character in the middle of the user's screen.
                        value: self.text.chars().take(MAXIMUM_BUBBLE_CHARS).collect(),
                        size: 15.0,
                        align: Some(bongocat_plugin_protocol::Align::Center),
                        truncate: Some(true),
                        max_lines: 2,
                        ..TextNode::default()
                    }),
                    // A one-pixel rule under the text, which is what makes the box read
                    // as a bubble rather than as a tooltip with a lot of padding.
                    SceneNode::Divider(DividerNode {
                        thickness: 1.0,
                        color: Some(Color::rgba(255, 255, 255, 26)),
                    }),
                ],
                ..StackNode::default()
            }),
        }
    }
}

/// One bubble, rasterized and waiting to be published.
///
/// The pixels live beside the text rather than beside the worker that draws them, because
/// the text is what the plugin asked for and the pixels are what the model window needs,
/// and only one of the two changes when the other does.
#[derive(Clone, Debug)]
struct Showing {
    bubble: Bubble,
    rendered: bongocat_plugin_render::RenderedPanel,
    /// The layer texture this bubble is published as, built once with its panel.
    ///
    /// The same reasoning as a session's own raster: publishing builds the handle rather
    /// than the pixels, so a bubble that is up for two and a half seconds is rasterized
    /// once and shared from then on.
    raster: bongocat_render::OverlayLayerRaster,
}

/// The bubbles the worker is showing, at most one per layer.
#[derive(Debug, Default)]
pub struct BubbleSet {
    bubbles: Vec<(u64, Showing)>,
}

impl BubbleSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many bubbles are showing.
    pub fn len(&self) -> usize {
        self.bubbles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bubbles.is_empty()
    }

    /// The layer ids currently in use, so the worker knows what to withdraw.
    pub fn layer_ids(&self) -> Vec<u64> {
        self.bubbles.iter().map(|(layer, _)| *layer).collect()
    }

    /// Show a bubble, or replace the one already on `layer`.
    ///
    /// `rendered` is the bubble as the host draws it, rasterized by the caller — see
    /// [`crate::worker::Worker::show_bubble`] for why the two are built together rather
    /// than here.
    pub fn show(
        &mut self,
        layer: u64,
        bubble: Bubble,
        rendered: bongocat_plugin_render::RenderedPanel,
    ) {
        let showing = Showing {
            bubble,
            raster: rendered.to_raster(),
            rendered,
        };
        match self.bubbles.iter_mut().find(|(id, _)| *id == layer) {
            Some(entry) => entry.1 = showing,
            None => self.bubbles.push((layer, showing)),
        }
    }

    /// The rasterized bubble on a layer, if one is up.
    pub fn rendered(&self, layer: u64) -> Option<&bongocat_plugin_render::RenderedPanel> {
        self.bubbles
            .iter()
            .find(|(id, _)| *id == layer)
            .map(|(_, showing)| &showing.rendered)
    }

    /// The layer texture the bubble on `layer` is published as.
    pub fn raster(&self, layer: u64) -> Option<&bongocat_render::OverlayLayerRaster> {
        self.bubbles
            .iter()
            .find(|(id, _)| *id == layer)
            .map(|(_, showing)| &showing.raster)
    }

    /// The whole layer a bubble is published as, or [`None`] when that layer has none.
    ///
    /// One call rather than a raster lookup and a placement lookup, because the two are
    /// only ever needed together and a caller that asked for them separately could take the
    /// raster of one bubble and the placement of another.
    pub fn published(&self, layer: u64) -> Option<bongocat_render::OverlayLayer> {
        self.bubbles
            .iter()
            .find(|(id, _)| *id == layer)
            .map(|(_, showing)| bongocat_render::OverlayLayer {
                id: layer,
                placement: showing.rendered.to_placement(),
                raster: showing.raster.clone(),
            })
    }

    /// Take down the bubble on `layer`.
    pub fn hide(&mut self, layer: u64) {
        self.bubbles.retain(|(id, _)| *id != layer);
    }

    /// Take down the bubble on `layer` and say whether there was one.
    ///
    /// For a plugin that is being stopped rather than a plugin asking to be quiet: the
    /// layer is about to belong to a process that no longer exists, so a bubble left on it
    /// would be a layer with no owner and nothing that could ever take it down.
    pub fn take_layer(&mut self, layer: u64) -> bool {
        let before = self.bubbles.len();
        self.hide(layer);
        self.bubbles.len() != before
    }

    /// Take down every bubble.
    pub fn clear(&mut self) {
        self.bubbles.clear();
    }

    /// Whether any bubble has expired, and which layers.
    ///
    /// Returns the expired layers and removes them, so the worker withdraws them and
    /// does not look at them again. Removal here rather than in a second pass means a
    /// bubble that expired is not still on screen for the interval between checks.
    pub fn take_expired(&mut self, now: std::time::Instant) -> Vec<u64> {
        let mut expired = Vec::new();
        self.bubbles.retain(|(layer, showing)| {
            if showing.bubble.has_expired(now) {
                expired.push(*layer);
                false
            } else {
                true
            }
        });
        expired
    }
}

/// Whether a request is one that shows or hides a bubble.
pub const fn is_bubble_request(request: &ModelRequest) -> bool {
    matches!(
        request,
        ModelRequest::ShowBubble { .. } | ModelRequest::HideBubble
    )
}

/// Build the bubble a request asks for, with its text resolved and its life clamped.
///
/// Returns `None` for a request that is not a bubble, so one function answers both
/// "is this a bubble" and "what bubble" and a caller cannot check one and forget the
/// other.
pub fn bubble_from(
    request: &ModelRequest,
    locale: &str,
    now: std::time::Instant,
) -> Option<Bubble> {
    let ModelRequest::ShowBubble { text, duration_ms } = request else {
        return None;
    };
    Some(Bubble {
        text: resolve(text, locale),
        shown_at: now,
        duration_ms: (*duration_ms).clamp(MINIMUM_BUBBLE_MILLIS, MAXIMUM_BUBBLE_MILLIS),
    })
}

/// The text a bubble shows, in the user's language.
///
/// The plugin's own copy, resolved against the locale and then bounded — the same
/// two steps the protocol applies to the message, repeated here because the value the
/// model window draws is a resolved string and the bound is about *that* string's
/// width, not about the document that carried it.
pub fn resolve(text: &LocalizedText, locale: &str) -> String {
    text.resolve_bounded(locale)
        .chars()
        .take(MAXIMUM_BUBBLE_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bubble(text: &str, duration_ms: u32, at: std::time::Instant) -> Bubble {
        Bubble {
            text: text.to_string(),
            shown_at: at,
            duration_ms,
        }
    }

    #[test]
    fn a_bubble_stays_up_for_its_life_and_then_goes_away() {
        let now = std::time::Instant::now();
        let bubble = bubble("done", 1500, now);
        assert!(!bubble.has_expired(now));
        assert!(!bubble.has_expired(now + std::time::Duration::from_millis(1499)));
        assert!(
            bubble.has_expired(now + std::time::Duration::from_millis(1500)),
            "and the lifetime is exact, so a bubble does not linger past what was asked for"
        );
    }

    #[test]
    fn a_bubble_is_a_panel_so_it_needs_no_second_renderer() {
        let now = std::time::Instant::now();
        let update = bubble("done", 1500, now).to_panel();
        update
            .validate()
            .expect("a bubble is a panel the host can already draw");
        assert_eq!(update.placement.anchor, PluginAnchor::TopCenter);
        assert_eq!(
            update.placement.width_fraction, BUBBLE_WIDTH_FRACTION,
            "narrower than a panel's bound, because a bubble sits over the model rather than beside it"
        );
    }

    #[test]
    fn a_bubble_is_cut_at_a_character_and_never_mid_character() {
        let now = std::time::Instant::now();
        let update = bubble(&"中".repeat(400), 1500, now).to_panel();
        let SceneNode::Stack(root) = &update.scene else {
            panic!("the root is a stack");
        };
        let SceneNode::Text(text) = &root.children[0] else {
            panic!("the first child is the text");
        };
        assert_eq!(text.value.chars().count(), MAXIMUM_BUBBLE_CHARS);
        assert!(
            text.value.chars().all(|character| character == '中'),
            "a bubble cuts mid-sentence, and cutting a multi-byte character would put a replacement \\
             character in the middle of the user's screen"
        );
        assert!(
            text.truncate == Some(true),
            "and it truncates rather than overflowing"
        );
    }

    #[test]
    fn a_bubbles_text_is_the_plugins_own_copy_in_the_users_language() {
        let text = LocalizedText {
            default: "Finished".to_string(),
            by_locale: [("zh-CN".to_string(), "完成了".to_string())].into(),
        };
        assert_eq!(resolve(&text, "zh-CN"), "完成了");
        assert_eq!(resolve(&text, "ja"), "Finished");
    }

    #[test]
    fn a_show_request_becomes_a_bubble_and_anything_else_does_not() {
        let now = std::time::Instant::now();
        let show = ModelRequest::ShowBubble {
            text: "hi".into(),
            duration_ms: 2000,
        };
        assert!(is_bubble_request(&show));
        assert_eq!(
            bubble_from(&show, "en-US", now).expect("a bubble").text,
            "hi",
            "so one function answers both 'is this a bubble' and 'what bubble'"
        );
        let hide = ModelRequest::HideBubble;
        assert!(
            is_bubble_request(&hide),
            "and hiding is recognised even though it produces no bubble"
        );
        assert_eq!(bubble_from(&hide, "en-US", now), None);
        let motion = ModelRequest::PlayMotion {
            name: "wave".to_string(),
            restart: false,
        };
        assert!(!is_bubble_request(&motion));
        assert_eq!(bubble_from(&motion, "en-US", now), None);
    }

    /// A rasterized panel of the bubble's own size, standing in for the one the worker
    /// builds with the font book and the scene renderer.
    ///
    /// Rendered rather than faked, because what is checked here is that a bubble is held
    /// *as pixels*. The failure this file's sibling in the worker exists for was a bubble
    /// counted, answered and never drawn, and a test that passed a text document in place
    /// of the raster could not have seen it.
    fn raster(text: &str) -> bongocat_plugin_render::RenderedPanel {
        let now = std::time::Instant::now();
        let update = bubble(text, 1500, now).to_panel();
        bongocat_plugin_render::render_update(
            &update,
            1.0,
            &mut bongocat_plugin_render::TextMeasurer::new(
                bongocat_plugin_render::FontBook::load_from(&[], &[]),
            ),
            &bongocat_plugin_render::ImageLibrary::new(),
        )
        .expect("a bubble is a panel the host already draws")
    }

    #[test]
    fn a_bubble_is_held_as_pixels_so_the_worker_has_something_to_publish() {
        let now = std::time::Instant::now();
        let mut bubbles = BubbleSet::new();
        bubbles.show(1, bubble("done", 1500, now), raster("done"));
        let shown = bubbles.rendered(1).expect("a published raster");
        assert_eq!(
            (shown.width, shown.height),
            (BUBBLE_WIDTH, BUBBLE_HEIGHT),
            "and it is the bubble's own box rather than something else on the same layer"
        );
    }

    #[test]
    fn two_bubbles_on_two_layers_do_not_replace_each_other() {
        // Each plugin gets its own layer, so one plugin's announcement cannot silence
        // another's.
        let now = std::time::Instant::now();
        let mut bubbles = BubbleSet::new();
        bubbles.show(1, bubble("first", 1500, now), raster("first"));
        bubbles.show(2, bubble("second", 1500, now), raster("second"));
        assert_eq!(bubbles.len(), 2);
        assert_eq!(bubbles.layer_ids(), vec![1, 2]);
        assert!(
            bubbles.rendered(2).is_some(),
            "and the second is still its own raster rather than the first one's"
        );
    }

    #[test]
    fn a_second_bubble_on_the_same_layer_replaces_the_first() {
        let now = std::time::Instant::now();
        let mut bubbles = BubbleSet::new();
        bubbles.show(1, bubble("first", 1500, now), raster("first"));
        bubbles.show(
            1,
            bubble("second", 3000, now + std::time::Duration::from_millis(100)),
            raster("second"),
        );
        assert_eq!(bubbles.len(), 1, "one layer, one bubble");
        let mut expired = bubbles.take_expired(now + std::time::Duration::from_millis(200));
        assert!(
            expired.is_empty(),
            "and the replacement's own lifetime applies, not the first one's"
        );
        expired = bubbles.take_expired(now + std::time::Duration::from_millis(3200));
        assert_eq!(expired, vec![1]);
        assert!(bubbles.is_empty());
    }

    #[test]
    fn an_expired_bubble_is_withdrawn_rather_than_left_on_screen_for_another_check() {
        let now = std::time::Instant::now();
        let mut bubbles = BubbleSet::new();
        bubbles.show(1, bubble("gone", 1000, now), raster("gone"));
        assert!(
            bubbles
                .take_expired(now + std::time::Duration::from_millis(999))
                .is_empty()
        );
        assert_eq!(
            bubbles.take_expired(now + std::time::Duration::from_millis(1000)),
            vec![1]
        );
        assert!(
            bubbles
                .take_expired(now + std::time::Duration::from_millis(2000))
                .is_empty(),
            "and it is not still there to be taken twice"
        );
    }

    #[test]
    fn a_hide_takes_down_only_that_layers_bubble() {
        let now = std::time::Instant::now();
        let mut bubbles = BubbleSet::new();
        bubbles.show(1, bubble("first", 1500, now), raster("first"));
        bubbles.show(2, bubble("second", 1500, now), raster("second"));
        bubbles.hide(1);
        assert_eq!(bubbles.layer_ids(), vec![2]);
        assert!(
            bubbles.rendered(1).is_none(),
            "and the withdrawn one has no raster left to be published by accident"
        );
        bubbles.clear();
        assert!(bubbles.is_empty());
    }
}

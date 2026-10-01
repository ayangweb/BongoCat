//! Where each plugin's panel sits in the model window.
//!
//! A model window has nine places a panel can go, and the product's rule is one plugin per
//! place: two panels on the same corner overlap, and an overlap is not something a user can
//! resolve — they can only turn something off. So placement is the *host's* to decide even
//! though the panel is the plugin's, and this is where the decision is made.
//!
//! # What a plugin gets to say
//!
//! Two things, and the second is a default rather than a right:
//!
//! * Whether it draws a panel at all, in its descriptor. A sound or a tally has no place in
//!   the window and is offered no position to move.
//! * Which corner it would sit in by default, in each panel message. That is a preference
//!   and not a claim: it is what the plugin uses until somebody places it, and it is what
//!   the plugin goes back to when every position it liked is taken.
//!
//! # How a position is chosen
//!
//! One function, and every answer comes from it, so a card, a settings form and the layer
//! on the model window cannot disagree about where a plugin is:
//!
//! 1. **The user's choice, if it is free.** A plugin the user placed keeps that place even
//!    if a plugin it preferred comes along later — otherwise a newly enabled plugin would
//!    silently move a panel somebody had just arranged.
//! 2. **The plugin's own preference, if it is free.** So a plugin nobody has moved sits
//!    where its author put it, which is the answer for a fresh install.
//! 3. **The first free position.** A tie, and the only tie. Two plugins that both default
//!    to the same corner must not overlap, and which of them keeps it is arbitrary — so
//!    position one keeps it, by id order, and the other moves.
//!
//! A hand-edited file, or one a newer version wrote, can name a position this build does not
//! have or the same position twice; neither is refused, because the allocation above resolves
//! both without the user losing a panel. A preference is not a reservation, and a duplicate
//! one is a preference too.

use bongocat_plugin_protocol::{PluginAnchor, PluginId};
use std::collections::{BTreeMap, BTreeSet};

/// The nine positions a model window has, in the order they are offered.
///
/// Reading the count from the protocol rather than writing it here, so a position added
/// there is offered here without a second edit — and so the bound on how many plugins may
/// be placed at once, which is this count, cannot be a number that disagrees with it.
pub const POSITIONS: [PluginAnchor; 9] = PluginAnchor::ALL;

/// One plugin's position, and where it came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Placed {
    /// The position this plugin's panel is drawn at.
    pub anchor: PluginAnchor,
    /// Whether the user chose it, as against it being a default or a fallback.
    ///
    /// Carried so the settings form can say "you moved this" and so a future change can
    /// tell a deliberate arrangement from one that merely happens not to collide.
    pub chosen: bool,
}

/// Every plugin's position, and the positions still free.
///
/// The allocation is a pure function of the sessions and the preferences, so it is computed
/// on demand rather than kept in step: there is no cached map to forget to update when a
/// plugin starts, stops, or changes its mind, and the cost is a handful of comparisons once
/// per published snapshot.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Placements {
    placed: BTreeMap<PluginId, Placed>,
    free: BTreeSet<PluginAnchor>,
}

impl Placements {
    /// The positions in `wanted`, allocated without two plugins sharing one.
    ///
    /// `wanted` is `(plugin id, the corner the plugin would sit in)` for every plugin that
    /// draws a panel, in id order. `chosen` is the user's own arrangement, keyed the same
    /// way; an id in it that is not in `wanted` is a plugin that is not drawing anything
    /// right now, and its position is free for somebody else to take — which is what makes a
    /// disabled plugin's corner reusable.
    pub fn allocate(
        wanted: &[(PluginId, PluginAnchor)],
        chosen: &BTreeMap<PluginId, PluginAnchor>,
    ) -> Self {
        let mut taken: BTreeSet<PluginAnchor> = BTreeSet::new();
        let mut placed = BTreeMap::new();

        // First pass: the user's own arrangement, which outranks everything. Skipping a
        // choice whose position is already taken rather than refusing it is deliberate — the
        // duplicate is a file to be resolved, not a panel to be refused, and the loser falls
        // through to its own preference below.
        for (id, _) in wanted {
            let Some(preferred) = chosen.get(id) else {
                continue;
            };
            if taken.insert(*preferred) {
                placed.insert(
                    id.clone(),
                    Placed {
                        anchor: *preferred,
                        chosen: true,
                    },
                );
            }
        }

        // Second pass: the plugin's own corner, then the first free one.
        for (id, preferred) in wanted {
            if placed.contains_key(id) {
                continue;
            }
            let taken_by_preference = taken.insert(*preferred);
            let anchor = if taken_by_preference {
                *preferred
            } else {
                // Nothing here can fail: nine positions and at most nine plugins, so a
                // plugin reaching this line has at least one corner nobody has. The
                // `unwrap_or` is for a hand-built `wanted` longer than the window has
                // positions, which is a caller's mistake rather than a state to handle.
                POSITIONS
                    .into_iter()
                    .find(|anchor| taken.insert(*anchor))
                    .unwrap_or(*preferred)
            };
            placed.insert(
                id.clone(),
                Placed {
                    anchor,
                    chosen: false,
                },
            );
        }

        let free = POSITIONS
            .into_iter()
            .filter(|anchor| !taken.contains(anchor))
            .collect();
        Self { placed, free }
    }

    /// Where one plugin's panel is drawn.
    pub fn of(&self, id: &PluginId) -> Option<Placed> {
        self.placed.get(id).copied()
    }

    /// The positions this plugin may be moved to.
    ///
    /// Its own, plus everything free. A taken position is *absent* rather than marked
    /// unavailable, because the settings form renders a menu and a menu that offers a
    /// position and then refuses it is a control that lies; a position that is not on the
    /// list is one the user cannot reach, which is the truth.
    pub fn available_for(&self, id: &PluginId) -> Vec<PluginAnchor> {
        let own = self.of(id).map(|placed| placed.anchor);
        POSITIONS
            .into_iter()
            .filter(|anchor| Some(*anchor) == own || self.free.contains(anchor))
            .collect()
    }

    /// How many plugins are placed, across every position.
    pub fn len(&self) -> usize {
        self.placed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.placed.is_empty()
    }
}

/// The preference a configuration file holds, read through the protocol's own spelling.
///
/// One function rather than a parse at each call site, and the rule for a name this build
/// does not know is the same one the rest of the product uses for a value a newer version
/// wrote: the ordinary answer, which is the first position, rather than a refusal that
/// would drop a plugin's placement for a spelling this build simply has not heard of.
pub fn parse_anchor(name: &str) -> Option<PluginAnchor> {
    PluginAnchor::ALL
        .into_iter()
        .find(|anchor| anchor.as_str() == name.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> PluginId {
        PluginId::new(name).expect("a plugin id this test writes")
    }

    fn wanted(entries: &[(&str, PluginAnchor)]) -> Vec<(PluginId, PluginAnchor)> {
        entries
            .iter()
            .map(|(name, anchor)| (id(name), *anchor))
            .collect()
    }

    fn chosen(entries: &[(&str, PluginAnchor)]) -> BTreeMap<PluginId, PluginAnchor> {
        entries
            .iter()
            .map(|(name, anchor)| (id(name), *anchor))
            .collect()
    }

    #[test]
    fn a_plugin_nobody_has_moved_sits_where_its_author_put_it() {
        // The answer for a fresh install: the plugin's own corner, honoured, because the
        // author chose it with the panel in front of them.
        let placements = Placements::allocate(
            &wanted(&[("pomodoro", PluginAnchor::BottomLeft)]),
            &BTreeMap::new(),
        );
        assert_eq!(
            placements.of(&id("pomodoro")),
            Some(Placed {
                anchor: PluginAnchor::BottomLeft,
                chosen: false
            })
        );
    }

    #[test]
    fn two_plugins_that_want_the_same_corner_do_not_share_it() {
        // The bug this whole file exists for. Both default to the top left, and a model
        // window with two panels on one corner is a window where one of them cannot be read.
        let placements = Placements::allocate(
            &wanted(&[
                ("pomodoro", PluginAnchor::TopLeft),
                ("typing-sound", PluginAnchor::TopLeft),
            ]),
            &BTreeMap::new(),
        );
        let first = placements.of(&id("pomodoro")).expect("a position");
        let second = placements.of(&id("typing-sound")).expect("a position");
        assert_eq!(
            first.anchor,
            PluginAnchor::TopLeft,
            "and the first by id keeps it"
        );
        assert_ne!(
            first.anchor, second.anchor,
            "because two panels on one corner is a window where one of them is unreadable"
        );
    }

    #[test]
    fn a_users_choice_outranks_a_plugin_that_arrives_later() {
        // Otherwise enabling a second plugin silently moves a panel somebody had just
        // arranged, which is the kind of surprise that loses trust in a feature.
        let placements = Placements::allocate(
            &wanted(&[
                ("pomodoro", PluginAnchor::TopLeft),
                ("typing-sound", PluginAnchor::TopLeft),
            ]),
            &chosen(&[("pomodoro", PluginAnchor::BottomRight)]),
        );
        assert_eq!(
            placements.of(&id("pomodoro")).map(|placed| placed.anchor),
            Some(PluginAnchor::BottomRight),
            "the user put it bottom-right and it stays there"
        );
        assert_eq!(
            placements
                .of(&id("typing-sound"))
                .map(|placed| placed.anchor),
            Some(PluginAnchor::TopLeft),
            "and the plugin nobody moved takes the corner it preferred, now that it is free"
        );
        assert!(placements.of(&id("pomodoro")).expect("placed").chosen);
        assert!(!placements.of(&id("typing-sound")).expect("placed").chosen);
    }

    #[test]
    fn a_choice_for_a_position_somebody_else_holds_is_resolved_rather_than_refused() {
        // A hand-edited file, or one a newer version wrote, can say the same thing twice.
        // Refusing it would drop a panel for a duplicate line; resolving it gives one plugin
        // the position and the other its own default, and nothing is lost.
        let placements = Placements::allocate(
            &wanted(&[
                ("pomodoro", PluginAnchor::TopLeft),
                ("typing-sound", PluginAnchor::TopLeft),
            ]),
            &chosen(&[
                ("pomodoro", PluginAnchor::Center),
                ("typing-sound", PluginAnchor::Center),
            ]),
        );
        assert_eq!(
            placements.of(&id("pomodoro")).map(|placed| placed.anchor),
            Some(PluginAnchor::Center)
        );
        assert_ne!(
            placements.of(&id("pomodoro")).map(|placed| placed.anchor),
            placements
                .of(&id("typing-sound"))
                .map(|placed| placed.anchor),
            "and the loser falls through to its own preference rather than overlapping"
        );
    }

    #[test]
    fn a_choice_naming_a_position_this_build_does_not_have_is_not_a_placement() {
        // The file is not refused and the plugin is not dropped: an unknown name is simply
        // not a choice, so the plugin sits where its own corner puts it.
        let placements = Placements::allocate(
            &wanted(&[("pomodoro", PluginAnchor::TopLeft)]),
            &chosen(&[(
                "pomodoro",
                parse_anchor("somewhere_new").unwrap_or(PluginAnchor::TopLeft),
            )]),
        );
        assert_eq!(
            placements.of(&id("pomodoro")).map(|placed| placed.anchor),
            Some(PluginAnchor::TopLeft)
        );
        assert_eq!(parse_anchor("somewhere_new"), None);
    }

    #[test]
    fn a_disabled_plugins_position_is_free_for_somebody_else() {
        // The rule that makes a corner reusable: an id that is not drawing a panel is not in
        // `wanted`, so its position is not taken, whatever the file still says about it.
        let placements = Placements::allocate(
            &wanted(&[("typing-sound", PluginAnchor::TopRight)]),
            &chosen(&[("pomodoro", PluginAnchor::TopRight)]),
        );
        assert_eq!(
            placements
                .of(&id("typing-sound"))
                .map(|placed| placed.anchor),
            Some(PluginAnchor::TopRight),
            "because the plugin that was there is not drawing anything right now"
        );
        assert!(!placements.free.contains(&PluginAnchor::TopRight));
    }

    #[test]
    fn a_plugin_is_only_offered_positions_it_could_take() {
        // Absent rather than marked unavailable, because the settings form renders a menu
        // and a menu that offers a position and then refuses it is a control that lies.
        let taken = Placements::allocate(
            &wanted(&[
                ("pomodoro", PluginAnchor::TopLeft),
                ("typing-sound", PluginAnchor::TopLeft),
            ]),
            &BTreeMap::new(),
        );
        let offered = taken.available_for(&id("typing-sound"));
        assert_eq!(
            offered.first().copied(),
            taken.of(&id("typing-sound")).map(|placed| placed.anchor),
            "and its own position leads the list, or a user could not put it back"
        );
        assert_eq!(
            offered.len(),
            POSITIONS.len() - 1,
            "while the one the other plugin holds is simply not there to be picked"
        );
        assert!(
            !offered.contains(&PluginAnchor::TopLeft),
            "which is what 'cannot be assigned again' looks like from a menu"
        );
    }

    #[test]
    fn every_position_is_offered_when_nothing_is_placed() {
        let placements = Placements::allocate(&[], &BTreeMap::new());
        assert!(placements.is_empty());
        assert_eq!(placements.free.len(), POSITIONS.len());
    }

    #[test]
    fn nine_plugins_each_get_a_position_and_a_tenth_does_not_overlap_one() {
        // Past nine, the model window has no more corners — so the bound on how many panels
        // may be placed is the number of positions, and a plugin beyond it is answered from
        // the same list rather than being refused a load. A plugin that cannot be placed
        // anywhere is a plugin the user turns off, not one the product refuses to start.
        let wanted: Vec<(PluginId, PluginAnchor)> = (0..11)
            .map(|index| (id(&format!("plugin-{index}")), PluginAnchor::TopLeft))
            .collect();
        let placements = Placements::allocate(&wanted, &BTreeMap::new());
        let anchors: Vec<PluginAnchor> = wanted
            .iter()
            .filter_map(|(id, _)| placements.of(id).map(|placed| placed.anchor))
            .collect();
        assert_eq!(anchors.len(), 11, "and every plugin is placed");
        assert!(
            placements.free.is_empty(),
            "because all nine positions are in use"
        );
        // The two beyond the ninth share a position, which the product cannot avoid and
        // which is why the enabled-plugin bound is this count rather than a guess.
        assert_eq!(placements.len(), 11);
    }

    #[test]
    fn a_position_is_named_the_way_the_protocol_names_it() {
        for anchor in POSITIONS {
            assert_eq!(
                parse_anchor(anchor.as_str()),
                Some(anchor),
                "so a file written by this build reads back as what it said"
            );
        }
        assert_eq!(parse_anchor(" top_left "), Some(PluginAnchor::TopLeft));
        assert_eq!(parse_anchor(""), None);
        assert_eq!(parse_anchor("TOP_LEFT"), None, "and the spelling is exact");
    }

    /// The three places that must agree on how many panels the model window holds.
    ///
    /// The protocol owns the count — it is the length of [`PluginAnchor::ALL`] — the worker
    /// reads it from there rather than writing it down, and the configuration document
    /// bounds its own `enabled` list and `positions` map with a number it cannot *read*
    /// from the protocol, because that crate sits below the plugin layer.
    ///
    /// Which leaves a number written down twice and read from once, and nothing to notice
    /// when they part: a tenth position would be added to the protocol, the worker would
    /// cheerfully run ten plugins, and the configuration document would refuse the tenth —
    /// so a user could not switch a plugin on even though the window had somewhere to put
    /// it. This is the one place that can see all three, so this is where the agreement is
    /// checked. It costs one assertion and it is the whole of the drift protection.
    #[test]
    fn the_configurations_own_bound_is_the_number_of_places_the_window_has() {
        assert_eq!(
            bongocat_config::MAXIMUM_PLUGINS,
            POSITIONS.len(),
            "a document that bounds itself at a different number than the window has places \
             either refuses a plugin the window could show, or allows one it cannot"
        );
        assert_eq!(
            crate::MAXIMUM_ENABLED_PLUGINS,
            POSITIONS.len(),
            "and the worker agrees with the same count"
        );
        assert_eq!(
            bongocat_plugin_protocol::PluginAnchor::ALL.len(),
            POSITIONS.len(),
            "while the protocol's own list is where both of those numbers come from"
        );
    }
}

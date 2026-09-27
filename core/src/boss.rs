//! Per-blind state that some boss modifiers need to remember.
//!
//! Most bosses are stateless — a debuff is a pure function of the card. The
//! ones here need something remembered between hands, or across the whole
//! ante, so it lives here rather than being derived on demand.

use crate::joker::Jokers;
use crate::rank::HandRank;
use std::collections::HashSet;

/// Card ids, not cards: a card's id is stable across the deck, hand and
/// discard piles, which is what every one of these sets actually means.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Default)]
pub(crate) struct BossState {
    /// The Eye: the hand type played last this blind. Playing it again is
    /// illegal.
    pub(crate) last_played: Option<HandRank>,
    /// The Mouth: the type the first hand set, and the only one legal for
    /// the rest of the blind.
    pub(crate) locked_rank: Option<HandRank>,
    /// The House: the first hand of the blind hasn't been dealt yet, so it
    /// is the one that comes up face down.
    pub(crate) first_hand_dealt: bool,
    /// Crimson Heart: the Joker debuffed for the hand being played.
    pub(crate) debuffed_joker: Option<Jokers>,
    /// Verdant Leaf: every card debuffed until a Joker is sold.
    pub(crate) all_cards_debuffed: bool,
    /// Pillar: cards already played this ante. Rebuilt on save load, which
    /// only ever restores the current ante's state.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) played_this_ante: HashSet<usize>,
    /// Cerulean Bell: the card forced to stay selected.
    pub(crate) forced_card: Option<usize>,
}

impl BossState {
    /// Clears everything that is scoped to a single blind. Called when a
    /// new blind is selected, and when the run moves to a new ante.
    pub(crate) fn on_new_blind(&mut self) {
        self.last_played = None;
        self.locked_rank = None;
        self.first_hand_dealt = false;
        self.debuffed_joker = None;
        self.forced_card = None;
    }

    /// Pillar's set spans a whole ante rather than a blind, so it is only
    /// cleared when the ante changes.
    pub(crate) fn on_new_ante(&mut self) {
        self.on_new_blind();
        self.played_this_ante.clear();
    }
}

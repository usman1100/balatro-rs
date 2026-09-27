pub use balatro_types::Tag;

use balatro_types::{Edition, Rarity};

use crate::rank::HandRank;

/// Tag effects that don't resolve the instant the tag is collected.
///
/// Everything a tag does at collection time is handled by
/// `Game::apply_tag`; this is the residue — effects aimed at a later shop,
/// a later blind, or the next round. Cleared wholesale whenever the thing it
/// was waiting for happens, so a stale entry can't fire twice.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Default)]
pub(crate) struct TagState {
    /// Investment: $25 owed after the next boss blind is defeated.
    pub(crate) investment: bool,
    /// Foil / Holographic / Polychrome / Negative: the edition the next
    /// base-edition shop Joker becomes (and is made free). Cleared on the
    /// first base-edition Joker the shop generates.
    pub(crate) pending_edition: Option<Edition>,
    /// Uncommon / Rare: a free Joker of that rarity for the next shop.
    pub(crate) pending_free_joker: Option<Rarity>,
    /// Double: the next tag collected is collected twice.
    pub(crate) double: bool,
    /// Coupon: the next shop's Jokers, consumables and packs are all free.
    pub(crate) coupon: bool,
    /// Juggle: extra hand size for the next round only.
    pub(crate) hand_size_bonus: usize,
    /// D6: the next shop's rerolls start at $0.
    pub(crate) free_rerolls: bool,
    /// Orbital: the hand to level up, chosen when the tag is *drawn* so the
    /// player can see it before deciding to skip.
    pub(crate) orbital_hand: Option<HandRank>,
    /// Voucher: the next shop offers a second voucher.
    pub(crate) voucher_slot: bool,
}

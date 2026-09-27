use crate::card::Edition;
use crate::planet::Planets;
use crate::spectral::Spectral;
use crate::tarot::Tarot;
#[cfg(feature = "python")]
use pyo3::pyclass;

/// Which consumable a [`Consumable`] is, ignoring its edition. Kept separate
/// so the edition can be attached without changing every match on the
/// consumable's variant.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "python", pyclass(eq))]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ConsumableKind {
    Planet(Planets),
    Tarot(Tarot),
    Spectral(Spectral),
}

/// A consumable card and its edition.
///
/// The edition is part of the card's identity, not decoration: a Negative
/// Hanged Man and a base Hanged Man are two different cards, which is the
/// whole point of the Negativer and of Perkeo. Equality and hashing
/// therefore include it, so a shop can offer a base copy of something you
/// already hold a Negative of.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "python", pyclass(eq))]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct Consumable {
    pub kind: ConsumableKind,
    pub edition: Edition,
}

/// What a Negative edition adds to a consumable's price, and what the
/// Playing Card equivalent costs. The real game applies the same
/// `+ 5` in `Card:set_cost` for every card type.
const NEGATIVE_COST: usize = 5;

impl Consumable {
    pub fn planet(p: Planets) -> Self {
        Self::base(ConsumableKind::Planet(p))
    }
    pub fn tarot(t: Tarot) -> Self {
        Self::base(ConsumableKind::Tarot(t))
    }
    pub fn spectral(s: Spectral) -> Self {
        Self::base(ConsumableKind::Spectral(s))
    }

    fn base(kind: ConsumableKind) -> Self {
        Self {
            kind,
            edition: Edition::Base,
        }
    }

    /// A Negative copy. The real game gets these from the Negativer, which
    /// upgrades a held consumable in place; Perkeo mints a copy instead.
    pub fn negative(kind: ConsumableKind) -> Self {
        Self {
            kind,
            edition: Edition::Negative,
        }
    }

    /// Strips the edition, as Ankh does for Jokers.
    pub fn unedited(&self) -> Self {
        Self::base(self.kind)
    }

    pub fn is_negative(&self) -> bool {
        self.edition == Edition::Negative
    }

    /// A Negative consumable is slot-free: the real game raises the
    /// consumable card limit by one when the edition is applied, which nets
    /// out to the card not taking up a slot of its own.
    pub fn takes_a_slot(&self) -> bool {
        !self.is_negative()
    }

    pub fn cost(&self) -> usize {
        self.kind.cost() + if self.is_negative() { NEGATIVE_COST } else { 0 }
    }

    pub fn name(&self) -> &str {
        self.kind.name()
    }

    pub fn type_label(&self) -> &str {
        self.kind.type_label()
    }

    pub fn description(&self) -> String {
        self.kind.description()
    }

    /// The real game derives a card's sell value from the price it cost,
    /// so a Negative consumable sells for more than its base version.
    pub fn sell_value(&self) -> usize {
        (self.cost() / 2).max(1)
    }

    /// Save-file id for this consumable's centre. Editions are stored
    /// separately on the card, so two editions of one consumable share an id.
    pub fn id(&self) -> &'static str {
        self.kind.id()
    }

    /// Parses a save-file id into a base-edition [`Consumable`].
    pub fn from_id(s: &str) -> Option<Self> {
        Planets::from_id(s)
            .map(Self::planet)
            .or_else(|| Tarot::from_id(s).map(Self::tarot))
            .or_else(|| Spectral::from_id(s).map(Self::spectral))
    }
}

impl ConsumableKind {
    pub fn cost(&self) -> usize {
        match self {
            Self::Planet(p) => p.cost(),
            Self::Tarot(t) => t.cost(),
            Self::Spectral(s) => s.cost(),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Planet(p) => p.name(),
            Self::Tarot(t) => t.name(),
            Self::Spectral(s) => s.name(),
        }
    }

    pub fn type_label(&self) -> &str {
        match self {
            Self::Planet(_) => "planet",
            Self::Tarot(_) => "tarot",
            Self::Spectral(_) => "spectral",
        }
    }

    pub fn description(&self) -> String {
        match self {
            Self::Planet(p) => p.desc(),
            Self::Tarot(t) => t.description().to_string(),
            Self::Spectral(s) => s.description().to_string(),
        }
    }

    /// Save-file id for this consumable's centre.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Planet(p) => p.id(),
            Self::Tarot(t) => t.id(),
            Self::Spectral(s) => s.id(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consumable_planet_cost() {
        assert_eq!(Consumable::planet(Planets::Mercury).cost(), 3);
    }

    #[test]
    fn test_negative_consumable_costs_five_more_and_sells_for_more() {
        let base = Consumable::tarot(Tarot::Death);
        let negative = Consumable::negative(base.kind);
        assert_eq!(negative.cost(), base.cost() + NEGATIVE_COST);
        assert!(negative.sell_value() > base.sell_value());
    }

    #[test]
    fn test_a_negative_consumable_is_a_different_card() {
        let base = Consumable::tarot(Tarot::Death);
        let negative = Consumable::negative(base.kind);
        assert_ne!(base, negative, "you can hold both, which is the point");
        assert_eq!(base.id(), negative.id(), "but they share a centre");
    }

    #[test]
    fn test_a_negative_consumable_is_slot_free() {
        assert!(Consumable::tarot(Tarot::Death).takes_a_slot());
        assert!(!Consumable::negative(ConsumableKind::Tarot(Tarot::Death)).takes_a_slot());
    }

    #[test]
    fn test_unedited_strips_only_the_edition() {
        let negative = Consumable::negative(ConsumableKind::Tarot(Tarot::Death));
        assert_eq!(negative.unedited(), Consumable::tarot(Tarot::Death));
    }
}

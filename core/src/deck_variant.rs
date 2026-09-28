use crate::card::{Card, Suit, Value};
use balatro_types::{DeckVariant, Stake};
use strum::IntoEnumIterator;

/// The opening card set for `variant`, built from the full 52.
///
/// Abandoned drops every face card and Checkered drops three of the four
/// suits, so both shrink the deck rather than reshuffle it; the rest are the
/// plain 52.
pub fn starting_cards(variant: DeckVariant) -> Vec<Card> {
    let mut cards: Vec<Card> = Value::iter()
        .flat_map(|v| Suit::iter().map(move |s| Card::new(v, s)))
        .filter(|c| match variant {
            DeckVariant::Abandoned => !c.is_face_card(),
            DeckVariant::Checkered => matches!(c.suit, Suit::Spade | Suit::Heart),
            DeckVariant::Erratic => true,
            _ => true,
        })
        .collect();
    if variant == DeckVariant::Erratic {
        // Every card gets a random rank and suit. The real game draws them
        // one at a time from its own pool; we reuse the same values and suits
        // and shuffle, which is indistinguishable once shuffled.
        let values: Vec<Value> = Value::iter().collect();
        let suits: Vec<Suit> = Suit::iter().collect();
        for card in &mut cards {
            card.value = values[(card.id * 7 + 3) % values.len()];
            card.suit = suits[(card.id * 5 + 1) % suits.len()];
        }
    }
    cards
}

/// The deck's starting hand size, added on top of the configured one.
/// Painted is the only deck that changes it.
fn hand_size_delta(variant: DeckVariant) -> isize {
    match variant {
        DeckVariant::Painted => 2,
        _ => 0,
    }
}

/// The deck's starting joker slots, added on top of the configured one.
/// Black adds one and Painted takes one away.
fn joker_slot_delta(variant: DeckVariant) -> isize {
    match variant {
        DeckVariant::Black => 1,
        DeckVariant::Painted => -1,
        _ => 0,
    }
}

/// The deck's starting consumable slots. Nebula takes one away.
fn consumable_slot_delta(variant: DeckVariant) -> isize {
    match variant {
        DeckVariant::Nebula => -1,
        _ => 0,
    }
}

/// Extra starting money. Yellow is the only one.
fn money_bonus(variant: DeckVariant) -> i64 {
    match variant {
        DeckVariant::Yellow => 10,
        _ => 0,
    }
}

/// Vouchers the deck starts holding.
fn starting_vouchers(variant: DeckVariant) -> Vec<balatro_types::Voucher> {
    match variant {
        DeckVariant::Magic => vec![balatro_types::Voucher::CrystalBall],
        DeckVariant::Nebula => vec![balatro_types::Voucher::Telescope],
        DeckVariant::Zodiac => vec![
            balatro_types::Voucher::TarotMerchant,
            balatro_types::Voucher::PlanetMerchant,
            balatro_types::Voucher::Overstock,
        ],
        _ => vec![],
    }
}

/// Consumables the deck starts holding. Magic gets two Fool, Ghost a Hex.
fn starting_consumables(variant: DeckVariant) -> Vec<crate::consumable::Consumable> {
    use crate::consumable::Consumable;
    use balatro_types::{Spectral, Tarot};
    match variant {
        DeckVariant::Magic => vec![
            Consumable::tarot(Tarot::Fool),
            Consumable::tarot(Tarot::Fool),
        ],
        DeckVariant::Ghost => vec![Consumable::spectral(Spectral::Hex)],
        _ => vec![],
    }
}

/// Green Deck pays out for unspent hands and discards at the end of a round
/// instead of paying interest. `extra_hand_bonus` and
/// `extra_discard_bonus` from the deck's config.
pub fn green_deck_end_of_round(hand: i64, discard: i64) -> i64 {
    const EXTRA_HAND_BONUS: i64 = 2;
    const EXTRA_DISCARD_BONUS: i64 = 1;
    hand * EXTRA_HAND_BONUS + discard * EXTRA_DISCARD_BONUS
}

/// The Green Deck's `no_interest` flag.
pub fn deck_denies_interest(variant: DeckVariant) -> bool {
    variant == DeckVariant::Green
}

/// The stake's own `stake_level` value, as the save file stores it.
pub fn stake_level(stake: Stake) -> u8 {
    stake.id()
}

/// The stake's effect on per-round resources, transcribed from the real
/// game's stake table (1.0.0 `Game:start_run`).
///
/// Only Blue (stake 5) touches a resource: `-1` discard. The rest are
/// flags or scaling: Red sets `no_blind_reward.Small`, Green sets
/// `scaling = 2`, Black enables eternals, Purple raises scaling to 3, and
/// Orange/Gold enable perishables/rentals.
pub fn stake_discards_delta(stake: Stake) -> isize {
    if stake >= Stake::Blue {
        -1
    } else {
        0
    }
}

/// Red Stake and above: the Small Blind pays no cash reward.
pub fn stake_denies_small_reward(stake: Stake) -> bool {
    stake >= Stake::Red
}

/// The ante-scaling multiplier: 1 by default, 2 from Green, 3 from Purple.
/// The stakes stack rather than replace, which is why Purple is written as
/// an assignment in the source but has to read as a max here.
pub fn stake_scaling(stake: Stake) -> usize {
    match stake {
        s if s >= Stake::Purple => 3,
        s if s >= Stake::Green => 2,
        _ => 1,
    }
}

/// The Small Blind score requirement for each ante, per scaling level.
///
/// Transcribed from the real game's `get_blind_amount` (1.0.0
/// `misc_functions.lua`), which holds one table per `modifiers.scaling`
/// rather than a formula. Index 0 is ante 1.
const ANTE_BASE_SCALING_1: [usize; 8] = [300, 800, 2000, 5000, 11000, 20000, 35000, 50000];
const ANTE_BASE_SCALING_2: [usize; 8] = [300, 900, 2600, 8000, 20000, 36000, 60000, 100000];
const ANTE_BASE_SCALING_3: [usize; 8] = [300, 1000, 3200, 9000, 25000, 60000, 110000, 200000];

/// The effective ante scaling for a run: the stake's, raised by the Plasma
/// Deck's own `ante_scaling = 2`. The deck and the stake both write
/// `modifiers.scaling`, so the higher of the two wins.
pub fn effective_scaling(deck: DeckVariant, stake: Stake) -> usize {
    let deck_scaling = match deck {
        DeckVariant::Plasma => 2,
        _ => 1,
    };
    stake_scaling(stake).max(deck_scaling)
}

/// The base score for `ante` (1-based) at `scaling`.
pub fn ante_base(scaling: usize, ante: usize) -> usize {
    let table = match scaling {
        2 => &ANTE_BASE_SCALING_2,
        3 => &ANTE_BASE_SCALING_3,
        _ => &ANTE_BASE_SCALING_1,
    };
    match ante {
        0 => 100,
        n if n <= 8 => table[n - 1],
        // Past ante 8 the real game extrapolates and rounds to two
        // significant figures. `core` stops at ante 8, so this is only here
        // to keep the function total.
        n => table[7].saturating_mul(2).saturating_pow((n - 8) as u32),
    }
}

/// Hands per round, including the deck's modifier. No stake changes the
/// hand count — Blue Stake takes a discard, not a hand — so there is no
/// stake parameter to drift out of sync with the source.
pub fn plays_per_round(base: usize, deck: DeckVariant) -> usize {
    let delta = match deck {
        DeckVariant::Blue => 1,
        DeckVariant::Black => -1,
        _ => 0,
    };
    (base as isize + delta).max(1) as usize
}

/// Discards per round, including the deck's and the stake's modifiers.
pub fn discards_per_round(base: usize, deck: DeckVariant, stake: Stake) -> usize {
    let mut delta = 0isize;
    delta += match deck {
        DeckVariant::Red => 1,
        _ => 0,
    };
    delta += stake_discards_delta(stake);
    (base as isize + delta).max(0) as usize
}

pub fn deck_hand_size_delta(variant: DeckVariant) -> isize {
    hand_size_delta(variant)
}

pub fn deck_joker_slot_delta(variant: DeckVariant) -> isize {
    joker_slot_delta(variant)
}

pub fn deck_consumable_slot_delta(variant: DeckVariant) -> isize {
    consumable_slot_delta(variant)
}

pub fn deck_money_bonus(variant: DeckVariant) -> i64 {
    money_bonus(variant)
}

pub fn deck_starting_vouchers(variant: DeckVariant) -> Vec<balatro_types::Voucher> {
    starting_vouchers(variant)
}

pub fn deck_starting_consumables(variant: DeckVariant) -> Vec<crate::consumable::Consumable> {
    starting_consumables(variant)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_deck_is_fifty_two() {
        assert_eq!(starting_cards(DeckVariant::Red).len(), 52);
        assert_eq!(starting_cards(DeckVariant::Blue).len(), 52);
        assert_eq!(starting_cards(DeckVariant::Plasma).len(), 52);
    }

    #[test]
    fn test_abandoned_drops_the_twelve_face_cards() {
        let cards = starting_cards(DeckVariant::Abandoned);
        assert_eq!(cards.len(), 40);
        assert!(!cards.iter().any(|c| c.is_face_card()));
    }

    #[test]
    fn test_checkered_drops_the_black_suits() {
        let cards = starting_cards(DeckVariant::Checkered);
        assert_eq!(cards.len(), 26);
        assert!(cards
            .iter()
            .all(|c| matches!(c.suit, Suit::Spade | Suit::Heart)));
    }

    #[test]
    fn test_erratic_keeps_fifty_two_cards_but_varies_them() {
        let cards = starting_cards(DeckVariant::Erratic);
        assert_eq!(cards.len(), 52);
        let first = starting_cards(DeckVariant::Red);
        assert_ne!(cards, first, "the cards are no longer a plain 52");
    }

    #[test]
    fn test_blue_stake_takes_a_discard_not_a_hand() {
        // Blue Stake (level 5) is the only stake touching discards, and
        // every stake from Blue up takes the same one.
        // Red Deck adds its own discard, so Blue Stake cancels it out.
        assert_eq!(discards_per_round(4, DeckVariant::Red, Stake::White), 5);
        assert_eq!(discards_per_round(4, DeckVariant::Red, Stake::Red), 5);
        assert_eq!(discards_per_round(4, DeckVariant::Red, Stake::Blue), 4);
        assert_eq!(discards_per_round(4, DeckVariant::Red, Stake::Gold), 4);
        // A plain deck on Blue Stake really is one discard down.
        assert_eq!(discards_per_round(4, DeckVariant::Plasma, Stake::Blue), 3);
    }

    #[test]
    fn test_plasma_raises_scaling_above_white_stake() {
        assert_eq!(effective_scaling(DeckVariant::Red, Stake::White), 1);
        assert_eq!(effective_scaling(DeckVariant::Plasma, Stake::White), 2);
        // The stake wins where it is higher.
        assert_eq!(effective_scaling(DeckVariant::Plasma, Stake::Purple), 3);
        assert_eq!(effective_scaling(DeckVariant::Red, Stake::Purple), 3);
    }

    #[test]
    fn test_ante_base_tables_match_the_real_game() {
        // Every ante's base at scaling 1, from `get_blind_amount`.
        let expected_1 = [300, 800, 2000, 5000, 11000, 20000, 35000, 50000];
        for (i, want) in expected_1.iter().enumerate() {
            assert_eq!(ante_base(1, i + 1), *want, "ante {}", i + 1);
        }
        // Green stake onward: the scaling-2 table.
        assert_eq!(ante_base(2, 2), 900);
        assert_eq!(ante_base(2, 8), 100000);
        // Purple stake onward: the scaling-3 table.
        assert_eq!(ante_base(3, 2), 1000);
        assert_eq!(ante_base(3, 8), 200000);
        // Ante 0 is the fixed 100 the real game returns for pre-ante-one.
        assert_eq!(ante_base(1, 0), 100);
        assert_eq!(ante_base(3, 0), 100);
    }

    #[test]
    fn test_scaling_never_lowers_a_requirement() {
        for ante in 1..=8 {
            assert!(ante_base(2, ante) >= ante_base(1, ante));
            assert!(ante_base(3, ante) >= ante_base(2, ante));
        }
    }

    #[test]
    fn test_stakes_stack_from_their_level_up() {
        // Transcribed from the 1.0.0 stake table: >= 2 denies the Small
        // Blind reward, >= 3 sets scaling 2, >= 5 takes a discard, >= 6
        // raises scaling to 3.
        assert!(!stake_denies_small_reward(Stake::White));
        assert!(stake_denies_small_reward(Stake::Red));
        assert!(stake_denies_small_reward(Stake::Gold));
        assert_eq!(stake_scaling(Stake::White), 1);
        assert_eq!(stake_scaling(Stake::Red), 1);
        assert_eq!(stake_scaling(Stake::Green), 2);
        assert_eq!(stake_scaling(Stake::Black), 2);
        assert_eq!(stake_scaling(Stake::Purple), 3);
        assert_eq!(stake_scaling(Stake::Gold), 3);
    }

    #[test]
    fn test_blue_deck_adds_a_hand_and_black_takes_one() {
        assert_eq!(plays_per_round(4, DeckVariant::Blue), 5);
        assert_eq!(plays_per_round(4, DeckVariant::Black), 3);
    }

    #[test]
    fn test_red_deck_adds_a_discard() {
        assert_eq!(discards_per_round(4, DeckVariant::Red, Stake::White), 5);
    }

    #[test]
    fn test_hands_never_drop_to_zero() {
        assert_eq!(plays_per_round(1, DeckVariant::Black), 1);
    }
}

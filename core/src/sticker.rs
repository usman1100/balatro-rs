use crate::joker::Jokers;
use balatro_types::{Stake, Stickers};

/// How many rounds a Perishable joker lasts before expiring, from the real
/// game's `G.GAME.perishable_rounds`.
pub const PERISHABLE_ROUNDS: i32 = 5;

/// What a Rental joker charges at the end of each round, from
/// `G.GAME.rental_rate`.
pub const RENTAL_RATE: i64 = 3;

/// What a Rental joker costs to buy. `Card:set_cost` ends with
/// `if self.ability.rental then self.cost = 1 end`, overriding the joker's
/// own price outright rather than discounting it.
pub const RENTAL_COST: usize = 1;

/// Which stake level first allows each sticker. Transcribed from the real
/// game's stake table (`Game:start_run`):
/// `>= 4` eternals, `>= 7` perishables, `>= 8` rentals.
fn allows_eternal(stake: Stake) -> bool {
    stake >= Stake::Black
}
fn allows_perishable(stake: Stake) -> bool {
    stake >= Stake::Orange
}
fn allows_rental(stake: Stake) -> bool {
    stake >= Stake::Gold
}

/// Rolls a joker's stickers for a card appearing in the shop or a pack.
///
/// Transcribed from the real game's `add_joker`. Eternal and Perishable share
/// one roll on `etperpoll` and are mutually exclusive by construction: the
/// `elseif` means a high roll can only be Eternal and a middle roll only
/// Perishable. Rental is a separate roll on `ssjr` and is independent of the
/// other two, so a joker can be both Rental and Eternal.
///
/// The thresholds are `> 0.7` for Eternal and `(0.4, 0.7]` for Perishable,
/// so a single uniform roll decides all three outcomes: 30% Eternal, 30%
/// Perishable, 40% neither.
///
/// `set_eternal` and `set_perishable` both refuse to set themselves when the
/// joker is not compatible for that sticker, and each refuses when the other
/// is already present, so an incompatible joker ends up with no sticker at
/// all rather than the wrong one.
pub fn roll_stickers(
    joker: &mut Jokers,
    stake: Stake,
    eternal_perishable_roll: f64,
    rental_roll: f64,
) {
    let mut stickers = Stickers::default();

    if eternal_roll_clear(eternal_perishable_roll) && allows_eternal(stake) {
        set_eternal(joker, &mut stickers, true);
    } else if perishable_roll_clear(eternal_perishable_roll) && allows_perishable(stake) {
        set_perishable(joker, &mut stickers, true);
    }

    // Rental is independent of the other two and has no compat gate.
    if rental_roll > 0.7 && allows_rental(stake) {
        stickers.rental = true;
    }

    joker.set_stickers(stickers);
}

/// `etperpoll > 0.7`, named so the threshold is not a bare float at the call
/// site.
fn eternal_roll_clear(roll: f64) -> bool {
    roll > 0.7
}

/// `etperpoll > 0.4 and <= 0.7`.
fn perishable_roll_clear(roll: f64) -> bool {
    roll > 0.4 && roll <= 0.7
}

/// `Card:set_eternal`: refused outright when the joker is not eternal
/// compatible, and refused when the joker is already Perishable.
pub fn set_eternal(joker: &Jokers, stickers: &mut Stickers, eternal: bool) {
    stickers.eternal = false;
    if joker.eternal_compatible() && !stickers.perishable {
        stickers.eternal = eternal;
    }
}

/// `Card:set_perishable`: refused when not perishable compatible, refused
/// when already Eternal, and always sets the tally to the full lifetime
/// rather than the argument — the real game ignores its parameter here.
pub fn set_perishable(joker: &Jokers, stickers: &mut Stickers, perishable: bool) {
    stickers.perishable = false;
    if joker.perishable_compatible() && !stickers.eternal {
        stickers.perishable = true;
        stickers.perish_tally = PERISHABLE_ROUNDS;
        let _ = perishable;
    }
}

/// Whether a joker with these stickers can be sold. Eternal is the only
/// sticker that blocks a sale; Perishable and Rental sell normally.
pub fn can_sell(stickers: &Stickers) -> bool {
    !stickers.eternal
}

/// Whether a boss debuff applies. A Perishable that has run out is debuffed
/// and removed, which is what `Card:set_debuff` does before it looks at the
/// requested state.
pub fn is_debuffed_for_expired_perishable(stickers: &Stickers) -> bool {
    stickers.perishable && stickers.perish_tally <= 0
}

/// The price of a joker as the player sees it: Rental overrides the joker's
/// own cost outright.
pub fn price(joker: &Jokers) -> usize {
    if joker.stickers().rental {
        RENTAL_COST
    } else {
        joker.cost()
    }
}

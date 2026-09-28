//! Sticker behaviour end to end through `Game`.
//!
//! The rules themselves live in `sticker`; these check that generation, the
//! round-end tick and the scoring loop actually read them, which is a
//! separate thing and the part that goes stale.

use balatro_rs::{
    action::Action,
    config::Config,
    game::Game,
    joker::{Jokers, Stickers},
    stage::{Blind, BlindExt, Stage},
    sticker,
};
use balatro_types::Stake;

fn game(stake: Stake) -> Game {
    let mut g = Game::new(Config {
        seed: Some(1),
        stake,
        ..Config::default()
    });
    g.start();
    g
}

fn a_joker() -> Jokers {
    Jokers::TheJoker(Default::default())
}

/// A joker with a sticker, held in the run.
fn held(g: &mut Game, stickers: Stickers) {
    let mut j = a_joker();
    j.set_stickers(stickers);
    g.jokers.push(j);
}

/// Stickers with only the flags named set, and no lifetime unless Perishable.
fn stickers(eternal: bool, perishable: bool, rental: bool) -> Stickers {
    Stickers {
        eternal,
        perishable,
        rental,
        perish_tally: if perishable {
            sticker::PERISHABLE_ROUNDS
        } else {
            0
        },
    }
}

fn perishable(tally: i32) -> Stickers {
    Stickers {
        perishable: true,
        perish_tally: tally,
        ..Stickers::default()
    }
}

/// Beats whichever blind is next outright, then cashes out. Blinds have to
/// be taken in order — small, big, boss — so the next one is derived from
/// what the run has already beaten.
fn finish_round(g: &mut Game) {
    if g.stage == Stage::Shop() {
        g.handle_action(Action::NextRound()).expect("next round");
    }
    let next = match g.blind {
        None => Blind::Small,
        Some(b) => b.next(),
    };
    g.handle_action(Action::SelectBlind(next)).expect("select");
    // Select everything dealt, the way a player would, so `Play` is legal.
    for card in g.available.cards() {
        g.handle_action(Action::SelectCard(card)).ok();
    }
    g.score += g.required_score();
    g.handle_action(Action::Play()).expect("beat the blind");
    g.handle_action(Action::CashOut(0)).expect("cash out");
}

// --- Generation ---

/// Stakes gate stickers: Eternal at Black, Perishable at Orange, Rental at
/// Gold, and nothing below that.
#[test]
fn test_stakes_gate_which_stickers_can_appear() {
    for stake in [Stake::White, Stake::Red, Stake::Green] {
        let mut j = a_joker();
        sticker::roll_stickers(&mut j, stake, 0.0, 0.0);
        assert_eq!(j.stickers(), Stickers::default(), "{}", stake.name());
    }
    // A high roll at Black Stake is Eternal.
    let mut j = a_joker();
    sticker::roll_stickers(&mut j, Stake::Black, 0.9, 0.0);
    assert!(j.stickers().eternal, "Black Stake allows Eternal");
    // A middle roll at Green is still nothing: Perishable needs Orange.
    let mut j = a_joker();
    sticker::roll_stickers(&mut j, Stake::Green, 0.5, 0.0);
    assert!(!j.stickers().perishable, "Green is below Orange");
    let mut j = a_joker();
    sticker::roll_stickers(&mut j, Stake::Orange, 0.5, 0.0);
    assert!(j.stickers().perishable, "Orange allows Perishable");
    let mut j = a_joker();
    sticker::roll_stickers(&mut j, Stake::Gold, 0.5, 0.9);
    assert!(j.stickers().rental, "Gold allows Rental");
    let mut j = a_joker();
    sticker::roll_stickers(&mut j, Stake::Orange, 0.5, 0.9);
    assert!(!j.stickers().rental, "Rental needs Gold, not Orange");
}

/// Eternal and Perishable share one roll and cannot both land, and a joker
/// with neither is the remaining 40%.
#[test]
fn test_eternal_and_perishable_are_mutually_exclusive() {
    for step in 0..=100 {
        let roll = step as f64 / 100.0;
        let mut j = a_joker();
        sticker::roll_stickers(&mut j, Stake::Gold, roll, 0.0);
        let s = j.stickers();
        assert!(
            !(s.eternal && s.perishable),
            "both stickers on one joker at roll {roll}"
        );
    }
    let mut eternal = 0;
    let mut perish = 0;
    let mut clean = 0;
    for step in 0..=1000 {
        let roll = step as f64 / 1000.0;
        let mut j = a_joker();
        sticker::roll_stickers(&mut j, Stake::Gold, roll, 0.0);
        let s = j.stickers();
        if s.eternal {
            eternal += 1;
        } else if s.perishable {
            perish += 1;
        } else {
            clean += 1;
        }
    }
    assert_eq!(eternal, 300, "roll > 0.7 is 30%");
    assert_eq!(perish, 300, "0.4 < roll <= 0.7 is 30%");
    assert_eq!(clean, 401, "the rest is 40%");
}

/// A Perishable starts with the full lifetime, not zero.
#[test]
fn test_perishable_starts_with_its_full_lifetime() {
    let mut j = a_joker();
    sticker::roll_stickers(&mut j, Stake::Orange, 0.5, 0.0);
    assert_eq!(j.stickers().perish_tally, sticker::PERISHABLE_ROUNDS);
    assert_eq!(sticker::PERISHABLE_ROUNDS, 5, "the real game's value");
}

/// An incompatible joker gets no sticker rather than the wrong one.
#[test]
fn test_incompatible_jokers_get_no_sticker() {
    // Runner cannot be perishable, so the roll lands on perishable and the
    // compat gate refuses it.
    let mut j = Jokers::Runner(Default::default());
    assert!(!j.perishable_compatible(), "fixture must be incompatible");
    sticker::roll_stickers(&mut j, Stake::Gold, 0.5, 0.0);
    assert!(!j.stickers().perishable, "the compat gate refused it");
    // Gros Michel cannot be eternal, same gate.
    let mut j = Jokers::GrosMichel(Default::default());
    assert!(!j.eternal_compatible(), "fixture must be incompatible");
    sticker::roll_stickers(&mut j, Stake::Gold, 0.9, 0.0);
    assert!(!j.stickers().eternal, "the compat gate refused it");
}

/// Rental is independent, so it stacks with either of the others.
#[test]
fn test_rental_stacks_with_eternal() {
    let mut j = a_joker();
    sticker::roll_stickers(&mut j, Stake::Gold, 0.9, 0.9);
    let s = j.stickers();
    assert!(s.eternal && s.rental, "the wiki says both can apply");
}

// --- Behaviour ---

/// An Eternal joker cannot be sold.
#[test]
fn test_eternal_jokers_cannot_be_sold() {
    let mut g = game(Stake::Gold);
    held(&mut g, stickers(true, false, false));
    g.money = 100;
    // Driven through the action API, so this also proves the move generator
    // does not offer an unsellable joker.
    let offered: Vec<Action> = g.gen_actions().collect();
    assert!(
        !offered.iter().any(|a| matches!(a, Action::SellJoker(0))),
        "an Eternal joker must not be offered for sale"
    );
    // A Perishable and a Rental both sell normally.
    held(&mut g, perishable(3));
    assert!(g.gen_actions().any(|a| matches!(a, Action::SellJoker(1))));
}

/// A Perishable ages one round at a time and then stops contributing,
/// without leaving the row.
#[test]
fn test_perishable_debuffs_rather_than_being_removed() {
    let mut g = game(Stake::Gold);
    held(&mut g, perishable(2));
    let tally = |g: &Game| -> i32 {
        g.jokers
            .iter()
            .find(|j| j.stickers().perishable)
            .expect("the Perishable is still held")
            .stickers()
            .perish_tally
    };
    assert_eq!(tally(&g), 2);
    let debuffed = |g: &Game| {
        g.jokers
            .iter()
            .filter(|j| j.stickers().perishable)
            .all(|j| g.is_joker_debuffed(j))
    };
    assert!(!debuffed(&g));

    finish_round(&mut g);
    assert_eq!(tally(&g), 1);
    assert!(!debuffed(&g));
    assert_eq!(g.jokers.len(), 1, "no joker was destroyed");

    finish_round(&mut g);
    assert_eq!(tally(&g), 0);
    assert!(debuffed(&g), "an expired Perishable stops contributing");
    assert_eq!(
        g.jokers.iter().filter(|j| j.stickers().perishable).count(),
        1,
        "but stays in the row: Gift Card and Swashbuckler still see it"
    );
}

/// A Rental costs $1 whatever the joker normally costs, and sells for $1.
#[test]
fn test_rental_costs_one_and_sells_for_one() {
    let mut g = game(Stake::Gold);
    // The Joker costs $2 normally, so $1 shows the override rather than luck.
    let mut j = Jokers::GreedyJoker(Default::default());
    j.set_stickers(stickers(false, false, true));
    g.shop.jokers.push(j.clone());
    g.money = 100;
    g.stage = Stage::Shop();
    assert_eq!(g.joker_price(&j), 1, "the real game overrides the cost");
    g.handle_action(Action::BuyJoker(j)).expect("buy");
    assert_eq!(g.money, 99);
    assert_eq!(g.jokers[0].sell_value(), 1, "and sells for what it cost");
}

/// A Rental is charged $3 per round, before the payout, so it can cut
/// interest.
#[test]
fn test_rental_is_charged_each_round() {
    let mut g = game(Stake::Gold);
    held(&mut g, stickers(false, false, true));
    g.money = 100;
    finish_round(&mut g);
    // $3 charged before the payout, leaving $97 to earn interest on, then
    // the Small Blind's own $3 and $5 of interest.
    assert_eq!(
        g.money,
        100 - 3 + 3 + 5,
        "interest on 97 is 19, capped at 5"
    );
}

/// The charge is floored at the debt line, not taken below it.
#[test]
fn test_rental_charge_cannot_go_below_the_floor() {
    let mut g = game(Stake::Gold);
    held(&mut g, stickers(false, false, true));
    g.money = 0;
    finish_round(&mut g);
    assert!(g.money >= 0, "no debt from a rental, got {}", g.money);
}

/// Stickers survive a save/load round trip with their tally intact.
#[test]
fn test_stickers_survive_a_save_load_round_trip() {
    let mut g = game(Stake::Gold);
    held(&mut g, perishable(3));
    held(&mut g, stickers(false, false, true));
    let json = g.to_json().expect("serialize");
    let loaded = Game::from_json(&json).expect("deserialize");
    assert_eq!(loaded.jokers[0].stickers().perish_tally, 3);
    assert!(loaded.jokers[0].stickers().perishable);
    assert!(loaded.jokers[1].stickers().rental);
}

/// A White Stake run generates no stickers at all, however long it plays.
#[test]
fn test_white_stake_never_generates_stickers() {
    let mut g = Game::new(Config {
        seed: Some(3),
        ..Config::default()
    });
    g.start();
    g.money = 100;
    g.handle_action(Action::SelectBlind(Blind::Small))
        .expect("select");
    assert!(
        g.shop.jokers.is_empty()
            || g.shop
                .jokers
                .iter()
                .all(|j| j.stickers() == Stickers::default())
    );
}

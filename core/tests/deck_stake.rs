//! Deck and stake behaviour, end to end through `Game`.
//!
//! The per-variant rules live in `deck_variant`; these check that `Game`
//! actually reads them, which is a separate thing and the one that regresses
//! when a new modifier is added there and nowhere else.

use balatro_rs::{
    action::Action,
    config::Config,
    game::Game,
    stage::{Blind, Stage},
};
use balatro_types::{Ante, DeckVariant, Stake};
use rand::{Rng, SeedableRng};
use strum::IntoEnumIterator;

fn game_with(deck: DeckVariant, stake: Stake) -> Game {
    Game::new(Config {
        seed: Some(1),
        deck,
        stake,
        ..Config::default()
    })
}

fn started(mut g: Game) -> Game {
    g.start();
    g
}

/// The deck's card set reaches the run, and the default is a plain 52.
#[test]
fn test_every_deck_produces_a_starting_deck() {
    for deck in DeckVariant::iter() {
        let g = started(game_with(deck, Stake::White));
        assert!(
            !g.full_deck().is_empty(),
            "{} started with an empty deck",
            deck.name()
        );
    }
    assert_eq!(
        started(game_with(DeckVariant::Red, Stake::White))
            .full_deck()
            .len(),
        52
    );
}

#[test]
fn test_abandoned_and_checkered_start_smaller() {
    assert_eq!(
        started(game_with(DeckVariant::Abandoned, Stake::White))
            .full_deck()
            .len(),
        40,
        "12 face cards removed"
    );
    assert_eq!(
        started(game_with(DeckVariant::Checkered, Stake::White))
            .full_deck()
            .len(),
        26,
        "two suits removed"
    );
}

#[test]
fn test_deck_modifiers_change_the_slots_they_say_they_do() {
    let slots = |deck| {
        let g = started(game_with(deck, Stake::White));
        (
            g.hand_size(),
            g.joker_slots(),
            g.consumable_slots(),
            g.discards_per_round(),
            g.plays_per_round(),
        )
    };
    let base = slots(DeckVariant::Red);
    assert_eq!(
        slots(DeckVariant::Painted).0,
        base.0 + 2,
        "Painted hand size"
    );
    assert_eq!(
        slots(DeckVariant::Painted).1,
        base.1 - 1,
        "Painted joker slot"
    );
    assert_eq!(slots(DeckVariant::Black).1, base.1 + 1, "Black joker slot");
    assert_eq!(
        slots(DeckVariant::Nebula).2,
        base.2 - 1,
        "Nebula consumable slot"
    );
    assert_eq!(slots(DeckVariant::Blue).4, base.4 + 1, "Blue hand");
    assert_eq!(slots(DeckVariant::Black).4, base.4 - 1, "Black hand");
}

#[test]
fn test_magic_and_ghost_start_with_consumables() {
    let held = |deck| started(game_with(deck, Stake::White)).consumables.clone();
    assert_eq!(held(DeckVariant::Magic).len(), 2, "two Fool");
    assert_eq!(held(DeckVariant::Ghost).len(), 1, "one Hex");
    assert!(held(DeckVariant::Red).is_empty());
}

#[test]
fn test_decks_start_with_their_vouchers() {
    let owned = |deck| {
        started(game_with(deck, Stake::White))
            .vouchers
            .owned()
            .to_vec()
    };
    assert!(owned(DeckVariant::Magic).contains(&balatro_types::Voucher::CrystalBall));
    assert!(owned(DeckVariant::Nebula).contains(&balatro_types::Voucher::Telescope));
    assert_eq!(owned(DeckVariant::Zodiac).len(), 3);
    assert!(owned(DeckVariant::Red).is_empty());
}

#[test]
fn test_yellow_deck_starts_with_ten_more() {
    let plain = started(game_with(DeckVariant::Red, Stake::White));
    let yellow = started(game_with(DeckVariant::Yellow, Stake::White));
    assert_eq!(yellow.money, plain.money + 10);
}

/// Stakes stack from their level up, and only Blue changes a resource.
#[test]
fn test_stakes_stack_from_their_level() {
    for stake in Stake::iter() {
        let g = started(game_with(DeckVariant::Red, stake));
        // Base 3 + Red Deck's 1 = 4, and Blue Stake's -1 from level 5 up
        // takes it back to 3.
        let expected = if stake >= Stake::Blue { 3 } else { 4 };
        assert_eq!(
            g.discards_per_round(),
            expected,
            "{} discards",
            stake.name()
        );
    }
}

/// Red Stake denies the Small Blind reward and White does not.
#[test]
fn test_red_stake_denies_the_small_blind_reward() {
    let small_reward = |stake| {
        let mut g = started(game_with(DeckVariant::Red, stake));
        g.money = 100;
        g.handle_action(Action::SelectBlind(Blind::Small))
            .expect("select the Small Blind");
        g.cashout_reward(Blind::Small)
    };
    let white = small_reward(Stake::White);
    let red = small_reward(Stake::Red);
    assert!(white > 0);
    assert!(
        red < white,
        "Red Stake pays less at the Small Blind: {red} vs {white}"
    );
}

/// Green and Purple raise the ante base; White leaves it alone.
#[test]
fn test_stakes_raise_the_ante_base() {
    let base = |stake, ante| {
        let mut g = started(game_with(DeckVariant::Red, stake));
        g.ante_current = ante;
        g.ante_base()
    };
    assert_eq!(base(Stake::White, Ante::Two), 800);
    assert_eq!(base(Stake::Green, Ante::Two), 900);
    assert_eq!(base(Stake::Purple, Ante::Two), 1000);
    assert_eq!(base(Stake::Purple, Ante::Eight), 200000);
}

#[test]
fn test_plasma_deck_raises_the_ante_base_too() {
    let mut g = started(game_with(DeckVariant::Plasma, Stake::White));
    g.ante_current = Ante::Three;
    assert_eq!(
        g.ante_base(),
        2600,
        "ante 3 at scaling 2, the same table Green stake uses"
    );
}

/// Green Deck replaces interest with a per-hand and per-discard payout.
#[test]
fn test_green_deck_pays_out_instead_of_interest() {
    let reward = |deck| {
        let mut g = started(game_with(deck, Stake::White));
        g.money = 100;
        g.handle_action(Action::SelectBlind(Blind::Small))
            .expect("select the Small Blind");
        g.cashout_reward(Blind::Small)
    };
    let plain = reward(DeckVariant::Red);
    let green = reward(DeckVariant::Green);
    // $100 held would earn $20 of interest on a plain deck, capped at 5.
    // Green gives the table up entirely and pays 2 per unspent hand and 1
    // per unspent discard instead.
    assert_eq!(plain, 3 + 5 + 4, "blind reward + interest + per hand");
    // `no_interest` only replaces the interest table; the per-hand bonus
    // still applies, so: 3 blind + 4 hands + (2*4 hands + 3 discards).
    assert_eq!(green, 3 + 4 + 2 * 4 + 3, "blind + per hand + Green payout");
}

/// A run's deck and stake survive a save/load round trip.
#[test]
fn test_deck_and_stake_survive_a_save_load_round_trip() {
    let g = game_with(DeckVariant::Checkered, Stake::Purple);
    let json = g.to_json().expect("serialize");
    let loaded = Game::from_json(&json).expect("deserialize");
    assert_eq!(loaded.config.deck, DeckVariant::Checkered);
    assert_eq!(loaded.config.stake, Stake::Purple);
    assert_eq!(loaded.ante_base(), g.ante_base());
}

/// Every deck and stake combination starts a playable run.
#[test]
fn test_every_deck_and_stake_starts_a_playable_run() {
    for deck in DeckVariant::iter() {
        for stake in Stake::iter() {
            let mut g = game_with(deck, stake);
            g.start();
            assert_eq!(g.stage, Stage::PreBlind());
            assert!(
                g.plays_per_round() >= 1,
                "{} / {}",
                deck.name(),
                stake.name()
            );
            assert!(
                g.joker_slots() >= 1,
                "{} / {} left no joker slot",
                deck.name(),
                stake.name()
            );
            assert!(
                g.consumable_slots() >= 1,
                "{} / {} left no consumable slot",
                deck.name(),
                stake.name()
            );
        }
    }
}

/// Every deck/stake combination survives a full run driven by both public
/// move APIs. The point is the new modifiers, so this is where a deck that
/// leaves an invalid slot count or a zero hand count would surface.
#[test]
fn test_every_deck_and_stake_completes_a_run() {
    for deck in DeckVariant::iter() {
        for stake in Stake::iter() {
            for seed in 0..6u64 {
                let mut g = Game::new(Config {
                    seed: Some(seed),
                    deck,
                    stake,
                    ..Config::default()
                });
                g.start();
                let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
                for _ in 0..4000 {
                    if g.is_over() {
                        break;
                    }
                    let space = g.gen_action_space();
                    let mask = space.to_vec();
                    let legal: Vec<usize> = mask
                        .iter()
                        .enumerate()
                        .filter(|(_, m)| **m == 1)
                        .map(|(i, _)| i)
                        .collect();
                    if legal.is_empty() {
                        break;
                    }
                    let pick = legal[rng.gen_range(0..legal.len())];
                    let action = space
                        .to_action(pick, &g)
                        .unwrap_or_else(|e| panic!("{deck:?}/{stake:?}: unmasked index did not convert: {e}"));
                    g.handle_action(action.clone()).unwrap_or_else(|e| {
                        panic!("{deck:?}/{stake:?} seed {seed}: {action:?} was offered but failed: {e:?}")
                    });
                }
                // The run must have actually progressed, not merely avoided
                // panicking. `plays_per_round() >= 1` was true at `start()`
                // and so asserted nothing.
                assert!(
                    g.round > 0 || g.is_over(),
                    "{:?}/{:?} seed {seed}: no hand was ever played",
                    deck.name(),
                    stake.name()
                );
            }
        }
    }
}

/// Every stake plays a full run, which is what exercises stickers: only
/// Black and above can produce them, so a stress pass on the default White
/// Stake would never reach this code.
#[test]
fn test_every_stake_completes_a_run_with_stickers_possible() {
    use rand::{Rng, SeedableRng};
    for stake in Stake::iter() {
        for seed in 0..4u64 {
            let mut g = Game::new(Config {
                seed: Some(seed),
                stake,
                ..Config::default()
            });
            g.start();
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let mut turns = 0;
            for _ in 0..3000 {
                if g.is_over() {
                    break;
                }
                let space = g.gen_action_space();
                let legal: Vec<usize> = space
                    .to_vec()
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| **m == 1)
                    .map(|(i, _)| i)
                    .collect();
                if legal.is_empty() {
                    break;
                }
                let pick = legal[rng.gen_range(0..legal.len())];
                let action = space
                    .to_action(pick, &g)
                    .unwrap_or_else(|e| panic!("{stake:?}: unmasked index did not convert: {e}"));
                g.handle_action(action.clone()).unwrap_or_else(|e| {
                    panic!("{stake:?} seed {seed}: {action:?} was offered but failed: {e:?}")
                });
                turns += 1;
                assert!(
                    turns < 20_000,
                    "{stake:?} seed {seed}: the run never terminated"
                );
            }
            assert!(
                g.round > 0 || g.is_over(),
                "{stake:?} seed {seed}: no hand was ever played"
            );
        }
    }
}

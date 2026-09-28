use balatro_rs::{
    action::Action,
    config::{Config, RngMode},
    game::Game,
    stage::Stage,
    voucher::Voucher,
};
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;

/// Plays a whole run driven by `gen_actions`, with a seeded rng so a
/// failure reproduces. Asserts the run actually finished and that every
/// offered action executed, which the previous version did not: it used
/// `thread_rng()`, so a failure could not be replayed, and it broke out of
/// the loop on an error without failing.
fn run_game_gen_actions(seed: u64) -> Game {
    let mut g = Game::new(Config {
        seed: Some(seed),
        ..Config::default()
    });
    let mut rng = StdRng::seed_from_u64(seed);
    g.start();
    let mut turns = 0;
    while !g.is_over() {
        let actions: Vec<Action> = g.gen_actions().collect();
        if actions.is_empty() {
            break;
        }
        let action = actions[rng.gen_range(0..actions.len())].clone();
        g.handle_action(action.clone())
            .unwrap_or_else(|e| panic!("seed {seed}: offered {action:?} but it failed: {e:?}"));
        turns += 1;
        assert!(turns < 20_000, "seed {seed}: the run never terminated");
    }
    assert!(
        g.result().is_some(),
        "seed {seed}: the run ended without a result"
    );
    assert!(
        matches!(g.stage, Stage::End(_)),
        "seed {seed}: ended in {:?}",
        g.stage
    );
    g
}

/// Plays a whole run driven by the action space. The index is chosen from
/// the unmasked entries rather than by rejection-sampling the whole vector,
/// so it cannot spin on a sparse mask, and `to_action` is checked before
/// execution — which is the pair that was broken by the missing
/// `reroll_boss` arm.
fn run_game_action_space(seed: u64) -> Game {
    let mut g = Game::new(Config {
        seed: Some(seed),
        ..Config::default()
    });
    let mut rng = StdRng::seed_from_u64(seed);
    g.start();
    let mut turns = 0;
    while !g.is_over() {
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
        let i = legal[rng.gen_range(0..legal.len())];
        let action = space
            .to_action(i, &g)
            .unwrap_or_else(|e| panic!("seed {seed}: unmasked index {i} did not convert: {e}"));
        g.handle_action(action)
            .unwrap_or_else(|e| panic!("seed {seed}: unmasked index {i} failed to execute: {e:?}"));
        turns += 1;
        assert!(turns < 20_000, "seed {seed}: the run never terminated");
    }
    assert!(
        g.result().is_some(),
        "seed {seed}: the run ended without a result"
    );
    g
}

/// The two APIs must be able to reach the same conclusion. Playing the same
/// seeds through both and checking both end cleanly is a weaker check than
/// comparing them turn by turn, but it does mean a divergence that makes one
/// of them wedge cannot hide.
fn run_both_apis(seed: u64) {
    run_game_gen_actions(seed);
    run_game_action_space(seed);
}

#[test]
fn test_game() {
    for seed in 0..10 {
        run_both_apis(seed);
    }
}

// `RngMode::Real` routes shop and pack generation through `balatro-seed`,
// including the shop-playing-card branch that only the Magic Trick voucher
// can reach — it used to be an outright `panic!`, since nothing could
// activate it before vouchers existed.
#[test]
fn test_real_rng_mode_shop_playing_cards() {
    for seed in ["TEST", "ABCD1234", "BALATRO"] {
        let mut g = Game::new(Config {
            rng_mode: RngMode::Real,
            seed_str: Some(seed.to_string()),
            ..Config::default()
        });
        g.start();
        // Straight to a stocked shop without having to win a blind.
        g.stage = Stage::PostBlind();
        g.handle_action(Action::CashOut(0)).expect("cash out");

        g.shop.voucher = Some(Voucher::MagicTrick);
        g.money = 1000;
        g.handle_action(Action::BuyVoucher(Voucher::MagicTrick))
            .expect("buy Magic Trick");

        let mut bought = 0;
        for _ in 0..200 {
            // Every purchase grows the deck, stop once it's at `deck_max`
            // rather than asserting a buy that's correctly rejected.
            if g.deck.cards().len() >= g.config.deck_max {
                break;
            }
            while let Some(card) = g.shop.cards.first().copied() {
                let deck_before = g.deck.cards().len();
                g.money = 1000;
                g.handle_action(Action::BuyPlayingCard(card))
                    .expect("buy shop playing card");
                assert_eq!(g.deck.cards().len(), deck_before + 1);
                bought += 1;
                if g.deck.cards().len() >= g.config.deck_max {
                    break;
                }
            }
            g.money = 1000;
            g.handle_action(Action::Reroll()).expect("reroll");
        }
        assert!(
            bought > 0,
            "seed {seed}: Magic Trick never produced a shop playing card"
        );
    }
}

/// The soak tests, un-ignored. They were `#[ignore]`d and never ran in CI,
/// while being the only exercise of the two APIs over many complete runs.
/// Seeded, so a failure replays.
///
/// The count is set by `BALATRO_SOAK_GAMES` so a normal run stays quick and
/// CI can widen it without editing code.
fn soak_games() -> u64 {
    std::env::var("BALATRO_SOAK_GAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200)
}

#[test]
fn test_games_gen_actions() {
    for seed in 0..soak_games() {
        run_game_gen_actions(seed);
    }
}

#[test]
fn test_games_action_space() {
    for seed in 0..soak_games() {
        run_game_action_space(seed);
    }
}

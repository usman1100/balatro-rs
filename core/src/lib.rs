pub mod action;
pub mod ante;

pub fn seed_from_str(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
pub mod available;
pub mod boss;
pub mod card;
pub mod config;
pub mod consumable;
pub mod deck;
pub mod effect;
pub mod error;
pub mod game;
pub mod generator;
pub mod hand;
pub mod joker;
pub mod pack;
pub mod planet;
pub mod rank;
mod rng;
pub mod score;
pub mod shop;
pub mod space;
pub mod spectral;
pub mod stage;
pub mod tag;
pub mod tarot;
pub mod voucher;

#[cfg(test)]
mod tests {
    use crate::action::Action;
    use crate::consumable::{Consumable, ConsumableKind};
    use crate::stage::{Blind, End};
    use crate::error::GameError;
    use crate::game::Game;
    use crate::planet::Planets;
    use crate::rank::HandRank;
    use crate::stage::Stage;

    use rand::Rng;

    #[test]
    fn test_planetarium_level_up_changes_scoring() {
        let mut g = Game::default();
        let before = g.planetarium.level(HandRank::OnePair);
        g.planetarium.level_up(HandRank::OnePair);
        let after = g.planetarium.level(HandRank::OnePair);
        assert_eq!(after.chips, before.chips + 15);
        assert_eq!(after.mult, before.mult + 1);
        assert_eq!(after.level, before.level + 1);
    }

    #[test]
    fn test_buy_consumable() {
        let mut g = Game::default();
        g.start();
        g.money = 100;
        g.stage = Stage::Shop();
        g.shop.consumables = vec![Consumable::planet(Planets::Mercury)];
        g.handle_action(Action::BuyConsumable(Consumable::planet(Planets::Mercury)))
            .unwrap();
        assert_eq!(g.consumables.len(), 1);
        assert_eq!(g.money, 97);
        assert!(g.shop.consumables.is_empty());
    }

    #[test]
    fn test_use_consumable_planet() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        g.consumables = vec![Consumable::planet(Planets::Mercury)];
        let before = g.planetarium.level(HandRank::OnePair);
        g.handle_action(Action::UseConsumable(Consumable::planet(Planets::Mercury)))
            .unwrap();
        assert!(g.consumables.is_empty());
        let after = g.planetarium.level(HandRank::OnePair);
        assert_eq!(after.chips, before.chips + 15);
        assert_eq!(after.mult, before.mult + 1);
    }

    #[test]
    fn test_use_consumable_in_postblind() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::PostBlind();
        g.consumables = vec![Consumable::planet(Planets::Jupiter)];
        let before = g.planetarium.level(HandRank::Flush);
        g.handle_action(Action::UseConsumable(Consumable::planet(Planets::Jupiter)))
            .unwrap();
        assert!(g.consumables.is_empty());
        let after = g.planetarium.level(HandRank::Flush);
        assert_eq!(after.chips, before.chips + 15);
        assert_eq!(after.mult, before.mult + 2);
    }

    #[test]
    fn test_buy_consumable_slots_full() {
        let mut g = Game::default();
        g.start();
        g.money = 100;
        g.stage = Stage::Shop();
        g.consumables = vec![
            Consumable::planet(Planets::Pluto),
            Consumable::planet(Planets::Mercury),
        ];
        g.shop.consumables = vec![Consumable::planet(Planets::Venus)];
        let res = g.handle_action(Action::BuyConsumable(Consumable::planet(Planets::Venus)));
        assert!(matches!(res, Err(GameError::NoAvailableSlot)));
    }

    #[test]
    fn test_buy_consumable_insufficient_funds() {
        let mut g = Game::default();
        g.start();
        g.money = 0;
        g.stage = Stage::Shop();
        g.shop.consumables = vec![Consumable::planet(Planets::Mercury)];
        let res = g.handle_action(Action::BuyConsumable(Consumable::planet(Planets::Mercury)));
        assert!(matches!(res, Err(GameError::InvalidBalance)));
    }

    #[test]
    fn test_use_consumable_valid_in_blind() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Blind(crate::stage::Blind::Small);
        g.consumables = vec![Consumable::planet(Planets::Mercury)];
        let before = g.planetarium.level(HandRank::OnePair);
        let res = g.handle_action(Action::UseConsumable(Consumable::planet(Planets::Mercury)));
        assert!(res.is_ok());
        assert_eq!(
            g.planetarium.level(HandRank::OnePair).chips,
            before.chips + 15
        );
    }

    #[test]
    fn test_use_consumable_valid_in_preblind() {
        let mut g = Game::default();
        g.start();
        g.consumables = vec![Consumable::planet(Planets::Saturn)];
        let before = g.planetarium.level(HandRank::Straight);
        let res = g.handle_action(Action::UseConsumable(Consumable::planet(Planets::Saturn)));
        assert!(res.is_ok());
        assert_eq!(
            g.planetarium.level(HandRank::Straight).chips,
            before.chips + 30
        );
    }

    #[test]
    fn test_use_consumable_invalid_at_end() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::End(crate::stage::End::Win);
        g.consumables = vec![Consumable::planet(Planets::Mercury)];
        let res = g.handle_action(Action::UseConsumable(Consumable::planet(Planets::Mercury)));
        assert!(matches!(res, Err(GameError::InvalidAction)));
    }

    #[test]
    fn test_gen_actions_buy_consumable_in_shop() {
        let mut g = Game::default();
        g.start();
        g.money = 100;
        g.stage = Stage::Shop();
        g.shop.consumables = vec![Consumable::planet(Planets::Mercury)];
        let actions: Vec<Action> = g.gen_actions().collect();
        assert!(actions.contains(&Action::BuyConsumable(Consumable::planet(Planets::Mercury))));
    }

    #[test]
    fn test_gen_actions_use_consumable_in_shop() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        g.consumables = vec![Consumable::planet(Planets::Earth)];
        let actions: Vec<Action> = g.gen_actions().collect();
        assert!(actions.contains(&Action::UseConsumable(Consumable::planet(Planets::Earth))));
    }

    #[test]
    fn test_gen_actions_use_consumable_in_blind() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Blind(crate::stage::Blind::Small);
        g.consumables = vec![Consumable::planet(Planets::Mercury)];
        let actions: Vec<Action> = g.gen_actions().collect();
        assert!(actions
            .iter()
            .any(|a| matches!(a, Action::UseConsumable(_))));
        // buy is still shop-only
        assert!(!actions
            .iter()
            .any(|a| matches!(a, Action::BuyConsumable(_))));
    }

    #[test]
    // Test executing a full game using the gen_actions api
    fn test_game_gen_actions() {
        let mut g = Game::default();

        g.start();
        while !g.is_over() {
            // Get all available actions
            let actions: Vec<Action> = g.gen_actions().collect();
            if actions.is_empty() {
                break;
            }

            // Pick a random move and execute it
            let i = rand::thread_rng().gen_range(0..actions.len());
            let action = actions[i].clone();
            dbg!("game state:\n{}", g.clone());
            dbg!("play action: {}", action.clone());
            let action_res = g.handle_action(action.clone());
            dbg!(action);
            assert!(action_res.is_ok());
        }
        let result = g.result();
        // Ensure game is over at end
        assert!(result.is_some());
        // Check game state at end
        assert!(matches!(g.stage, Stage::End(_)));
        dbg!("game action history: {:?}", g.action_history);
    }

    #[test]
    fn test_use_consumable_hand_touching_tarot_illegal_from_shop() {
        use crate::tarot::Tarot;
        let mut g = Game::default();
        g.start();
        // at Shop, available is empty (clear_blind returns cards to deck without re-dealing)
        g.stage = Stage::Shop();
        assert_eq!(g.available.cards().len(), 0);
        g.consumables = vec![Consumable::tarot(Tarot::Magician)];
        let res = g.handle_action(Action::UseConsumable(Consumable::tarot(Tarot::Magician)));
        assert!(matches!(res, Err(GameError::InvalidAction)));
        // Consumable is not consumed on a failed use, and no substitute hand is dealt.
        assert_eq!(g.consumables, vec![Consumable::tarot(Tarot::Magician)]);
        assert_eq!(g.stage, Stage::Shop());
        assert_eq!(g.available.cards().len(), 0);
    }

    #[test]
    fn test_use_consumable_hand_touching_spectral_illegal_from_shop() {
        use crate::spectral::Spectral;
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        assert_eq!(g.available.cards().len(), 0);
        g.consumables = vec![Consumable::spectral(Spectral::Talisman)];
        let res = g.handle_action(Action::UseConsumable(Consumable::spectral(
            Spectral::Talisman,
        )));
        assert!(matches!(res, Err(GameError::InvalidAction)));
        assert_eq!(g.consumables, vec![Consumable::spectral(Spectral::Talisman)]);
        assert_eq!(g.stage, Stage::Shop());
        assert_eq!(g.available.cards().len(), 0);
    }

    // Fool copies the last Tarot or Planet used, never a Spectral (matches its
    // own description text and real Balatro). Using a Spectral in between must
    // not overwrite last_consumable_used, and Fool must never duplicate it.
    #[test]
    fn test_fool_does_not_copy_spectral_used_via_consumable() {
        use crate::spectral::Spectral;
        use crate::tarot::Tarot;

        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        g.config.consumable_slots = 3;

        g.consumables = vec![Consumable::planet(Planets::Mercury)];
        g.handle_action(Action::UseConsumable(Consumable::planet(Planets::Mercury)))
            .unwrap();
        assert_eq!(
            g.last_consumable_used,
            Some(Consumable::planet(Planets::Mercury))
        );

        // Wraith is a 0-target Spectral that doesn't touch the hand, so it's
        // legal from Shop.
        g.consumables.push(Consumable::spectral(Spectral::Wraith));
        g.handle_action(Action::UseConsumable(Consumable::spectral(
            Spectral::Wraith,
        )))
        .unwrap();
        // Using the Spectral must not overwrite what Fool remembers.
        assert_eq!(
            g.last_consumable_used,
            Some(Consumable::planet(Planets::Mercury))
        );

        g.consumables.push(Consumable::tarot(Tarot::Fool));
        g.handle_action(Action::UseConsumable(Consumable::tarot(Tarot::Fool)))
            .unwrap();
        assert!(g.consumables.contains(&Consumable::planet(Planets::Mercury)));
        assert!(
            !g.consumables
                .iter()
                .any(|c| matches!(c.kind, ConsumableKind::Spectral(_))),
            "Fool must never duplicate a Spectral"
        );
    }

    // A 0-target Spectral that still destroys a random card from the hand
    // (see requires_hand()) must not be usable from Shop, where the hand is
    // empty — this is the crash this gate exists to prevent.
    #[test]
    fn test_use_consumable_zero_target_hand_touching_spectral_illegal_from_shop() {
        use crate::spectral::Spectral;
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        assert_eq!(g.available.cards().len(), 0);
        g.consumables = vec![Consumable::spectral(Spectral::Familiar)];
        let res = g.handle_action(Action::UseConsumable(Consumable::spectral(
            Spectral::Familiar,
        )));
        assert!(matches!(res, Err(GameError::InvalidAction)));
        assert_eq!(g.consumables, vec![Consumable::spectral(Spectral::Familiar)]);

        let actions: Vec<Action> = g.gen_actions().collect();
        assert!(!actions.contains(&Action::UseConsumable(Consumable::spectral(
            Spectral::Familiar
        ))));
    }

    #[test]
    fn test_gen_actions_use_consumable_spectral_in_shop() {
        use crate::spectral::Spectral;
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        g.consumables = vec![Consumable::spectral(Spectral::Sigil)];
        let actions: Vec<Action> = g.gen_actions().collect();
        // Sigil is 0-target but still touches the hand, so it's not offered from Shop.
        assert!(!actions.contains(&Action::UseConsumable(Consumable::spectral(Spectral::Sigil))));
    }

    /// The two public move APIs duplicated the "can I use this consumable"
    /// rule and the copies drifted: the action-space copy let a hand-touching
    /// Spectral out during a blind-less stage, and `handle_action` then
    /// rejected it. Pin both against the same predicate.
    #[test]
    fn test_both_move_apis_agree_on_usable_consumables() {
        use crate::card::{Card, Suit, Value};
        use crate::consumable::Consumable;
        use crate::spectral::Spectral;
        use crate::tarot::Tarot;

        // One at a time, because the action space has a fixed two-slot
        // consumable bucket and indexing past it is an error, not a skip.
        let all: Vec<Consumable> = [
            Consumable::tarot(Tarot::Death),
            Consumable::tarot(Tarot::Fool),
            Consumable::spectral(Spectral::Familiar),
            Consumable::spectral(Spectral::Sigil),
            Consumable::spectral(Spectral::Ankh),
            Consumable::planet(crate::planet::Planets::Mercury),
        ]
        .into_iter()
        .collect();

        for stage in [
            Stage::PreBlind(),
            Stage::Blind(Blind::Small),
            Stage::PostBlind(),
            Stage::Shop(),
            Stage::PackOpen(),
            Stage::End(End::Lose),
        ] {
            for selected in [0usize, 1, 2] {
                for c in &all {
                    let mut g = Game::default();
                    g.start();
                    g.stage = stage;
                    g.consumables = vec![*c];
                    g.available.extend(
                        (0..5)
                            .map(|i| {
                                Card::new(
                                    Value::Two,
                                    if i % 2 == 0 { Suit::Heart } else { Suit::Club },
                                )
                            })
                            .collect(),
                    );
                    for card in g.available.not_selected().iter().take(selected) {
                        g.select_card(*card).expect("select");
                    }

                    let offered = g.gen_actions().any(|a| a == Action::UseConsumable(*c));
                    let masked = {
                        let space = g.gen_action_space();
                        let vec = space.to_vec();
                        (0..vec.len())
                            .filter(|i| vec[*i] == 1)
                            .filter_map(|i| space.to_action(i, &g).ok())
                            .any(|a| a == Action::UseConsumable(*c))
                    };
                    assert_eq!(
                        offered, masked,
                        "{stage:?} with {selected} selected: {c:?} offered by one API but not the other"
                    );
                    if offered {
                        assert!(
                            g.handle_action(Action::UseConsumable(*c)).is_ok(),
                            "{stage:?} with {selected} selected: {c:?} was offered but rejected"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_tarot_hand_selection_capped_at_max_targets() {
        use crate::pack::{Pack, PackCategory, PackContent, PackSize};
        use crate::tarot::Tarot;

        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        g.money = 100;
        let pack = Pack {
            category: PackCategory::Arcana,
            size: PackSize::Normal,
            contents: vec![PackContent::Tarot(Tarot::Magician)],
        };
        g.shop.packs = vec![pack.clone()];
        g.handle_action(Action::BuyPack(pack)).unwrap();
        assert_eq!(g.stage, Stage::PackOpen());
        g.handle_action(Action::PickPackCard(PackContent::Tarot(Tarot::Magician)))
            .unwrap();
        assert!(matches!(g.stage, Stage::TarotHand(Tarot::Magician)));
        let cards = g.available.cards();
        g.handle_action(Action::SelectCard(cards[0])).unwrap();
        g.handle_action(Action::SelectCard(cards[1])).unwrap();
        assert_eq!(g.available.selected().len(), 2);
        let res = g.handle_action(Action::SelectCard(cards[2]));
        assert!(matches!(res, Err(GameError::InvalidAction)));
        let actions: Vec<Action> = g.gen_actions().collect();
        assert!(!actions.iter().any(|a| matches!(a, Action::SelectCard(_))));
        assert!(actions.contains(&Action::ApplyTarot()));
    }

    #[test]
    fn test_spectral_hand_select_and_apply_round_trip() {
        use crate::card::Seal;
        use crate::pack::{Pack, PackCategory, PackContent, PackSize};
        use crate::spectral::Spectral;

        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        g.money = 100;
        let pack = Pack {
            category: PackCategory::Spectral,
            size: PackSize::Normal,
            contents: vec![PackContent::Spectral(Spectral::Talisman)],
        };
        g.shop.packs = vec![pack.clone()];
        g.handle_action(Action::BuyPack(pack)).unwrap();
        g.handle_action(Action::PickPackCard(PackContent::Spectral(
            Spectral::Talisman,
        )))
        .unwrap();
        assert!(matches!(g.stage, Stage::SpectralHand(Spectral::Talisman)));

        let actions: Vec<Action> = g.gen_actions().collect();
        assert!(!actions.contains(&Action::ApplySpectral()));

        let card = g.available.cards()[0];
        g.handle_action(Action::SelectCard(card)).unwrap();

        let actions: Vec<Action> = g.gen_actions().collect();
        assert!(actions.contains(&Action::ApplySpectral()));

        g.handle_action(Action::ApplySpectral()).unwrap();
        // The Normal pack's single pick is now used, so finish_pack ran and
        // returned to Shop, sending any leftover drawn-hand cards to the deck.
        assert_eq!(g.stage, Stage::Shop());
        assert_eq!(g.available.cards().len(), 0);
        let updated = g
            .deck
            .cards()
            .into_iter()
            .find(|c| c.id == card.id)
            .unwrap();
        assert_eq!(updated.seal, Some(Seal::Gold));
    }

    #[test]
    // Test executing a full game using the gen_action_space (vector) api
    fn test_game_action_space() {
        let mut g = Game::default();

        g.start();
        while !g.is_over() {
            // Get action space and vector
            let space = g.gen_action_space();
            let space_vec = space.to_vec();
            if space.is_empty() {
                break;
            }

            // Pick a random move and ensure its unmasked
            let mut i: usize;
            loop {
                i = rand::thread_rng().gen_range(0..space_vec.len());
                if space_vec[i] == 1 {
                    break;
                }
            }
            let action = space.to_action(i, &g).expect("valid index to action");
            dbg!("game state:\n{}", g.clone());
            dbg!("play action: {}", action.clone());
            let action_res = g.handle_action(action.clone());
            dbg!(action);
            assert!(action_res.is_ok());
        }
        let result = g.result();
        // Ensure game is over at end
        assert!(result.is_some());
        // Check game state at end
        assert!(matches!(g.stage, Stage::End(_)));
        dbg!("game action history: {:?}", g.action_history);
    }
}

use balatro_types::*;
use strum::IntoEnumIterator;

#[test]
fn canonical_roster_counts() {
    assert_eq!(Jokers::iter().count(), 150, "Jokers");
    assert_eq!(Tarot::iter().count(), 22, "Tarot");
    assert_eq!(Planets::iter().count(), 12, "Planets");
    assert_eq!(Spectral::iter().count(), 18, "Spectral");
    assert_eq!(Voucher::iter().count(), 32, "Voucher");
    assert_eq!(Tag::iter().count(), 24, "Tag");
    assert_eq!(DeckVariant::iter().count(), 15, "DeckVariant");
    assert_eq!(Stake::iter().count(), 8, "Stake");
    assert_eq!(BossBlind::iter().count(), 28, "BossBlind");
    assert_eq!(HandRank::iter().count(), 13, "HandRank");
    assert_eq!(Value::iter().count(), 13, "Value");
    assert_eq!(Suit::iter().count(), 4, "Suit");
    assert_eq!(Enhancement::iter().count(), 8, "Enhancement");
    assert_eq!(Seal::iter().count(), 4, "Seal");
    assert_eq!(Edition::iter().count(), 5, "Edition");
    assert_eq!(Blind::iter().count(), 3, "Blind");
}

/// Every joker's price, pinned against the real game.
///
/// `cost` is transcribed from the game's own centre definitions, one of the
/// largest hand-entered tables in the workspace: 150 rows of `cost = N`. A
/// typo in any of them is a silent economy change — a Joker bought for the
/// wrong price, and a sell value derived from it — and nothing else in the
/// tree would notice. `canonical_roster_counts` checks that there are 150
/// of them; this checks what each one says.
///
/// Regenerate by re-reading the `cost = N` out of the centre table, or by
/// hand after a version bump: `grep -o 'cost = [0-9]*'`.
#[test]
fn every_joker_cost_matches_the_game() {
    // (save-file id, price)
    const EXPECTED: &[(&str, usize)] = &[
    ("j_8_ball", 5),
    ("j_abstract", 4),
    ("j_acrobat", 6),
    ("j_ancient", 8),
    ("j_arrowhead", 7),
    ("j_astronomer", 8),
    ("j_banner", 5),
    ("j_baron", 8),
    ("j_baseball", 8),
    ("j_blackboard", 6),
    ("j_bloodstone", 7),
    ("j_blue_joker", 5),
    ("j_blueprint", 10),
    ("j_bootstraps", 7),
    ("j_brainstorm", 10),
    ("j_bull", 6),
    ("j_burglar", 6),
    ("j_burnt", 8),
    ("j_business", 4),
    ("j_caino", 20),
    ("j_campfire", 9),
    ("j_card_sharp", 6),
    ("j_cartomancer", 6),
    ("j_castle", 6),
    ("j_cavendish", 4),
    ("j_ceremonial", 6),
    ("j_certificate", 6),
    ("j_chaos", 4),
    ("j_chicot", 20),
    ("j_clever", 4),
    ("j_cloud_9", 7),
    ("j_constellation", 6),
    ("j_crafty", 4),
    ("j_crazy", 4),
    ("j_credit_card", 1),
    ("j_delayed_grat", 4),
    ("j_devious", 4),
    ("j_diet_cola", 6),
    ("j_dna", 8),
    ("j_drivers_license", 7),
    ("j_droll", 4),
    ("j_drunkard", 4),
    ("j_duo", 8),
    ("j_dusk", 5),
    ("j_egg", 4),
    ("j_erosion", 6),
    ("j_even_steven", 4),
    ("j_faceless", 4),
    ("j_family", 8),
    ("j_fibonacci", 8),
    ("j_flash", 5),
    ("j_flower_pot", 6),
    ("j_fortune_teller", 6),
    ("j_four_fingers", 7),
    ("j_gift", 6),
    ("j_glass", 6),
    ("j_gluttenous_joker", 5),
    ("j_golden", 6),
    ("j_greedy_joker", 5),
    ("j_green_joker", 4),
    ("j_gros_michel", 5),
    ("j_hack", 6),
    ("j_half", 5),
    ("j_hallucination", 4),
    ("j_hanging_chad", 4),
    ("j_hiker", 5),
    ("j_hit_the_road", 8),
    ("j_hologram", 7),
    ("j_ice_cream", 5),
    ("j_idol", 6),
    ("j_invisible", 8),
    ("j_joker", 2),
    ("j_jolly", 3),
    ("j_juggler", 4),
    ("j_loyalty_card", 5),
    ("j_luchador", 5),
    ("j_lucky_cat", 6),
    ("j_lusty_joker", 5),
    ("j_mad", 4),
    ("j_madness", 7),
    ("j_mail", 4),
    ("j_marble", 6),
    ("j_matador", 7),
    ("j_merry_andy", 7),
    ("j_midas_mask", 7),
    ("j_mime", 5),
    ("j_misprint", 4),
    ("j_mr_bones", 5),
    ("j_mystic_summit", 5),
    ("j_obelisk", 8),
    ("j_odd_todd", 4),
    ("j_onyx_agate", 7),
    ("j_oops", 4),
    ("j_order", 8),
    ("j_pareidolia", 5),
    ("j_perkeo", 20),
    ("j_photograph", 5),
    ("j_popcorn", 5),
    ("j_raised_fist", 5),
    ("j_ramen", 6),
    ("j_red_card", 5),
    ("j_reserved_parking", 6),
    ("j_ride_the_bus", 6),
    ("j_riff_raff", 6),
    ("j_ring_master", 5),
    ("j_rocket", 6),
    ("j_rough_gem", 7),
    ("j_runner", 5),
    ("j_satellite", 6),
    ("j_scary_face", 4),
    ("j_scholar", 4),
    ("j_seance", 6),
    ("j_seeing_double", 6),
    ("j_selzer", 6),
    ("j_shoot_the_moon", 5),
    ("j_shortcut", 7),
    ("j_sixth_sense", 6),
    ("j_sly", 3),
    ("j_smeared", 7),
    ("j_smiley", 4),
    ("j_sock_and_buskin", 6),
    ("j_space", 5),
    ("j_splash", 3),
    ("j_square", 4),
    ("j_steel_joker", 7),
    ("j_stencil", 8),
    ("j_stone", 6),
    ("j_stuntman", 7),
    ("j_supernova", 5),
    ("j_superposition", 4),
    ("j_swashbuckler", 4),
    ("j_throwback", 6),
    ("j_ticket", 5),
    ("j_to_the_moon", 5),
    ("j_todo_list", 4),
    ("j_trading", 6),
    ("j_tribe", 8),
    ("j_triboulet", 20),
    ("j_trio", 8),
    ("j_troubadour", 6),
    ("j_trousers", 6),
    ("j_turtle_bean", 6),
    ("j_vagabond", 8),
    ("j_vampire", 7),
    ("j_walkie_talkie", 4),
    ("j_wee", 8),
    ("j_wily", 4),
    ("j_wrathful_joker", 5),
    ("j_yorick", 20),
    ("j_zany", 4),
    ];

    assert_eq!(
        EXPECTED.len(),
        crate::joker::Jokers::iter().count(),
        "the table covers every joker exactly once"
    );

    for (id, price) in EXPECTED {
        let joker = crate::joker::Jokers::from_id(id)
            .unwrap_or_else(|| panic!("{id} does not resolve to a joker"));
        assert_eq!(
            joker.cost(),
            *price,
            "{} ({id}) costs {price}, not {}",
            joker.name(),
            joker.cost()
        );
    }
}

/// Costs must be unique enough to be distinguishable, and the sell value is
/// derived from the cost, so a zero or one would make a Joker free or
/// worthless.
#[test]
fn joker_prices_are_in_range() {
    for j in crate::joker::Jokers::iter() {
        assert!(
            (1..=20).contains(&j.cost()),
            "{} has an out-of-range price of {}",
            j.name(),
            j.cost()
        );
        assert!(
            j.sell_value() >= 1,
            "{} would sell for nothing",
            j.name()
        );
    }
}

/// Every hand's level-1 base and its per-level step, pinned against the
/// real game.
///
/// These two tables are what every score in the engine is built from: the
/// hand level, the joker multipliers, the ante requirements. A single wrong
/// entry makes every hand of that rank score wrong, and the existing tests
/// only pinned four of the twelve — Two Pair, Straight, Flush, Full House,
/// Four of a Kind, Five of a Kind, Flush House and Flush Five were checked
/// by nothing at all.
///
/// The base values are the wiki's own "Base Scoring" column, which agrees
/// with the game for all thirteen hands. The per-level steps are the
/// `l_chips`/`l_mult` of the game's hand table.
///
/// Royal Flush has no entry of its own: it scores as a Straight Flush, which
/// is why `scoring_rank()` folds it in and why the table has twelve rows.
#[test]
fn hand_levels_match_the_game() {
    use crate::rank::HandRank;

    // (rank, level-1 chips, level-1 mult, chips per level, mult per level)
    const EXPECTED: &[(HandRank, usize, usize, usize, usize)] = &[
        (HandRank::HighCard, 5, 1, 10, 1),
        (HandRank::OnePair, 10, 2, 15, 1),
        (HandRank::TwoPair, 20, 2, 20, 1),
        (HandRank::ThreeOfAKind, 30, 3, 20, 2),
        (HandRank::Straight, 30, 4, 30, 3),
        (HandRank::Flush, 35, 4, 15, 2),
        (HandRank::FullHouse, 40, 4, 25, 2),
        (HandRank::FourOfAKind, 60, 7, 30, 3),
        (HandRank::StraightFlush, 100, 8, 40, 4),
        (HandRank::FiveOfAKind, 120, 12, 35, 3),
        (HandRank::FlushHouse, 140, 14, 40, 4),
        (HandRank::FlushFive, 160, 16, 50, 3),
    ];

    let p = crate::planet::Planetarium::new();
    for (rank, chips, mult, step_chips, step_mult) in EXPECTED {
        let level = p.level(*rank);
        assert_eq!(level.level, 1, "{rank:?} starts at level 1");
        assert_eq!(
            (level.chips, level.mult),
            (*chips, *mult),
            "{rank:?} level 1 is ({chips}, {mult})"
        );

        // The step is what a Planet card for that rank adds, so level it and
        // diff rather than reading the private table back.
        let mut leveled = p.clone();
        leveled.level_up(*rank);
        let after = leveled.level(*rank);
        assert_eq!(
            (after.chips, after.mult),
            (chips + step_chips, mult + step_mult),
            "one level of {rank:?} adds ({step_chips}, {step_mult})"
        );
    }

    // Royal Flush scores as a Straight Flush rather than having its own row.
    assert_eq!(
        HandRank::RoyalFlush.scoring_rank(),
        HandRank::StraightFlush,
        "Royal Flush folds into Straight Flush"
    );
    assert_eq!(
        p.level(HandRank::RoyalFlush),
        p.level(HandRank::StraightFlush),
        "and so shares its level"
    );
}

/// Consumable prices are uniform per type in the real game, so they are
/// three numbers rather than fifty-two.
#[test]
fn consumable_prices_match_the_game() {
    use crate::consumable::{Consumable, ConsumableKind};
    use crate::planet::Planets;
    use crate::spectral::Spectral;
    use crate::tarot::Tarot;
    use strum::IntoEnumIterator;

    for t in Tarot::iter() {
        assert_eq!(t.cost(), 3, "{t:?} costs 3");
        assert_eq!(t.sell_value(), 1, "{t:?} sells for 1");
    }
    for p in Planets::iter() {
        assert_eq!(p.cost(), 3, "{p:?} costs 3");
        assert_eq!(p.sell_value(), 1, "{p:?} sells for 1");
    }
    for s in Spectral::iter() {
        assert_eq!(s.cost(), 4, "{s:?} costs 4");
        assert_eq!(s.sell_value(), 1, "{s:?} sells for 1");
    }

    // A Negative copy costs five more, which the earlier Perkeo work added.
    let base = Consumable::tarot(Tarot::Fool);
    let negative = Consumable::negative(ConsumableKind::Tarot(Tarot::Fool));
    assert_eq!(negative.cost(), base.cost() + 5);
    assert!(negative.sell_value() > base.sell_value());
}

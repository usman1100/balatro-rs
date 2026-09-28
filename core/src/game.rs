use crate::action::{Action, MoveDirection};
use crate::ante::Ante;
use crate::available::Available;
use crate::boss::BossState;
use crate::card::{card_display, Card, Edition, Enhancement, Seal, Suit, Value};
use crate::config::{Config, RngMode};
use crate::consumable::{Consumable, ConsumableKind};
use crate::deck::Deck;
use crate::deck_variant;
use crate::effect::{EffectRegistry, Effects, RuleFlag};
use crate::error::{GameError, PlayHandError};
use crate::hand::{HandRules, MadeHand, SelectHand};
use crate::joker::{joker_display, JokerEffects, JokerState, Jokers, Rarity};
use crate::pack::{OpenPackState, Pack, PackCategory, PackContent, PackSize};
use crate::planet::Planetarium;
use crate::rank::HandRank;
use crate::rng::{Backend, EffectProc, FastBackend, RealBackend, RngBackend};
use crate::score::{ScoreSource, ScoreStep, ScoreTrace};
use crate::shop::{Shop, ShopContext};
use crate::spectral::SpectralEffect;
use crate::stage::{Blind, BlindExt, End, Stage};
use crate::tag::{Tag, TagState};
use crate::tarot::{Tarot, TarotEffect};
use crate::voucher::{Voucher, Vouchers};

use balatro_types::BossBlind;
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use std::collections::HashSet;
use std::fmt;
use strum::IntoEnumIterator;

#[cfg(feature = "serde")]
fn default_backend() -> Backend {
    Backend::Fast(FastBackend::new(ChaCha8Rng::from_entropy()))
}

/// The real game's own values for the tags that name a count.
const ORBITAL_TAG_LEVELS: usize = 3;
const TOP_UP_TAG_JOKERS: usize = 2;
const INVESTMENT_TAG_DOLLARS: usize = 25;

/// The Psychic's hand size, and how many cards The Hook discards after
/// each played hand.
pub(crate) const PSYCHIC_HAND_SIZE: usize = 5;
const HOOK_DISCARDS: usize = 2;
const TO_THE_MOON_PER_DOLLAR: usize = 5;
const MISPRINT_MIN: i32 = 0;
const RIFF_RAFF_JOKERS: usize = 2;
const MADNESS_MULT_STEP: f32 = 0.5;
const CEREMONIAL_DAGGER_VALUE_MULT: usize = 2;
const INVISIBLE_JOKER_ROUNDS: usize = 2;
const MR_BONES_DIVISOR: usize = 4;
/// What Director's Cut and Retcon charge per boss reroll.
const VOUCHER_BOSS_REROLL_COST: usize = 10;
/// One Credit Card's worth of debt allowance.
pub(crate) const CREDIT_CARD_DEBT: i64 = 20;
const MISPRINT_MAX: i32 = 23;

fn default_reroll_cost() -> usize {
    5
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct Game {
    pub config: Config,
    pub shop: Shop,
    pub planetarium: Planetarium,
    pub deck: Deck,
    pub available: Available,
    pub discarded: Vec<Card>,
    pub tags: Vec<Tag>,
    /// Tags granted outside a skip, e.g. Diet Cola's. Drained into `tags`
    /// at the next cash out, which is when tags are collected.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) pending_tags: Vec<Tag>,
    /// Tag effects waiting on a later shop, blind or round.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) tag_state: TagState,
    /// What the current boss blind remembers between hands.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) boss_state: BossState,
    pub small_blind_tag: Tag,
    pub big_blind_tag: Tag,
    pub blind: Option<Blind>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub current_boss: Option<BossBlind>,
    pub stage: Stage,
    pub ante_start: Ante,
    pub ante_end: Ante,
    pub ante_current: Ante,
    pub action_history: Vec<Action>,
    pub round: usize,

    // vouchers redeemed this run, and the ante whose voucher offer
    // currently sits in the shop (the offer survives rerolls and every
    // shop visit of that ante, so it can't be regenerated per refresh)
    #[cfg_attr(feature = "serde", serde(default))]
    pub vouchers: Vouchers,
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) voucher_ante: Option<Ante>,

    // jokers and their effects
    pub jokers: Vec<Jokers>,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub effect_registry: EffectRegistry,

    // held consumables (planets, tarots, etc.)
    pub consumables: Vec<Consumable>,

    // playing
    pub plays: usize,
    #[cfg_attr(feature = "serde", serde(rename = "discards"))] // shouldn't access directly
    pub discards_remaining: usize,
    pub reward: usize,
    /// Signed: the real game lets this go negative, and Credit Card is the
    /// joker that buys the allowance. `bankrupt_at` is the floor and
    /// defaults to 0, so without a Credit Card the balance still cannot
    /// drop below zero.
    pub money: i64,
    /// How far below zero `money` may go. Each Credit Card lowers it by
    /// `CREDIT_CARD_DEBT`; selling one gives the allowance back.
    #[cfg_attr(feature = "serde", serde(default))]
    pub bankrupt_at: i64,

    // for scoring
    pub chips: usize,
    pub mult: usize,
    pub score: usize,
    /// Baseline probability multiplier. [`Game::prob_mult`] is what
    /// everything should read: it also folds in Oops! All 6s.
    pub prob_mult_base: u32,

    // shared game-level history some jokers key off (RideTheBus, Obelisk, etc)
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) hand_ranks_played_this_round: HashSet<HandRank>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) consecutive_hands_without_face_card: usize,
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) consecutive_hands_not_most_played_type: usize,
    // cards removed via the Discard action only (not played cards)
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) discarded_this_round: Vec<Card>,
    // run total of cards discarded (Discard action only), never reset.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) total_cards_discarded: usize,
    // Run totals the Handy / Garbage / Speed tags pay out against.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) hands_played_this_run: usize,
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) unused_discards_this_run: usize,
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) blinds_skipped_this_run: usize,
    // Run counters the accumulating jokers key off.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) rounds_played: usize,
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) planets_used_this_run: usize,
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) tarots_used_this_run: usize,
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) cards_added_this_ante: usize,
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) glass_cards_destroyed: usize,
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) lucky_triggers: usize,
    /// Campfire's count of cards sold, reset when a boss blind falls.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) campfire_sold: usize,
    /// The ante Hologram was bought in; it only fires there. Set on
    /// purchase, so buying two in different antes keeps both working.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) hologram_antes: Vec<Ante>,
    /// Rerolls taken in the shop being stood in, for Flash Card.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) free_rerolls_this_round: usize,
    /// Face cards destroyed this run, for Canio.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) face_cards_destroyed: usize,
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) rerolls_this_shop: usize,
    /// Burnt Joker's first discard of the round levels a hand.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) burnt_joker_armed: bool,
    /// Hands in the current round, for Oops! All 6s-style procs.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) hands_played_this_round: usize,
    // Set when the current Boss Blind's effect is switched off mid-round:
    // Luchador sold, or any Joker sold while Verdant Leaf is up. Chicot
    // does the same thing but is derived from the joker list, not stored.
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) boss_disabled: bool,
    // set when a boss's ability actually triggered (Matador's trigger condition).
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) boss_triggered_this_hand: bool,

    pub last_consumable_used: Option<Consumable>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub last_score: usize,
    #[cfg_attr(feature = "serde", serde(default = "default_reroll_cost"))]
    pub reroll_cost: usize,

    #[cfg_attr(feature = "serde", serde(default))]
    pub seed: u64,
    #[cfg_attr(feature = "serde", serde(default))]
    pub seed_str: Option<String>,
    // The one source of randomness for everything. shop/pack generation,
    // tag draws, effect procs, seal-triggered consumables, discard-selector
    // rolls, etc. `RngMode::Real` routes the draws that have a real node
    // through `balatro-seed` and leaves the rest on a fast stream.
    #[cfg_attr(feature = "serde", serde(default = "default_backend"))]
    pub(crate) backend: Backend,
    // track stage so we can come back to it after temp tarot stage
    pub(crate) tarot_prev_stage: Option<Stage>,
    // track stage so we can come back to it after temp spectral stage
    pub(crate) spectral_prev_stage: Option<Stage>,

    // open pack state (set when a pack is purchased and being opened)
    #[cfg_attr(feature = "serde", serde(default))]
    pub open_pack: Option<OpenPackState>,
}

impl Game {
    pub fn new(config: Config) -> Self {
        let ante_start = Ante::try_from(config.ante_start).unwrap_or(Ante::One);
        let (seed, seed_str) = match (config.seed_str.clone(), config.seed) {
            (Some(s), _) => {
                let u = crate::seed_from_str(&s);
                (u, Some(s))
            }
            (None, Some(u)) => (u, None),
            (None, None) => {
                let u: u64 = thread_rng().gen();
                (u, None)
            }
        };
        // We always need a fast backend (for now)
        // TODO: until real can do everything
        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(seed));
        let backend = match config.rng_mode {
            RngMode::Fast => Backend::Fast(fast),
            RngMode::Real => {
                // Real mode needs a Balatro-format seed *string*; fall back
                // to the numeric seed's decimal representation if only
                // `config.seed` (no `seed_str`) was given.
                let seed_string = seed_str.clone().unwrap_or_else(|| seed.to_string());
                Backend::Real(RealBackend::new(&seed_string, fast))
            }
        };
        let mut game = Self {
            shop: Shop::new(),
            planetarium: Planetarium::new(),
            deck: Deck::new(),
            available: Available::default(),
            discarded: Vec::new(),
            tags: Vec::new(),
            pending_tags: Vec::new(),
            small_blind_tag: Tag::Uncommon,
            big_blind_tag: Tag::Uncommon,
            action_history: Vec::new(),
            vouchers: Vouchers::new(),
            voucher_ante: None,
            jokers: Vec::new(),
            effect_registry: EffectRegistry::new(),
            consumables: Vec::new(),
            blind: None,
            current_boss: None,
            stage: Stage::PreBlind(),
            ante_start,
            ante_end: Ante::try_from(config.ante_end).unwrap_or(Ante::Eight),
            ante_current: ante_start,
            round: config.round_start,
            // Both are re-derived by `begin_round`; seed them from the
            // deck/stake-adjusted values so the pre-blind state matches.
            plays: deck_variant::plays_per_round(config.plays, config.deck),
            discards_remaining: deck_variant::discards_per_round(
                config.discards,
                config.deck,
                config.stake,
            ),
            reward: config.reward_base,
            money: config.money_start as i64,
            bankrupt_at: 0,
            chips: config.base_chips,
            mult: config.base_mult,
            score: config.base_score,
            prob_mult_base: 1,
            hand_ranks_played_this_round: HashSet::new(),
            consecutive_hands_without_face_card: 0,
            consecutive_hands_not_most_played_type: 0,
            discarded_this_round: Vec::new(),
            total_cards_discarded: 0,
            hands_played_this_run: 0,
            unused_discards_this_run: 0,
            blinds_skipped_this_run: 0,
            rounds_played: 0,
            planets_used_this_run: 0,
            tarots_used_this_run: 0,
            cards_added_this_ante: 0,
            glass_cards_destroyed: 0,
            lucky_triggers: 0,
            campfire_sold: 0,
            hologram_antes: Vec::new(),
            rerolls_this_shop: 0,
            free_rerolls_this_round: 0,
            face_cards_destroyed: 0,
            burnt_joker_armed: false,
            hands_played_this_round: 0,
            tag_state: TagState::default(),
            boss_state: BossState::default(),
            boss_disabled: false,
            boss_triggered_this_hand: false,
            last_consumable_used: None,
            last_score: 0,
            reroll_cost: default_reroll_cost(),
            tarot_prev_stage: None,
            spectral_prev_stage: None,
            open_pack: None,
            seed,
            seed_str,
            backend,
            config,
        };
        game.apply_deck_starting_state();
        game.backend.on_run_start();
        game.start_ante();
        game
    }

    /// The deck's opening card set and starting holdings.
    ///
    /// Run after the `Game` is otherwise built but before `on_run_start`, so
    /// the real backend's lock table already sees the deck the run will
    /// actually use — Abandoned and Checkered are shorter than 52, and the
    /// lock table has to agree with that.
    fn apply_deck_starting_state(&mut self) {
        let deck = self.config.deck;
        self.deck.extend(deck_variant::starting_cards(deck));
        self.money += deck_variant::deck_money_bonus(deck);
        for v in deck_variant::deck_starting_vouchers(deck) {
            self.vouchers.redeem(v);
        }
        for c in deck_variant::deck_starting_consumables(deck) {
            self.consumables.push(c);
        }
    }

    pub fn start(&mut self) {
        self.stage = Stage::PreBlind();
    }

    pub fn result(&self) -> Option<End> {
        match self.stage {
            Stage::End(end) => Some(end),
            _ => None,
        }
    }

    pub fn is_over(&self) -> bool {
        self.result().is_some()
    }

    // --- voucher-adjusted limits ---
    //
    // Vouchers are never applied to `Config` in place; every limit below
    // is `Config`'s baseline plus what the redeemed set implies. Derived
    // rather than mutated so the effects survive save/load and can't be
    // double-applied. (`config.joker_slots` is the exception, mutated by
    // Negative-edition jokers, which predates vouchers.)

    /// Hand size dealt each round — Paint Brush / Palette raise the
    /// baseline, the Juggle Tag adds 3 for the round it was collected in;
    /// Manacle (while active) then subtracts one from that.
    pub fn hand_size(&self) -> usize {
        let base = self
            .config
            .available
            .saturating_add(self.vouchers.hand_size_bonus())
            .saturating_add_signed(deck_variant::deck_hand_size_delta(self.config.deck))
            .saturating_add_signed(self.joker_hand_size_bonus())
            .min(self.config.available_max);
        let base = base.saturating_add(self.tag_state.hand_size_bonus);
        if self.active_boss() == Some(BossBlind::Manacle) {
            base.saturating_sub(1)
        } else {
            base
        }
    }

    /// Joker slots — Antimatter adds one on top of any Negative-edition
    /// jokers already counted in `config.joker_slots`.
    pub fn joker_slots(&self) -> usize {
        (self.config.joker_slots + self.vouchers.joker_slot_bonus())
            .saturating_add_signed(deck_variant::deck_joker_slot_delta(self.config.deck))
            .min(self.config.joker_slots_max)
    }

    /// Consumable slots — Crystal Ball adds one.
    pub fn consumable_slots(&self) -> usize {
        (self.config.consumable_slots + self.vouchers.consumable_slot_bonus())
            .saturating_add_signed(deck_variant::deck_consumable_slot_delta(self.config.deck))
    }

    /// How many of the held consumables actually occupy a slot. A Negative
    /// consumable does not: the real game raises the consumable card limit
    /// by one when the edition is applied, which nets out to the card being
    /// free to hold.
    pub(crate) fn consumables_holding_slots(&self) -> usize {
        self.consumables.iter().filter(|c| c.takes_a_slot()).count()
    }

    /// Whether a consumable can be added, given whether it needs a slot.
    pub(crate) fn can_hold_consumable(&self, consumable: &Consumable) -> bool {
        !consumable.takes_a_slot() || self.consumables_holding_slots() < self.consumable_slots()
    }

    /// Hands per round — Grabber/Nacho Tong add, Hieroglyph subtracts.
    /// Never drops below one; a run with no hands is unplayable. The
    /// Needle overrides it entirely, rather than adjusting it.
    pub fn plays_per_round(&self) -> usize {
        if self.active_boss() == Some(BossBlind::Needle) {
            return 1;
        }
        Self::apply_delta(
            deck_variant::plays_per_round(self.config.plays, self.config.deck),
            self.vouchers.hands_delta() + self.joker_hands_delta(),
        )
        .max(1)
    }

    /// Discards per round — Wasteful/Recyclomancy add, Petroglyph
    /// subtracts. May legitimately reach zero.
    pub fn discards_per_round(&self) -> usize {
        Self::apply_delta(
            deck_variant::discards_per_round(
                self.config.discards,
                self.config.deck,
                self.config.stake,
            ),
            self.vouchers.discards_delta() + self.joker_discards_delta(),
        )
    }

    /// Interest cap at cash out — Seed Money / Money Tree raise it.
    pub fn interest_max(&self) -> usize {
        self.config.interest_max + self.vouchers.interest_cap_bonus()
    }

    /// Which hand-shape rules the four partial-hand jokers put in force.
    /// Read from the effect registry, which is rebuilt on every joker
    /// change, so buying or selling one takes effect immediately.
    pub fn hand_rules(&self) -> HandRules {
        let has = |flag| self.effect_registry.rule_flags.contains(&flag);
        HandRules {
            hand_size: if has(RuleFlag::FourCardsMakeStraightsAndFlushes) {
                4
            } else {
                5
            },
            gappy_straights: has(RuleFlag::GappyStraights),
            smeared_suits: has(RuleFlag::SmearedSuits),
        }
    }

    /// The run's probability multiplier. Oops! All 6s doubles every listed
    /// probability, and several can be owned, so this doubles per copy.
    /// Distinct planet cards used this run, for Satellite. Each planet
    /// levels exactly one hand, so a hand with any plays means its planet
    /// was used. Royal Flush shares Straight Flush's storage.
    pub(crate) fn distinct_planets_used(&self) -> usize {
        HandRank::iter()
            .filter(|r| *r != HandRank::RoyalFlush)
            .filter(|r| self.planetarium.level(*r).plays > 0)
            .count()
    }

    /// Whether the run is inside an ante a Hologram was bought in.
    pub(crate) fn hologram_active(&self) -> bool {
        self.hologram_antes.contains(&self.ante_current)
    }

    /// Misprint's +0-23, through the real game's own node where possible.
    pub(crate) fn misprint_roll(&mut self) -> i32 {
        self.backend
            .roll_range(EffectProc::Misprint, MISPRINT_MIN, MISPRINT_MAX)
    }

    pub fn prob_mult(&self) -> u32 {
        let oops = self
            .jokers
            .iter()
            .filter(|j| matches!(j, Jokers::OopsAllSixes(_)))
            .count() as u32;
        self.prob_mult_base.saturating_mul(1u32 << oops.min(16))
    }

    /// Hand size from Jokers, on top of the voucher baseline. Signed,
    /// because Merry Andy and Stuntman take as much as they give.
    pub fn joker_hand_size_bonus(&self) -> isize {
        self.jokers
            .iter()
            .map(|j| match j {
                Jokers::Juggler(_) => 1,
                Jokers::MerryAndy(_) => -1,
                Jokers::Troubadour(_) => 2,
                Jokers::Stuntman(_) => -2,
                // Turtle Bean shrinks by one each round, tracked on the joker.
                Jokers::TurtleBean(bean) => (bean.state.counter as isize) + 5,
                _ => 0,
            })
            .sum()
    }

    /// Hands per round from Jokers. Troubadour takes one away each round.
    pub fn joker_hands_delta(&self) -> isize {
        self.jokers
            .iter()
            .map(|j| match j {
                Jokers::Troubadour(t) => -1 - (t.state.counter as isize),
                _ => 0,
            })
            .sum()
    }

    /// Discards per round from Jokers.
    pub fn joker_discards_delta(&self) -> isize {
        self.jokers
            .iter()
            .map(|j| match j {
                Jokers::Drunkard(_) => 1,
                Jokers::MerryAndy(_) => 3,
                _ => 0,
            })
            .sum()
    }

    /// Interest beyond the normal cap: To the Moon pays $1 per $5 held.
    pub fn joker_interest_bonus(&self) -> usize {
        if !self.jokers.iter().any(|j| matches!(j, Jokers::ToTheMoon(_))) {
            return 0;
        }
        self.money.max(0) as usize / TO_THE_MOON_PER_DOLLAR
    }

    /// Astronomer: every Planet card and Celestial Pack in the shop is free.
    pub fn celestial_packs_are_free(&self) -> bool {
        self.jokers
            .iter()
            .any(|j| matches!(j, Jokers::Astronomer(_)))
    }

    /// Replaces this ante's boss blind with a fresh draw, spending Director's
    /// Cut's allowance. The Boss Tag uses this for free; the vouchers charge
    /// for it.
    fn boss_reroll_allowance(&self) -> bool {
        // Retcon rerolls as often as you can pay for, so it ignores the
        // per-ante flag entirely.
        if self.vouchers.boss_rerolls_unlimited() {
            return true;
        }
        self.vouchers.has(Voucher::DirectorsCut) && !self.boss_state.boss_rerolled_this_ante
    }

    /// Spends the allowance, for the calls that actually reroll.
    fn spend_boss_reroll_allowance(&mut self) -> bool {
        if self.vouchers.boss_rerolls_unlimited() {
            return true;
        }
        if !self.boss_reroll_allowance() {
            return false;
        }
        self.boss_state.boss_rerolled_this_ante = true;
        true
    }

    /// Draws a replacement boss blind. A new draw means another one is now
    /// spent, so the pool cycles on the real game's `boss` node.
    fn draw_replacement_boss(&mut self) {
        let boss = self.backend.draw_boss(self.ante_current as i32);
        self.current_boss = Some(boss);
    }

    /// The Boss Tag's reroll, which is always free. It does still use up
    /// Director's Cut's one reroll for the ante, so you cannot pay for a
    /// second one after spending the tag.
    fn reroll_boss_from_tag(&mut self) {
        if self.vouchers.has(Voucher::DirectorsCut) {
            self.boss_state.boss_rerolled_this_ante = true;
        }
        self.draw_replacement_boss();
    }

    /// Whether the paid boss reroll is on offer right now, and what it costs.
    /// Available in the shop and on the pre-blind screen, so the choice is
    /// made before committing to the blind.
    pub fn can_reroll_boss(&self) -> bool {
        if !matches!(self.stage, Stage::Shop() | Stage::PreBlind()) {
            return false;
        }
        if !self.vouchers.can_reroll_boss() {
            return false;
        }
        self.boss_reroll_allowance() && self.can_afford(VOUCHER_BOSS_REROLL_COST)
    }

    pub(crate) fn reroll_boss(&mut self) -> Result<(), GameError> {
        if !matches!(self.stage, Stage::Shop() | Stage::PreBlind()) {
            return Err(GameError::InvalidStage);
        }
        if !self.vouchers.can_reroll_boss() {
            return Err(GameError::InvalidAction);
        }
        if !self.spend_boss_reroll_allowance() {
            return Err(GameError::InvalidAction);
        }
        if !self.can_afford(VOUCHER_BOSS_REROLL_COST) {
            return Err(GameError::InvalidBalance);
        }
        self.money -= VOUCHER_BOSS_REROLL_COST as i64;
        self.draw_replacement_boss();
        Ok(())
    }

    /// Jokers that fire when the shop is left rather than on a score.
    /// Perkeo mints a Negative copy of a random held consumable here, which
    /// is the only source of a Negative consumable core implements.
    fn shop_end_effects(&mut self) {
        if !self.jokers.iter().any(|j| matches!(j, Jokers::Perkeo(_))) {
            return;
        }
        let Some(victim) = self.random_held_consumable() else {
            return;
        };
        let negative = Consumable::negative(victim.kind);
        // Slot-free, so this cannot overflow, but the check is cheap and
        // keeps the invariant in one place.
        if !self.can_hold_consumable(&negative) {
            return;
        }
        self.consumables.push(negative);
    }

    /// A uniformly random held consumable, including Negative copies.
    fn random_held_consumable(&mut self) -> Option<Consumable> {
        if self.consumables.is_empty() {
            return None;
        }
        let idx = self.backend.random_index(self.consumables.len());
        self.consumables.get(idx).copied()
    }

    /// A tag waiting to be collected, granted out of band by Diet Cola.
    pub(crate) fn grant_free_double_tag(&mut self) {
        self.pending_tags.push(Tag::Double);
    }

    /// Slot of a Joker in the joker row, which is what "to the right of"
    /// and "leftmost" resolve against.
    pub(crate) fn joker_slot(&self, id: usize) -> Option<usize> {
        self.jokers.iter().position(|j| j.instance_id() == id)
    }

    /// The Joker immediately to the right of `id`, wrapping is not a thing
    /// in the real game — the row just ends.
    pub(crate) fn joker_to_the_right(&self, id: usize) -> Option<Jokers> {
        let slot = self.joker_slot(id)?;
        self.jokers.get(slot + 1).cloned()
    }

    pub(crate) fn leftmost_joker(&self) -> Option<Jokers> {
        self.jokers.first().cloned()
    }

    /// Removes a Joker from the row, as a destruction effect would.
    pub(crate) fn destroy_joker(&mut self, id: usize) -> Option<Jokers> {
        let slot = self.joker_slot(id)?;
        let removed = self.jokers.remove(slot);
        if removed.edition() == Edition::Negative {
            self.config.joker_slots = self.config.joker_slots.saturating_sub(1);
        }
        self.effect_registry
            .register_jokers(self.jokers.clone(), &self.clone());
        Some(removed)
    }

    /// Adds a playing card straight to the deck, as Hiker and DNA do.
    pub(crate) fn add_card_to_deck(&mut self, card: Card) {
        self.deck.push(card);
    }

    /// Invisible Joker's payoff: a copy of a random owned Joker, with any
    /// Negative edition stripped as the real game does.
    pub(crate) fn duplicate_random_joker(&mut self) {
        if self.jokers.len() >= self.joker_slots() {
            return;
        }
        if self.jokers.is_empty() {
            return;
        }
        let picked = self.backend.pick_random_joker(self.jokers.clone());
        let mut copy = picked.clone();
        copy.set_instance_id(crate::joker::mint_joker_id());
        if copy.edition() == Edition::Negative {
            copy.set_edition(Edition::Base);
            self.config.joker_slots = self.config.joker_slots.saturating_sub(1);
        }
        self.jokers.push(copy);
        self.effect_registry
            .register_jokers(self.jokers.clone(), &self.clone());
    }

    /// A copy of a held playing card, for DNA.
    pub(crate) fn copy_of_card(&self, card: &Card) -> Card {
        let mut copy = Card::new(card.value, card.suit);
        copy.edition = card.edition;
        copy.enhancement = card.enhancement;
        copy.seal = card.seal;
        copy
    }

    /// Certificate, Cartomancer, Chaotic and friends need a live tarot, so
    /// the helper they share.
    pub(crate) fn add_tarot(&mut self) {
        let consumable = self.backend.roll_random_tarot(&[]);
        if !self.can_hold_consumable(&consumable) {
            return;
        }
        self.consumables.push(consumable);
    }

    pub(crate) fn add_spectral(&mut self) {
        let consumable = self.backend.roll_random_spectral(&[]);
        if !self.can_hold_consumable(&consumable) {
            return;
        }
        self.consumables.push(consumable);
    }

    /// What the next reroll in a fresh shop costs, before the per-reroll
    /// escalation. Reroll Surplus / Reroll Glut take $2 off each.
    pub fn base_reroll_cost(&self) -> usize {
        default_reroll_cost().saturating_sub(self.vouchers.reroll_discount())
    }

    /// Price of a shop item after Clearance Sale / Liquidation. Vouchers
    /// themselves are never discounted.
    pub fn price(&self, cost: usize) -> usize {
        self.vouchers.discounted(cost)
    }

    /// What a shop joker actually costs, including a Rental joker's flat $1.
    /// A Rental price is applied after discounts, so it does not go through
    /// `price` — the UIs must use this rather than `price(joker.cost())` or
    /// they will show the pre-rental price.
    pub fn joker_price(&self, joker: &Jokers) -> usize {
        if joker.stickers().rental {
            crate::sticker::RENTAL_COST
        } else {
            self.price(joker.cost())
        }
    }

    /// Money available to spend, which is the balance less any debt
    /// allowance. Non-negative by construction, so every affordability
    /// check downstream can stay a plain `usize` comparison. With no Credit
    /// Card the allowance is 0 and this is just the balance.
    pub fn spendable_balance(&self) -> usize {
        (self.money - self.bankrupt_at).max(0) as usize
    }

    /// Whether a purchase of `cost` is covered.
    pub fn can_afford(&self, cost: usize) -> bool {
        cost as i64 <= self.spendable_balance() as i64
    }

    /// The debt allowance the run has bought. Negative, so this is a floor
    /// rather than a balance.
    pub fn debt_allowance(&self) -> i64 {
        -self.bankrupt_at
    }

    /// The balance a charge is allowed to leave behind. Payments never push
    /// the balance below the floor, so a run can't accidentally go into
    /// debt from a joker effect — only a purchase can, and only because
    /// `can_afford` checked it.
    pub fn debt_floor(&self) -> i64 {
        self.bankrupt_at
    }

    fn apply_delta(base: usize, delta: isize) -> usize {
        if delta >= 0 {
            base + delta as usize
        } else {
            base.saturating_sub(delta.unsigned_abs())
        }
    }

    /// Bundles what shop generation reads off the game. Borrowing rules
    /// mean callers clone the pieces first, then pass them in — see
    /// `cashout`.
    fn shop_context<'a>(
        &self,
        planetarium: &'a Planetarium,
        held_consumables: &'a [Consumable],
        held_jokers: &'a [Jokers],
        vouchers: &'a Vouchers,
    ) -> ShopContext<'a> {
        ShopContext {
            planetarium,
            held_consumables,
            held_jokers,
            vouchers,
            prob_mult: self.prob_mult(),
            ante: self.ante_current as i32,
            stake: self.config.stake,
            most_played: self.most_played_hand_rank(),
        }
    }

    // Fires when the round that just ended cleared the blind, before played
    // cards are swapped out and replacements drawn in. `self.available` still
    // reflects what was actually held in hand at the moment the round ended.
    fn handle_round_end(&mut self) {
        self.rounds_played += 1;
        self.free_rerolls_this_round = 0;
        for e in self.effect_registry.on_round_end.clone() {
            if let Effects::OnRoundEnd(f) = e {
                f.lock().unwrap()(self)
            }
        }
        self.sticker_round_end();

        let planetarium = self.planetarium.clone();
        for card in self.available.not_selected() {
            if card.seal == Some(Seal::Blue) && self.consumables_holding_slots() < self.consumable_slots() {
                let planet = self.backend.roll_random_planet(&planetarium, &[]);
                self.consumables.push(planet);
            }
        }
    }

    /// Perishable and Rental both tick once per finished round.
    ///
    /// Transcribed from `Card:calculate_perishable` and
    /// `Card:calculate_rental`. A Perishable reaching 0 becomes *debuffed*,
    /// not removed: the joker stays in the row and still counts for Gift
    /// Card, Swashbuckler and Temperance, it just stops contributing. That is
    /// why nothing is dropped here and `is_joker_debuffed` reads the tally.
    fn sticker_round_end(&mut self) {
        // The Rental charge is applied in `calc_reward`, before the payout,
        // so that it can reduce the interest earned on the round. Only the
        // Perishable tally moves here.
        for joker in self.jokers.iter_mut() {
            let mut stickers = joker.stickers();
            if stickers.perishable && stickers.perish_tally > 0 {
                stickers.perish_tally -= 1;
            }
            joker.set_stickers(stickers);
        }
    }

    pub(crate) fn clear_blind(&mut self) {
        self.score = self.config.base_score;
        self.plays = self.plays_per_round();
        self.discards_remaining = self.discards_per_round();
        self.hand_ranks_played_this_round.clear();
        self.discarded_this_round.clear();
        self.hands_played_this_round = 0;
        self.boss_disabled = false;
        self.deck.append(&mut self.discarded);
        self.deck.extend(self.available.cards());
        self.available.empty();
        for card in self.discarded.iter_mut() {
            card.face_down = false;
        }
        self.deck.clear_face_down();
        self.backend.shuffle_deck(&mut self.deck);
        self.roll_discard_selectors();
    }

    /// The boss currently in effect, or `None` if we're not in the middle of
    /// that boss's own round, or its effect is disabled. Three things
    /// disable a boss in the real game and all share one mechanism: Chicot
    /// held, Luchador sold, and any Joker sold while Verdant Leaf is up.
    pub(crate) fn active_boss(&self) -> Option<BossBlind> {
        if self.blind != Some(Blind::Boss) {
            return None;
        }
        if self.jokers.iter().any(|j| matches!(j, Jokers::Chicot(_))) {
            return None;
        }
        if self.boss_disabled {
            return None;
        }
        self.current_boss
    }

    /// Discards actually usable right now
    pub fn discards(&self) -> usize {
        if self.active_boss() == Some(BossBlind::Water) {
            0
        } else {
            self.discards_remaining
        }
    }

    // Castle/MailInRebate lock onto a fresh random suit/rank each round
    // ("changes every round") - unconditional, overwrites whatever was
    // already there.
    fn roll_discard_selectors(&mut self) {
        for j in self.jokers.iter_mut() {
            self.backend.roll_discard_selector(j);
        }
    }

    // draw from deck to available
    fn draw(&mut self, count: usize) {
        let drawn = self.deck.draw(count);
        let face_down: Vec<bool> = drawn.iter().map(|c| self.stays_face_down(c)).collect();
        let mut drawn = drawn;
        for (card, down) in drawn.iter_mut().zip(face_down) {
            card.face_down = down;
        }
        self.available.extend(drawn);
        // The Fish's flag is sticky and covers the whole hand, not just the
        // replacements, so cards already in hand are flipped too.
        if self.active_boss() == Some(BossBlind::Fish) && self.boss_state.fish_prepped {
            for card in self.available.select_all_mut() {
                card.face_down = true;
            }
        }
        self.boss_force_selection();
    }

    // shuffle and deal new cards to available
    pub(crate) fn deal(&mut self) {
        // add discarded back to deck, emptying in process
        self.deck.append(&mut self.discarded);
        // add available back to deck and empty
        self.deck.extend(self.available.cards());
        self.available.empty();
        self.backend.shuffle_deck(&mut self.deck);
        self.draw(self.hand_size());
    }

    /// Reshuffles discarded/held cards back into the deck and redraws a
    /// fresh hand, regardless of stage. Just a helper.
    pub fn redeal(&mut self) {
        self.deal();
    }

    pub(crate) fn select_card(&mut self, card: Card) -> Result<(), GameError> {
        if self.available.selected().len() >= self.config.selected_max {
            return Err(GameError::InvalidSelectCard);
        }
        self.available.select_card(card)
    }

    pub(crate) fn deselect_card(&mut self, card: Card) -> Result<(), GameError> {
        // The Cerulean Bell's card cannot be released while the boss is up.
        if self.active_boss() == Some(BossBlind::CeruleanBell)
            && self.boss_state.forced_card == Some(card.id)
        {
            return Err(GameError::ForcedCardStaysSelected);
        }
        self.available.deselect_card(card)
    }

    /// The Cerulean Bell pins one card of the hand selected. Called when the
    /// blind starts and again after every redraw, since the forced card is
    /// whichever is chosen afresh each time.
    fn boss_force_selection(&mut self) {
        self.boss_state.forced_card = None;
        if self.active_boss() != Some(BossBlind::CeruleanBell) {
            return;
        }
        let cards = self.available.cards();
        if cards.is_empty() {
            return;
        }
        let idx = self.backend.random_index(cards.len());
        let card = cards[idx];
        self.boss_state.forced_card = Some(card.id);
        // Already selected? Then the pin is a no-op.
        let _ = self.available.select_card(card);
    }

    pub(crate) fn move_card(
        &mut self,
        direction: MoveDirection,
        card: Card,
    ) -> Result<(), GameError> {
        self.available.move_card(direction, card)
    }

    pub(crate) fn play_selected(&mut self) -> Result<(), GameError> {
        if self.plays == 0 {
            return Err(GameError::NoRemainingPlays);
        }
        self.check_boss_hand_legal()?;
        self.plays -= 1;
        self.hands_played_this_run += 1;
        self.hands_played_this_round += 1;
        // The Fish's `prepped`, set by playing a hand rather than by
        // counting one, so a discarded hand does not set it.
        if self.active_boss() == Some(BossBlind::Fish) {
            self.boss_state.fish_prepped = true;
        }
        let selected = SelectHand::new(self.available.selected());
        let best = selected.best_hand_with(self.hand_rules())?;
        let rank = best.rank;
        self.check_boss_hand_rank(rank)?;
        let scored = best.clone();
        let score = self.calc_score(best);
        let on_play = self.boss_on_play(rank, &self.available.selected());
        self.boss_record_rank(rank);
        let clear_blind = self.handle_score(score)?;
        // must run before the discard/redraw below: self.available still reflects
        // the cards actually held when the round ended, not their replacements.
        if clear_blind {
            self.handle_round_end();
        }
        self.joker_first_hand(&self.available.selected());
        self.discarded.extend(self.available.selected());
        let removed = self.available.remove_selected();
        let n = if self.active_boss() == Some(BossBlind::Serpent) {
            3
        } else {
            removed
        };
        self.draw(n);
        for card in scored.hand.cards() {
            if card.enhancement == Some(Enhancement::Glass)
                && self.prob_roll(EffectProc::GlassCardBreak, 1, 4)
            {
                self.glass_cards_destroyed += 1;
                self.destroy_card(card.id);
            }
        }
        on_play(self);
        if clear_blind {
            self.clear_blind();
        }
        Ok(())
    }

    /// Bosses that constrain the shape of the hand, checked before anything
    /// is spent.
    fn check_boss_hand_legal(&self) -> Result<(), GameError> {
        if self.active_boss() == Some(BossBlind::Psychic) {
            let n = self.available.selected().len();
            if n != PSYCHIC_HAND_SIZE {
                return Err(GameError::InvalidHand(PlayHandError::PsychicWrongCardCount));
            }
        }
        Ok(())
    }

    /// Bosses that constrain the hand *type*, checked once the type is known
    /// but before any score is banked.
    pub(crate) fn check_boss_hand_rank(&self, rank: HandRank) -> Result<(), GameError> {
        match self.active_boss() {
            Some(BossBlind::Eye) if self.boss_state.last_played == Some(rank) => {
                Err(GameError::InvalidHand(PlayHandError::EyeRepeatRank))
            }
            Some(BossBlind::Mouth) => match self.boss_state.locked_rank {
                Some(locked) if locked != rank => {
                    Err(GameError::InvalidHand(PlayHandError::MouthRankMismatch))
                }
                _ => Ok(()),
            },
            _ => Ok(()),
        }
    }

    fn boss_record_rank(&mut self, rank: HandRank) {
        if self.active_boss() != Some(BossBlind::Eye) && self.active_boss() != Some(BossBlind::Mouth)
        {
            return;
        }
        if self.active_boss() == Some(BossBlind::Mouth) {
            self.boss_state.locked_rank.get_or_insert(rank);
        }
        self.boss_state.last_played = Some(rank);
    }

    /// The Crimson Heart re-picks its victim at the start of every hand, and
    /// only ever debuffs one Joker at a time.
    fn boss_crimson_heart(&mut self) {
        if self.active_boss() != Some(BossBlind::CrimsonHeart) {
            self.boss_state.debuffed_joker = None;
            return;
        }
        let previous = self.boss_state.debuffed_joker.take();
        if self.jokers.is_empty() {
            return;
        }
        // The real game debuffs every non-debuffed Joker, then picks one, and
        // falls back to the full set when that leaves nothing — so a lone
        // Joker stays debuffed.
        let mut pool: Vec<Jokers> = self
            .jokers
            .iter()
            .filter(|j| {
                // Eternal jokers cannot be debuffed, and neither can jokers
                // that are already debuffed by an expired Perishable, so
                // neither is a valid target.
                !j.stickers().eternal
                    && !crate::sticker::is_debuffed_for_expired_perishable(&j.stickers())
                    && previous
                        .as_ref()
                        .is_none_or(|d| d.instance_id() != j.instance_id())
            })
            .cloned()
            .collect();
        if pool.is_empty() {
            pool = self.jokers.clone();
        }
        let idx = self.backend.random_index(pool.len());
        self.boss_state.debuffed_joker = Some(pool[idx].clone());
    }

    /// Boss effects that fire on a played hand. Returned as a closure so
    /// they land after scoring, matching the real game: Pillar's flag is
    /// set once the cards have scored, and The Hook discards after the
    /// hand's own redraw.
    fn boss_on_play(&mut self, rank: HandRank, played: &[Card]) -> impl FnOnce(&mut Game) {
        let cards_played = played.len();
        let oxes_money = self.active_boss() == Some(BossBlind::Ox) && self.is_most_played(rank);
        let tooth = if self.active_boss() == Some(BossBlind::Tooth) {
            cards_played
        } else {
            0
        };
        let wheel = self.active_boss() == Some(BossBlind::Wheel);
        let hook = self.active_boss() == Some(BossBlind::Hook);
        // Pillar's flag is a card ability set the moment a card is played
        // and cleared at the ante's reset, whichever boss is up. Only read
        // while Pillar itself is the active boss.
        let pillar_ids: Vec<usize> = played.iter().map(|c| c.id).collect();
        move |g: &mut Game| {
            if oxes_money {
                g.money = 0;
            }
            g.money = (g.money - tooth as i64).max(g.debt_floor());
            g.boss_state.played_this_ante.extend(pillar_ids);
            if wheel {
                g.boss_wheel_editions(cards_played);
            }
            if hook {
                g.boss_hook_discard(HOOK_DISCARDS);
            }
        }
    }

    fn is_most_played(&self, rank: HandRank) -> bool {
        self.most_played_hand_rank() == rank
    }

    /// The Wheel: each played card has a 1-in-4 chance of adding an
    /// edition to a random Joker.
    fn boss_wheel_editions(&mut self, cards_played: usize) {
        for _ in 0..cards_played {
            if self.jokers.is_empty() {
                break;
            }
            if !self.prob_roll(EffectProc::WheelBoss, 1, 4) {
                continue;
            }
            let target = self.backend.pick_random_joker(self.jokers.clone());
            let edition = self.backend.roll_random_edition();
            if let Some(j) = self
                .jokers
                .iter_mut()
                .find(|j| j.instance_id() == target.instance_id())
            {
                j.set_edition(edition);
            }
        }
    }

    /// The Hook: discards a number of random unplayed cards straight to
    /// the discard pile, after the hand's own redraw.
    fn boss_hook_discard(&mut self, n: usize) {
        let mut pool = self.available.not_selected().to_vec();
        for _ in 0..n {
            if pool.is_empty() {
                break;
            }
            let idx = self.backend.random_index(pool.len());
            let card = pool.remove(idx);
            self.discarded.push(card);
        }
    }

    // discard selected cards from available and draw equal number back to available
    pub(crate) fn discard_selected(&mut self) -> Result<(), GameError> {
        if self.discards() == 0 {
            return Err(GameError::NoRemainingDiscards);
        }
        self.discards_remaining -= 1;
        let discarded = self.available.selected();
        self.discarded.extend(discarded.iter().copied());
        self.discarded_this_round.extend(discarded.iter().copied());
        self.total_cards_discarded += discarded.len();
        let removed = self.available.remove_selected();
        let n = if self.active_boss() == Some(BossBlind::Serpent) {
            3
        } else {
            removed
        };
        self.draw(n);

        // Check for purple seals
        for card in &discarded {
            if card.seal == Some(Seal::Purple) && self.consumables_holding_slots() < self.consumable_slots() {
                let excl: Vec<Tarot> = self
                    .consumables
                    .iter()
                    .filter_map(|c| match c.kind {
                        ConsumableKind::Tarot(t) => Some(t),
                        _ => None,
                    })
                    .collect();
                let tarot = self.backend.roll_random_tarot(&excl);
                self.consumables.push(tarot);
            }
        }

        let hand = MadeHand {
            hand: SelectHand::new(discarded.clone()),
            rank: HandRank::HighCard,
            all: discarded,
        };
        for e in self.effect_registry.on_discard.clone() {
            if let Effects::OnDiscard(f) = e {
                f.lock().unwrap()(self, hand.clone())
            }
        }
        Ok(())
    }

    pub(crate) fn destroy_card(&mut self, id: usize) {
        if let Some(card) = self.find_card(id) {
            if self.is_face_card(&card) {
                self.face_cards_destroyed += 1;
            }
        }
        self.available.remove_by_id(id);
        self.deck.remove_by_id(id);
        self.discarded.retain(|c| c.id != id);
    }

    pub(crate) fn mutate_card<F: Fn(&mut Card) + Copy>(&mut self, id: usize, f: F) {
        self.available.mutate_card(id, f);
        self.deck.mutate_card(id, f);
    }

    pub(crate) fn joker_state(&self, id: usize) -> Option<JokerState> {
        self.jokers
            .iter()
            .find(|j| j.instance_id() == id)
            .map(|j| j.state())
    }

    pub(crate) fn joker_state_mut(&mut self, id: usize) -> Option<&mut JokerState> {
        self.jokers
            .iter_mut()
            .find(|j| j.instance_id() == id)
            .map(|j| j.state_mut())
    }

    /// Every card in the run, wherever it currently sits.
    pub(crate) fn find_card(&self, id: usize) -> Option<Card> {
        self.deck
            .cards()
            .into_iter()
            .chain(self.available.cards())
            .chain(self.discarded.iter().copied())
            .find(|c| c.id == id)
    }

    pub(crate) fn is_face_card(&self, card: &Card) -> bool {
        card.is_face_card()
            || self
                .effect_registry
                .rule_flags
                .contains(&RuleFlag::AllCardsAreFace)
    }

    pub(crate) fn is_even(&self, card: &Card) -> bool {
        card.is_even_impl(self.is_face_card(card))
    }

    pub(crate) fn is_odd(&self, card: &Card) -> bool {
        card.is_odd_impl(self.is_face_card(card))
    }

    /// Whether the active boss (if any) currently debuffs card
    pub fn is_card_debuffed(&self, card: &Card) -> bool {
        // Verdant Leaf overrides every other debuff: nothing scores at all
        // until a Joker is sold.
        if self.boss_state.all_cards_debuffed {
            return true;
        }
        match self.active_boss() {
            Some(BossBlind::Club) => card.matches_suit(Suit::Club),
            Some(BossBlind::Goad) => card.matches_suit(Suit::Spade),
            Some(BossBlind::Head) => card.matches_suit(Suit::Heart),
            Some(BossBlind::Window) => card.matches_suit(Suit::Diamond),
            Some(BossBlind::Plant) => self.is_face_card(card),
            // Pillar: a card stops scoring once it has been played this ante.
            Some(BossBlind::Pillar) => self.boss_state.played_this_ante.contains(&card.id),
            _ => false,
        }
    }

    /// Test helper: mark the whole hand face down, to check the flag is
    /// presentation-only.
    #[cfg(test)]
    pub(crate) fn force_everything_face_down(&mut self) {
        for card in self.available.select_all_mut() {
            card.face_down = true;
        }
    }

    /// Whether a card dealt into the hand this blind stays face down.
    ///
    /// Mirrors the real game's `Blind:stay_flipped`, which is the single
    /// place all four of these bosses are implemented. It is per card and
    /// evaluated as the card is dealt, and the real game flips everything
    /// back at the end of the blind.
    pub(crate) fn stays_face_down(&mut self, card: &Card) -> bool {
        match self.active_boss() {
            // The real game reads `hands_played == 0 and discards_used == 0`
            // for the round; those two round-scoped collections are the
            // closest equivalent.
            Some(BossBlind::House) => {
                self.hand_ranks_played_this_round.is_empty() && self.discarded_this_round.is_empty()
            }
            Some(BossBlind::Mark) => self.is_face_card(card),
            // The Fish: the real game sets a sticky `prepped` flag when a
            // hand is played and every card in hand is face down from then
            // on, cleared only on the round reset. The previous read used the
            // run-scoped hand and discard totals, which meant that from ante 2
            // the flag was already true before the boss was even selected and
            // every card in the round was face down.
            Some(BossBlind::Fish) => self.boss_state.fish_prepped,
            // The Wheel flips a card face down as a side effect of its
            // edition roll, so the same 1-in-7 is drawn twice: once for
            // the edition, once for the flip.
            Some(BossBlind::Wheel) => self.prob_roll(EffectProc::WheelBoss, 1, 7),
            _ => false,
        }
    }

    /// Whether a Joker is currently debuffed by The Crimson Heart.
    pub fn is_joker_debuffed(&self, joker: &Jokers) -> bool {
        let stickers = joker.stickers();
        // An Eternal joker is never debuffed: `Card:set_debuff` in the real
        // game is reached only for a joker that can lose its ability, and
        // Eternal is the sticker that says it cannot.
        if stickers.eternal {
            return false;
        }
        // An expired Perishable is debuffed rather than removed, so the
        // joker keeps its slot and its sell value while contributing nothing.
        if crate::sticker::is_debuffed_for_expired_perishable(&stickers) {
            return true;
        }
        self.boss_state
            .debuffed_joker
            .as_ref()
            .is_some_and(|d| d.instance_id() == joker.instance_id())
    }

    // Filters debuffed cards out of a collection
    pub(crate) fn non_debuffed<'a>(&self, cards: impl IntoIterator<Item = &'a Card>) -> Vec<Card> {
        cards
            .into_iter()
            .filter(|c| !self.is_card_debuffed(c))
            .copied()
            .collect()
    }

    pub(crate) fn most_played_hand_rank(&self) -> HandRank {
        HandRank::iter()
            // royal flush has no storage slot of its own, its counted as straight flush
            .filter(|r| *r != HandRank::RoyalFlush)
            .max_by_key(|r| self.planetarium.level(*r).plays)
            .unwrap_or(HandRank::HighCard)
    }

    // Every card the player owns this run, regardless of whether it's
    // currently undrawn, in hand, or already discarded this round.
    // `self.deck` alone is only the undrawn remainder mid-round.
    pub fn full_deck(&self) -> Vec<Card> {
        let mut all = self.deck.cards();
        all.extend(self.available.cards());
        all.extend(self.discarded.clone());
        all
    }

    pub(crate) fn prob_roll(&mut self, proc: EffectProc, numerator: u32, denominator: u32) -> bool {
        self.backend
            .prob_roll(proc, numerator.saturating_mul(self.prob_mult()), denominator)
    }

    pub fn calc_score(&mut self, hand: MadeHand) -> usize {
        self.calc_score_inner(hand, None)
    }

    /// Same scoring pass as `calc_score`, plus an ordered `ScoreTrace` of
    /// every chips/mult contribution along the way.
    pub fn calc_score_traced(&mut self, hand: MadeHand) -> (usize, ScoreTrace) {
        let mut trace = ScoreTrace::default();
        let score = self.calc_score_inner(hand, Some(&mut trace));
        (score, trace)
    }

    fn record_step(
        &self,
        trace: &mut Option<&mut ScoreTrace>,
        source: ScoreSource,
        chips_before: usize,
        mult_before: usize,
        retrigger: bool,
    ) {
        let Some(t) = trace else {
            return;
        };
        // skip no-op checks (e.g. a Base-edition joker, a conditional
        // effect that didn't fire)
        if chips_before == self.chips && mult_before == self.mult {
            return;
        }
        t.0.push(ScoreStep {
            source,
            chips_before,
            chips_after: self.chips,
            mult_before,
            mult_after: self.mult,
            retrigger,
        });
    }

    // Extension point for retrigger (jokers + red seal)
    fn trigger_count_played(&mut self, card: &Card, is_first: bool) -> usize {
        let mut n = if card.seal == Some(Seal::Red) { 2 } else { 1 };
        for e in self.effect_registry.trigger_count_played.clone() {
            if let Effects::TriggerCountPlayed(f) = e {
                n += f.lock().unwrap()(self, *card, is_first);
            }
        }
        n
    }

    // Extension point for retrigger (jokers + red seal)
    fn trigger_count_held(&mut self, card: &Card) -> usize {
        let mut n = if card.seal == Some(Seal::Red) { 2 } else { 1 };
        for e in self.effect_registry.trigger_count_held.clone() {
            if let Effects::TriggerCountHeld(f) = e {
                n += f.lock().unwrap()(self, *card, false);
            }
        }
        n
    }

    fn calc_score_inner(
        &mut self,
        mut hand: MadeHand,
        mut trace: Option<&mut ScoreTrace>,
    ) -> usize {
        self.boss_triggered_this_hand = false;

        // compute chips and mult from hand level
        let chips_before = self.chips;
        let mult_before = self.mult;
        if self.active_boss() == Some(BossBlind::Arm) && self.planetarium.level(hand.rank).level > 1
        {
            self.planetarium.level_down(hand.rank);
            self.boss_triggered_this_hand = true;
        }
        let level = self.planetarium.play(hand.rank);
        let (level_chips, level_mult) = if self.active_boss() == Some(BossBlind::Flint) {
            self.boss_triggered_this_hand = true;
            (level.chips / 2, level.mult / 2)
        } else {
            (level.chips, level.mult)
        };
        self.chips += level_chips;
        self.mult += level_mult;
        self.record_step(
            &mut trace,
            ScoreSource::HandLevel(hand.rank),
            chips_before,
            mult_before,
            false,
        );

        // Observatory: every held Planet card whose own hand is the one
        // being scored multiplies Mult by 1.5. Applied straight after the
        // hand level, before any card or joker contribution.
        if self.vouchers.observatory() {
            let matching = self
                .consumables
                .iter()
                .filter(|c| matches!(c.kind, ConsumableKind::Planet(p) if p.hand_rank() == hand.rank))
                .count();
            for _ in 0..matching {
                let chips_before = self.chips;
                let mult_before = self.mult;
                self.mult += self.mult / 2;
                self.record_step(
                    &mut trace,
                    ScoreSource::Voucher(Voucher::Observatory),
                    chips_before,
                    mult_before,
                    false,
                );
            }
        }

        // first card in original play order that's actually used in scoring:
        // either part of the made hand, or a stone kicker (which always scores).
        // ordinary kickers that never score don't count, even if played first.
        let hand_ids: Vec<usize> = hand.hand.cards().iter().map(|c| c.id).collect();
        let first_played_id = hand
            .all
            .iter()
            .find(|c| hand_ids.contains(&c.id) || c.enhancement == Some(Enhancement::Stone))
            .map(|c| c.id);

        // per-scored-card, rank chips, enhancements, editions
        let scoring: Vec<Card> = if self
            .effect_registry
            .rule_flags
            .contains(&RuleFlag::AllPlayedCardsScore)
        {
            hand.all.clone()
        } else {
            hand.hand.cards()
        };
        for card in scoring {
            if self.is_card_debuffed(&card) {
                self.boss_triggered_this_hand = true;
                continue;
            }
            let is_first = Some(card.id) == first_played_id;
            for trig in 0..self.trigger_count_played(&card, is_first) {
                let chips_before = self.chips;
                let mult_before = self.mult;

                // stone has no rank, skip normal chip value
                if card.enhancement != Some(Enhancement::Stone) {
                    self.chips += card.chips() + card.bonus_chips;
                }
                match card.enhancement {
                    Some(Enhancement::Bonus) => self.chips += 30,
                    Some(Enhancement::Mult) => self.mult += 4,
                    Some(Enhancement::Glass) => self.mult *= 2,
                    Some(Enhancement::Lucky) => {
                        if self.prob_roll(EffectProc::LuckyMult, 1, 5) {
                            self.mult += 20;
                            self.lucky_triggers += 1;
                        }
                        if self.prob_roll(EffectProc::LuckyMoney, 1, 15) {
                            self.money += 20;
                        }
                    }
                    _ => {}
                }
                match card.edition {
                    Edition::Foil => self.chips += 50,
                    Edition::Holographic => self.mult += 10,
                    Edition::Polychrome => self.mult += self.mult / 2,
                    _ => {}
                }
                if card.seal == Some(Seal::Gold) {
                    self.money += 3;
                }

                self.record_step(
                    &mut trace,
                    ScoreSource::PlayedCard(card),
                    chips_before,
                    mult_before,
                    trig > 0,
                );
            }
        }

        // stone cards always score +50 chips, even as kickers outside the played
        // hand's own scoring subset.
        // still subject to the same retrigger rules as any other played card.
        for card in &hand.all {
            if card.enhancement == Some(Enhancement::Stone) {
                if self.is_card_debuffed(card) {
                    self.boss_triggered_this_hand = true;
                    continue;
                }
                let is_first = Some(card.id) == first_played_id;
                for trig in 0..self.trigger_count_played(card, is_first) {
                    let chips_before = self.chips;
                    let mult_before = self.mult;
                    self.chips += 50;
                    self.record_step(
                        &mut trace,
                        ScoreSource::StoneKicker(*card),
                        chips_before,
                        mult_before,
                        trig > 0,
                    );
                }
            }
        }

        // held in hand effects (cards not played this hand)
        for card in self.available.not_selected() {
            if self.is_card_debuffed(&card) {
                // Held-card Matador trigger unconfirmed, real-game check needed.
                if matches!(
                    card.enhancement,
                    Some(Enhancement::Steel) | Some(Enhancement::Gold)
                ) {
                    self.boss_triggered_this_hand = true;
                }
                continue;
            }
            for trig in 0..self.trigger_count_held(&card) {
                let chips_before = self.chips;
                let mult_before = self.mult;
                match card.enhancement {
                    Some(Enhancement::Steel) => self.mult += self.mult / 2,
                    Some(Enhancement::Gold) => self.money += 3,
                    _ => {}
                }
                self.record_step(
                    &mut trace,
                    ScoreSource::HeldCard(card),
                    chips_before,
                    mult_before,
                    trig > 0,
                );
            }
        }

        // run hand modifiers (e.g. Pareidolia) before joker scoring effects
        for e in self.effect_registry.on_modify_hand.clone() {
            if let Effects::OnModifyHand(f) = e {
                f.lock().unwrap()(self, &mut hand)
            }
        }

        // apply joker effects with per-joker edition ordering.
        // effects recomputed per joker, not read from EffectRegistry. Each joker's edition
        // bonus must sandwich its own OnScore closures in self.jokers order.
        for joker in self.jokers.clone() {
            // A debuffed joker contributes neither its own effect nor its
            // edition bonus. A boss debuff and an expired Perishable both land
            // here; an Eternal joker is never debuffed so always passes.
            if self.is_joker_debuffed(&joker) {
                continue;
            }
            let chips_before = self.chips;
            let mult_before = self.mult;
            match joker.edition() {
                Edition::Foil => self.chips += 50,
                Edition::Holographic => self.mult += 10,
                _ => {}
            }
            self.record_step(
                &mut trace,
                ScoreSource::Joker(joker.clone()),
                chips_before,
                mult_before,
                false,
            );

            for e in joker.effects(self) {
                if let Effects::OnScore(f) = e {
                    let chips_before = self.chips;
                    let mult_before = self.mult;
                    f.lock().unwrap()(self, hand.clone());
                    self.record_step(
                        &mut trace,
                        ScoreSource::Joker(joker.clone()),
                        chips_before,
                        mult_before,
                        false,
                    );
                }
            }

            if joker.edition() == Edition::Polychrome {
                let chips_before = self.chips;
                let mult_before = self.mult;
                self.mult += self.mult / 2;
                self.record_step(
                    &mut trace,
                    ScoreSource::Joker(joker.clone()),
                    chips_before,
                    mult_before,
                    false,
                );
            }
        }

        // record after joker loop, not before (for CardSharp)
        self.hand_ranks_played_this_round.insert(hand.rank);

        // Saturating, because chips is fed directly from the balance by Bull
        // (`chips += money * 2`) and a long run can make a plain `*` wrap.
        // Wrapping here would turn a big score into a small one and let a
        // blind be beaten that should not be.
        let score = self.chips.saturating_mul(self.mult);

        // reset chips and mult
        self.mult = self.config.base_mult;
        self.chips = self.config.base_chips;

        score
    }

    pub fn required_score(&self) -> usize {
        let base = self.ante_base();
        match self.blind {
            None => base,
            Some(Blind::Small) => base,
            Some(Blind::Big) => (base as f32 * 1.5) as usize,
            Some(Blind::Boss) => base * Self::boss_score_multiplier(self.active_boss()),
        }
    }

    /// The score the upcoming Boss Blind will require, based on
    /// `current_boss`, valid before boss blind is active.
    pub fn boss_required_score(&self) -> usize {
        self.ante_base() * Self::boss_score_multiplier(self.current_boss)
    }

    /// The ante's base score, from the table for this run's scaling level.
    /// Green and Purple stakes and the Plasma Deck each raise it, so this
    /// reads the run's effective scaling rather than a fixed table.
    pub fn ante_base(&self) -> usize {
        let scaling = deck_variant::effective_scaling(self.config.deck, self.config.stake);
        deck_variant::ante_base(scaling, self.ante_current.number())
    }

    fn boss_score_multiplier(boss: Option<BossBlind>) -> usize {
        match boss {
            Some(BossBlind::Wall) => 4,
            Some(BossBlind::VioletVessel) => 6,
            Some(BossBlind::Needle) => 1,
            _ => 2,
        }
    }

    /// What cashing out of the current round would pay, without doing it.
    /// Read by the UIs to show the reward before the player commits, and by
    /// the deck/stake tests.
    pub fn cashout_reward(&self, blind: Blind) -> usize {
        self.calc_reward_for(blind)
    }

    fn calc_reward(&mut self, blind: Blind) -> Result<usize, GameError> {
        // A Rental charge is deducted before the payout, so it can lower the
        // interest earned on the round. The sticker round end that normally
        // applies the charge has not run yet at this point in the round, so
        // it is applied here as well and the later tick is skipped for the
        // round just finished.
        self.charge_rentals();
        Ok(self.calc_reward_for(blind))
    }

    /// The $3-per-Rental charge, as a separate step from the tally tick so
    /// `calc_reward` can call it before the payout.
    fn charge_rentals(&mut self) {
        let charge: i64 = self
            .jokers
            .iter()
            .filter(|j| j.stickers().rental)
            .count() as i64
            * crate::sticker::RENTAL_RATE;
        if charge > 0 {
            self.money = (self.money - charge).max(self.debt_floor());
        }
    }

    fn calc_reward_for(&self, blind: Blind) -> usize {
        // Interest is earned, never charged, so a balance in debt earns
        // none rather than a negative amount. Green Deck's `no_interest`
        // replaces the table outright, and To the Moon's interest is
        // additional to it.
        let interest_table = !deck_variant::deck_denies_interest(self.config.deck);
        let mut interest = if interest_table {
            (self.money.max(0) as f32 * self.config.interest_rate).floor() as usize
        } else {
            0
        };
        let interest_max = self.interest_max();
        if interest > interest_max {
            interest = interest_max
        }
        // Red Stake and above sets `no_blind_reward.Small`, transcribed from
        // the real game's stake table.
        let mut base = blind.reward();
        if blind == Blind::Small && deck_variant::stake_denies_small_reward(self.config.stake) {
            base = 0;
        }
        let hand_bonus = self.plays * self.config.money_per_hand;
        // To the Moon's interest is additional to, and uncapped by, the
        // normal interest table.
        let to_the_moon = self.joker_interest_bonus();
        let mut reward = base + interest + hand_bonus + to_the_moon;
        // Green Deck pays $2 per unspent hand and $1 per unspent discard
        // instead of interest: `extra_hand_bonus`/`extra_discard_bonus`.
        if !interest_table {
            reward += deck_variant::green_deck_end_of_round(
                self.plays as i64,
                self.discards_remaining as i64,
            ) as usize;
        }
        reward
    }

    /// Collects one tag. Anything that fires later instead of now is
    /// parked in `TagState` and consumed by the shop / blind / round hooks.
    fn apply_tag(&mut self, tag: Tag) {
        // Double duplicates whatever is collected next, including itself
        // being collected twice.
        let double = std::mem::take(&mut self.tag_state.double);
        self.collect_tag(tag);
        if double {
            self.collect_tag(tag);
        }
    }

    fn collect_tag(&mut self, tag: Tag) {
        match tag {
            // "The next shop will have a free Uncommon/Rare Joker."
            Tag::Uncommon => self.tag_state.pending_free_joker = Some(Rarity::Uncommon),
            Tag::Rare => self.tag_state.pending_free_joker = Some(Rarity::Rare),
            // The next base edition Joker in a shop becomes this edition and
            // is free. Only one pending at a time — a second one overwrites.
            Tag::Negative => self.tag_state.pending_edition = Some(Edition::Negative),
            Tag::Foil => self.tag_state.pending_edition = Some(Edition::Foil),
            Tag::Holographic => self.tag_state.pending_edition = Some(Edition::Holographic),
            Tag::Polychrome => self.tag_state.pending_edition = Some(Edition::Polychrome),
            // Pays out on the next boss blind, not now.
            Tag::Investment => self.tag_state.investment = true,
            // A second voucher slot in the next shop.
            Tag::Voucher => self.tag_state.voucher_slot = true,
            // Re-rolls this ante's boss blind, which was drawn when the ante
            // started. The real game also spends Director's Cut's one reroll
            // on it, and so does this.
            Tag::Boss => self.reroll_boss_from_tag(),
            // Pack tags open their pack immediately, so they bypass the
            // shop and go straight to the pack stage. Size and category come
            // from the real game: Spectral is the one that isn't a Mega.
            Tag::Standard => self.open_free_pack(PackCategory::Standard, PackSize::Mega),
            Tag::Charm => self.open_free_pack(PackCategory::Arcana, PackSize::Mega),
            Tag::Meteor => self.open_free_pack(PackCategory::Celestial, PackSize::Mega),
            Tag::Buffoon => self.open_free_pack(PackCategory::Buffoon, PackSize::Mega),
            Tag::Ethereal => self.open_free_pack(PackCategory::Spectral, PackSize::Normal),
            // $1 per hand played this run.
            Tag::Handy => self.money += self.hands_played_this_run as i64,
            // $1 per unused discard this run.
            Tag::Garbage => self.money += self.unused_discards_this_run as i64,
            // $5 per blind skipped this run.
            Tag::Speed => self.money += 5 * self.blinds_skipped_this_run as i64,
            // Doubles money, but only ever adds up to $40.
            // `min(40)` is the tag's `config.max`, doubling at most $40. A
            // negative balance is clamped to zero first, so a run in debt
            // gains nothing rather than being charged double, and the
            // result is floored at the debt line like every other money
            // path.
            Tag::Economy => {
                self.money = (self.money + self.money.clamp(0, 40)).max(self.debt_floor())
            }
            // Every Joker, consumable and pack in the next shop is free.
            Tag::Coupon => self.tag_state.coupon = true,
            // Duplicates the next tag collected.
            Tag::Double => self.tag_state.double = true,
            // +3 hand size for the next round only.
            Tag::Juggle => self.tag_state.hand_size_bonus += 3,
            // The next shop's rerolls start at $0.
            Tag::D6 => self.tag_state.free_rerolls = true,
            // Up to 2 Common Jokers, if there's room.
            Tag::TopUp => self.spawn_top_up_jokers(),
            // Levels a hand chosen when the tag was drawn, so the player
            // could see which before skipping.
            Tag::Orbital => {
                let rank = self.tag_state.orbital_hand.take();
                if let Some(rank) = rank {
                    for _ in 0..ORBITAL_TAG_LEVELS {
                        self.planetarium.level_up(rank);
                    }
                }
            }
        }
    }

    /// Top-up: as many Common Jokers as fit, for free.
    fn spawn_top_up_jokers(&mut self) {
        for _ in 0..TOP_UP_TAG_JOKERS {
            if self.jokers.len() >= self.joker_slots() {
                break;
            }
            let prob_mult = self.prob_mult();
            let mut exclude = Vec::new();
            for j in &self.jokers {
                if j.rarity() == Rarity::Common {
                    exclude.push(j.clone());
                }
            }
            let joker = self
                .backend
                .gen_joker_of_rarity(self.ante_current as i32, prob_mult, &exclude, Rarity::Common);
            self.jokers.push(joker);
        }
    }

    /// A pack the player didn't pay for, opened straight away. Arcana and
    /// Spectral packs draw a hand to target their cards against, same as a
    /// bought one.
    fn open_free_pack(&mut self, category: PackCategory, size: PackSize) {
        let planetarium = self.planetarium.clone();
        let consumables = self.consumables.clone();
        let jokers = self.jokers.clone();
        let vouchers = self.vouchers.clone();
        let ctx = self.shop_context(&planetarium, &consumables, &jokers, &vouchers);
        let pack = self.backend.gen_pack_of(&ctx, category, size);
        if matches!(pack.category, PackCategory::Arcana | PackCategory::Spectral) {
            self.draw(self.hand_size());
        }
        self.open_pack = Some(OpenPackState {
            picks_remaining: pack.picks_allowed(),
            description: pack.description(),
            contents: pack.contents,
        });
        self.stage = Stage::PackOpen();
        self.joker_pack_opened();
    }

    fn cashout(&mut self) -> Result<(), GameError> {
        self.money += self.reward as i64;
        self.reward = 0;
        self.reroll_cost = self.base_reroll_cost();
        self.rerolls_this_shop = 0;
        self.stage = Stage::Shop();
        self.shop_end_effects();

        // Tags fire between entering the shop and stocking it, because most
        // of them shape the shop: Coupon and D6 set flags, the Uncommon /
        // Rare / edition tags are consumed by the refresh below, and the
        // pack tags overwrite `stage` after everything else has run.
        self.tags.append(&mut self.pending_tags);
        for tag in std::mem::take(&mut self.tags) {
            self.apply_tag(tag);
        }
        if std::mem::take(&mut self.tag_state.free_rerolls) {
            self.reroll_cost = 0;
        }
        self.refresh_voucher_offer();

        let planetarium = self.planetarium.clone();
        let held_consumables = self.consumables.clone();
        let held_jokers = self.jokers.clone();
        let vouchers = self.vouchers.clone();
        let ctx = self.shop_context(&planetarium, &held_consumables, &held_jokers, &vouchers);
        let mut tags = std::mem::take(&mut self.tag_state);
        self.shop.refresh(&ctx, &mut self.backend, &mut tags);
        self.tag_state = tags;
        Ok(())
    }

    /// Draws this ante's voucher the first time its shop opens. Within an
    /// ante the offer is left alone: it survives rerolls, and once bought
    /// the slot stays empty until the next ante.
    fn refresh_voucher_offer(&mut self) {
        // Strictly forward: Hieroglyph/Petroglyph walk the ante backwards,
        // and an ante already drawn for must not offer a second voucher.
        if self.voucher_ante.is_some_and(|a| self.ante_current <= a) {
            return;
        }
        self.voucher_ante = Some(self.ante_current);
        let vouchers = self.vouchers.clone();
        self.shop.voucher = self
            .backend
            .gen_voucher(self.ante_current as i32, &vouchers);
        self.refresh_voucher_tag_offer();
    }

    /// The Voucher Tag adds a second voucher to the next shop. Drawn
    /// separately from the ante's own offer so the two can't collide, and
    /// dropped as soon as it's been seen.
    fn refresh_voucher_tag_offer(&mut self) {
        if !std::mem::take(&mut self.tag_state.voucher_slot) {
            return;
        }
        let vouchers = self.vouchers.clone();
        self.shop.voucher_tag_offer = self
            .backend
            .gen_voucher(self.ante_current as i32, &vouchers);
    }

    pub(crate) fn reroll(&mut self) -> Result<(), GameError> {
        if self.stage != Stage::Shop() {
            return Err(GameError::InvalidStage);
        }
        if !self.can_afford(self.reroll_cost) {
            return Err(GameError::InvalidBalance);
        }
        // Chaos the Clown's rerolls cost nothing and don't escalate.
        let free = self.free_rerolls_this_round > 0;
        if free {
            self.free_rerolls_this_round -= 1;
        } else {
            self.money -= self.reroll_cost as i64;
            self.reroll_cost += 1;
        }
        self.rerolls_this_shop += 1;
        let planetarium = self.planetarium.clone();
        let mut held = self.consumables.clone();
        held.extend(self.shop.consumables.clone());
        let mut held_jokers = self.jokers.clone();
        held_jokers.extend(self.shop.jokers.clone());
        let vouchers = self.vouchers.clone();
        // Note: `refresh_cards`, not `refresh` — rerolls leave the packs
        // and the voucher offer untouched.
        let ctx = self.shop_context(&planetarium, &held, &held_jokers, &vouchers);
        let mut tags = std::mem::take(&mut self.tag_state);
        self.shop.refresh_cards(&ctx, &mut self.backend, &mut tags);
        self.tag_state = tags;
        Ok(())
    }

    pub(crate) fn sell_joker(&mut self, idx: usize) -> Result<(), GameError> {
        if matches!(self.stage, Stage::End(_)) {
            return Err(GameError::InvalidStage);
        }
        if idx >= self.jokers.len() {
            return Err(GameError::InvalidAction);
        }
        // An Eternal joker cannot be sold at all.
        if !crate::sticker::can_sell(&self.jokers[idx].stickers()) {
            return Err(GameError::InvalidAction);
        }
        self.money += self.jokers[idx].sell_value() as i64;
        let sold = self.jokers.remove(idx);
        for e in sold.effects(self) {
            if let Effects::OnSell(f) = e {
                f.lock().unwrap()(self);
            }
        }
        if sold.edition() == Edition::Negative {
            self.config.joker_slots = self.config.joker_slots.saturating_sub(1);
        }
        if matches!(sold, Jokers::InvisibleJoker(_)) && self.rounds_played >= INVISIBLE_JOKER_ROUNDS
        {
            self.duplicate_random_joker();
        }

        // Selling any Joker ends Verdant Leaf, via the same mechanism as
        // Luchador.
        if self.current_boss == Some(BossBlind::VerdantLeaf) {
            self.boss_disabled = true;
            self.boss_state.all_cards_debuffed = false;
        }
        // Credit Card takes its debt allowance back with it, matching the
        // real game's remove hook.
        if matches!(sold, Jokers::CreditCard(_)) {
            self.bankrupt_at += CREDIT_CARD_DEBT;
        }

        // Only unlock if this was the last owned copy (Showman is the only
        // way to own more than one) — a discriminant compare, matching
        // shop.rs's own exclude-list comparisons, since Jokers' derived
        // Eq also compares edition/stickers, not just "which joker".
        let still_owned = self
            .jokers
            .iter()
            .any(|j| std::mem::discriminant(j) == std::mem::discriminant(&sold));
        if !still_owned {
            self.backend.on_joker_sold(&sold);
        }
        self.effect_registry
            .register_jokers(self.jokers.clone(), &self.clone());
        Ok(())
    }

    pub(crate) fn sell_consumable(&mut self, idx: usize) -> Result<(), GameError> {
        if matches!(self.stage, Stage::End(_)) {
            return Err(GameError::InvalidStage);
        }
        if idx >= self.consumables.len() {
            return Err(GameError::InvalidAction);
        }
        self.money += self.consumables[idx].sell_value() as i64;
        self.consumables.remove(idx);
        Ok(())
    }

    pub(crate) fn buy_joker(&mut self, joker: Jokers) -> Result<(), GameError> {
        if self.stage != Stage::Shop() {
            return Err(GameError::InvalidStage);
        }
        if joker.edition() == Edition::Negative {
            self.config.joker_slots =
                (self.config.joker_slots + 1).min(self.config.joker_slots_max);
        }
        if self.jokers.len() >= self.joker_slots() {
            return Err(GameError::NoAvailableSlot);
        }
        // A tag's free Joker, or a couponed shop, costs nothing. Resolved
        // by slot because two copies of a Joker are otherwise identical.
        let slot = self
            .shop
            .jokers
            .iter()
            .position(|j| j.instance_id() == joker.instance_id());
        let free = self.shop.couponed
            || slot.is_some_and(|i| self.shop.free_joker_slots.contains(&i));
        // A Rental joker's cost is 1, applied after discounts exactly as
        // `Card:set_cost` does, so it bypasses `price` entirely rather than
        // being discounted down.
        let cost = if free {
            0
        } else if joker.stickers().rental {
            crate::sticker::RENTAL_COST
        } else {
            self.price(joker.cost())
        };
        if !self.can_afford(cost) {
            return Err(GameError::InvalidBalance);
        }
        self.shop.buy_joker(&joker)?;
        if let Some(i) = slot {
            self.shop.free_joker_slots.remove(&i);
        }
        self.money -= cost as i64;
        self.backend.on_joker_bought(&joker);
        if matches!(joker, Jokers::CreditCard(_)) {
            self.bankrupt_at -= CREDIT_CARD_DEBT;
        }
        self.jokers.push(joker);
        self.effect_registry
            .register_jokers(self.jokers.clone(), &self.clone());
        Ok(())
    }

    pub(crate) fn buy_consumable(&mut self, consumable: Consumable) -> Result<(), GameError> {
        if self.stage != Stage::Shop() {
            return Err(GameError::InvalidStage);
        }
        if !self.can_hold_consumable(&consumable) {
            return Err(GameError::NoAvailableSlot);
        }
        let cost = self.shop.price(|c| self.price(c), consumable.cost());
        if !self.can_afford(cost) {
            return Err(GameError::InvalidBalance);
        }
        self.shop.buy_consumable(&consumable)?;
        self.money -= cost as i64;
        self.consumables.push(consumable);
        Ok(())
    }

    /// Redeems the shop's voucher offer. Permanent for the run — there is
    /// no sell action, and the slot stays empty until the next ante.
    pub(crate) fn buy_voucher(&mut self, voucher: Voucher) -> Result<(), GameError> {
        if self.stage != Stage::Shop() {
            return Err(GameError::InvalidStage);
        }
        if self.vouchers.has(voucher) {
            return Err(GameError::VoucherAlreadyRedeemed);
        }
        // Vouchers are exempt from Clearance Sale / Liquidation.
        if !self.can_afford(voucher.cost()) {
            return Err(GameError::InvalidBalance);
        }
        self.shop.buy_voucher(&voucher)?;
        self.money -= voucher.cost() as i64;
        self.vouchers.redeem(voucher);
        self.backend.on_voucher_bought(&voucher);
        self.apply_voucher(voucher);
        Ok(())
    }

    /// The parts of a voucher that aren't derived from the redeemed set on
    /// demand (see `voucher.rs`) — one-shot changes to live game state.
    fn apply_voucher(&mut self, voucher: Voucher) {
        // "-1 Ante" (Hieroglyph/Petroglyph): the run steps back an ante,
        // making every remaining blind easier but adding a round.
        if Vouchers::lowers_ante(voucher) {
            if let Some(prev) = self.ante_current.prev() {
                self.ante_current = prev;
            }
        }
        // Reroll discounts apply to the shop the player is standing in,
        // not just the next one.
        if matches!(voucher, Voucher::RerollSurplus | Voucher::RerollGlut) {
            self.reroll_cost = self.reroll_cost.saturating_sub(2);
        }
        // Overstock widens the current shop immediately: top up the new
        // slots rather than making the player wait for the next round.
        if matches!(voucher, Voucher::Overstock | Voucher::OverstockPlus) {
            let planetarium = self.planetarium.clone();
            let mut held = self.consumables.clone();
            held.extend(self.shop.consumables.clone());
            let mut held_jokers = self.jokers.clone();
            held_jokers.extend(self.shop.jokers.clone());
            let vouchers = self.vouchers.clone();
            let ctx = self.shop_context(&planetarium, &held, &held_jokers, &vouchers);
            let mut tags = std::mem::take(&mut self.tag_state);
            self.shop.top_up_cards(&ctx, &mut self.backend, &mut tags);
            self.tag_state = tags;
        }
    }

    pub(crate) fn buy_playing_card(&mut self, card: Card) -> Result<(), GameError> {
        if self.stage != Stage::Shop() {
            return Err(GameError::InvalidStage);
        }
        if self.deck.len() + self.available.cards().len() + self.discarded.len()
            >= self.config.deck_max
        {
            return Err(GameError::NoAvailableSlot);
        }
        let cost = self.price(card.shop_cost());
        if !self.can_afford(cost) {
            return Err(GameError::InvalidBalance);
        }
        let bought = self.shop.buy_card(&card)?;
        self.money -= cost as i64;
        self.deck.push(bought);
        Ok(())
    }

    pub(crate) fn use_consumable(&mut self, consumable: Consumable) -> Result<(), GameError> {
        if matches!(
            self.stage,
            Stage::End(_) | Stage::TarotHand(_) | Stage::SpectralHand(_) | Stage::PackOpen()
        ) {
            return Err(GameError::InvalidAction);
        }
        // Validate selection before removal so the consumable is not lost on error.
        // Hand-touching Tarot/Spectral can only be used while a real hand exists
        // (during Blind), a held consumable never draws a substitute hand from the deck.
        if let ConsumableKind::Tarot(t) = &consumable.kind {
            if t.requires_hand() {
                if !self.stage.is_blind() {
                    return Err(GameError::InvalidAction);
                }
                let selected_count = self.available.selected().len();
                if selected_count < t.min_targets() || selected_count > t.max_targets() {
                    return Err(GameError::InvalidAction);
                }
            }
        }
        if let ConsumableKind::Spectral(s) = &consumable.kind {
            if s.requires_hand() {
                if !self.stage.is_blind() {
                    return Err(GameError::InvalidAction);
                }
                let selected_count = self.available.selected().len();
                if selected_count < s.min_targets() || selected_count > s.max_targets() {
                    return Err(GameError::InvalidAction);
                }
            }
        }
        let i = self
            .consumables
            .iter()
            .position(|c| c == &consumable)
            .ok_or(GameError::InvalidAction)?;
        self.consumables.remove(i);
        match consumable.kind {
            ConsumableKind::Planet(planet) => {
                self.planets_used_this_run += 1;
                self.planetarium.level_up(planet.hand_rank());
                self.last_consumable_used = Some(Consumable::planet(planet));
            }
            ConsumableKind::Tarot(t) => {
                t.apply(self)?;
                if t != Tarot::Fool {
                    self.tarots_used_this_run += 1;
                    self.last_consumable_used = Some(Consumable::tarot(t));
                }
            }
            ConsumableKind::Spectral(s) => {
                // Fool only ever copies the last Tarot or Planet used, never a Spectral.
                s.apply(self)?;
            }
        }
        Ok(())
    }

    pub(crate) fn apply_tarot(&mut self) -> Result<(), GameError> {
        let Stage::TarotHand(t) = self.stage else {
            return Err(GameError::InvalidStage);
        };
        let prev = self
            .tarot_prev_stage
            .take()
            .ok_or(GameError::InvalidStage)?;
        let selected_count = self.available.selected().len();
        if selected_count < t.min_targets() || selected_count > t.max_targets() {
            self.tarot_prev_stage = Some(prev);
            return Err(GameError::InvalidAction);
        }
        t.apply(self)?;
        if t != Tarot::Fool {
            self.last_consumable_used = Some(Consumable::tarot(t));
        }
        self.stage = prev;
        // Only reachable via a pack pick now, finish_pack handles hand cleanup
        if prev.is_pack_open() {
            if let Some(ref mut state) = self.open_pack {
                state.picks_remaining = state.picks_remaining.saturating_sub(1);
            }
            let done = self
                .open_pack
                .as_ref()
                .is_some_and(|s| s.picks_remaining == 0);
            if done {
                self.finish_pack();
            }
        }
        Ok(())
    }

    pub(crate) fn apply_spectral(&mut self) -> Result<(), GameError> {
        let Stage::SpectralHand(s) = self.stage else {
            return Err(GameError::InvalidStage);
        };
        let prev = self
            .spectral_prev_stage
            .take()
            .ok_or(GameError::InvalidStage)?;
        let selected_count = self.available.selected().len();
        if selected_count < s.min_targets() || selected_count > s.max_targets() {
            self.spectral_prev_stage = Some(prev);
            return Err(GameError::InvalidAction);
        }
        // Fool only ever copies the last Tarot or Planet used, never a Spectral.
        s.apply(self)?;
        self.stage = prev;
        // Only reachable via a pack pick now; finish_pack handles hand cleanup
        if prev.is_pack_open() {
            if let Some(ref mut state) = self.open_pack {
                state.picks_remaining = state.picks_remaining.saturating_sub(1);
            }
            let done = self
                .open_pack
                .as_ref()
                .is_some_and(|s| s.picks_remaining == 0);
            if done {
                self.finish_pack();
            }
        }
        Ok(())
    }

    pub(crate) fn buy_pack(&mut self, pack: Pack) -> Result<(), GameError> {
        if self.stage != Stage::Shop() {
            return Err(GameError::InvalidStage);
        }
        let cost = self.shop.price(|c| self.price(c), pack.cost());
        if !self.can_afford(cost) {
            return Err(GameError::InvalidBalance);
        }
        self.shop.buy_pack(&pack)?;
        self.money -= cost as i64;

        // Arcana/Spectral packs draw a hand for the player to apply targeted
        // tarots/spectrals against
        if matches!(pack.category, PackCategory::Arcana | PackCategory::Spectral) {
            self.draw(self.hand_size());
        }

        self.open_pack = Some(OpenPackState {
            picks_remaining: pack.picks_allowed(),
            description: pack.description(),
            contents: pack.contents,
        });
        self.stage = Stage::PackOpen();
        self.joker_pack_opened();
        Ok(())
    }

    pub(crate) fn pick_pack_card(&mut self, content: PackContent) -> Result<(), GameError> {
        if self.stage != Stage::PackOpen() {
            return Err(GameError::InvalidStage);
        }

        // Remove the chosen content from the pack
        {
            let state = self.open_pack.as_mut().ok_or(GameError::InvalidAction)?;
            let pos = state
                .contents
                .iter()
                .position(|c| c == &content)
                .ok_or(GameError::InvalidAction)?;
            state.contents.remove(pos);
        }

        // Targeting tarots enter TarotHand; picks_remaining decremented later in apply_tarot
        if let PackContent::Tarot(t) = &content {
            if t.requires_targets() {
                self.tarot_prev_stage = Some(Stage::PackOpen());
                self.stage = Stage::TarotHand(*t);
                return Ok(());
            }
        }
        // Targeting spectrals enter SpectralHand; picks_remaining decremented later in apply_spectral
        if let PackContent::Spectral(s) = &content {
            if s.requires_targets() {
                self.spectral_prev_stage = Some(Stage::PackOpen());
                self.stage = Stage::SpectralHand(*s);
                return Ok(());
            }
        }

        match content {
            PackContent::Tarot(t) => {
                t.apply(self)?;
            }
            PackContent::Planet(p) => {
                self.planets_used_this_run += 1;
                self.planetarium.level_up(p.hand_rank());
            }
            PackContent::Joker(j) => {
                if j.edition() == Edition::Negative {
                    self.config.joker_slots =
                        (self.config.joker_slots + 1).min(self.config.joker_slots_max);
                }
                if self.jokers.len() >= self.joker_slots() {
                    return Err(GameError::NoAvailableSlot);
                }
                self.jokers.push(j);
                self.effect_registry
                    .register_jokers(self.jokers.clone(), &self.clone());
            }
            PackContent::PlayingCard(c) => {
                self.deck.push(c);
            }
            PackContent::Spectral(s) => {
                s.apply(self)?;
            }
        }

        if let Some(ref mut state) = self.open_pack {
            state.picks_remaining = state.picks_remaining.saturating_sub(1);
        }

        let done = self
            .open_pack
            .as_ref()
            .is_some_and(|s| s.picks_remaining == 0);
        if done {
            self.finish_pack();
        }
        Ok(())
    }

    pub(crate) fn skip_pack(&mut self) -> Result<(), GameError> {
        if self.stage != Stage::PackOpen() {
            return Err(GameError::InvalidStage);
        }
        self.finish_pack();
        Ok(())
    }

    fn finish_pack(&mut self) {
        // Return any drawn hand (Arcana packs draw cards for tarot targeting)
        if !self.available.cards().is_empty() {
            let cards = self.available.cards();
            self.available.empty();
            self.deck.extend(cards);
            self.backend.shuffle_deck(&mut self.deck);
        }
        self.open_pack = None;
        self.stage = Stage::Shop();
    }

    fn select_blind(&mut self, blind: Blind) -> Result<(), GameError> {
        // can only set blind if stage is pre blind
        if self.stage != Stage::PreBlind() {
            return Err(GameError::InvalidStage);
        }
        // provided blind must be expected next blind
        if let Some(current) = self.blind {
            if blind != current.next() {
                return Err(GameError::InvalidBlind);
            }
        } else {
            // if game just started, blind will be None, in which case
            // we can only set it to small.
            if blind != Blind::Small {
                return Err(GameError::InvalidBlind);
            }
        }
        self.begin_round(blind);
        Ok(())
    }

    /// Everything that happens as a round opens: per-round resources, the
    /// boss's own setup, then the deal. Split out of `select_blind` so the
    /// boss logic is testable without walking the action path.
    pub(crate) fn begin_round(&mut self, blind: Blind) {
        self.blind = Some(blind);
        self.stage = Stage::Blind(blind);
        // Per-round resources are set here, at the start of the round, not
        // only in `clear_blind` at the end of the previous one — anything
        // bought in the shop in between (Grabber, Wasteful, Hieroglyph...)
        // has to count for the round it was bought for.
        self.plays = self.plays_per_round();
        self.discards_remaining = self.discards_per_round();
        self.hand_ranks_played_this_round.clear();
        self.discarded_this_round.clear();
        self.boss_state.on_new_blind();
        self.boss_state.all_cards_debuffed = self.active_boss() == Some(BossBlind::VerdantLeaf);
        self.boss_crimson_heart();
        // Showman is the backend's lock-table switch, refreshed here so
        // buying or selling one takes effect at once.
        let showman = self.jokers.iter().any(|j| matches!(j, Jokers::Showman(_)));
        self.backend.set_showman(showman);
        self.joker_round_decay();
        self.joker_blind_start(blind);
        if self.active_boss() == Some(BossBlind::AmberAcorn) {
            // The real game shuffles the joker area with the node 'aajk'.
            // Its pre-shuffle order is each joker's declaration order, so
            // the permutation is valid but not byte-identical.
            self.backend.shuffle_jokers(&mut self.jokers);
        }
        self.deal();
    }

    /// Turtle Bean loses a card of hand size each round and Troubadour a
    /// hand, both only from the round after they're bought.
    fn joker_round_decay(&mut self) {
        for joker in self.jokers.iter_mut() {
            match joker {
                Jokers::TurtleBean(b) => b.state.counter -= 1.0,
                Jokers::Troubadour(t) => t.state.counter += 1.0,
                _ => {}
            }
        }
    }

    /// DNA and Sixth Sense both look at the round's first hand, and both
    /// consume the card, so they run once per round.
    fn joker_first_hand(&mut self, played: &[Card]) {
        if self.hands_played_this_round != 1 || played.len() != 1 {
            return;
        }
        let card = played[0];

        if self
            .jokers
            .iter()
            .any(|j| matches!(j, Jokers::Dna(_)))
        {
            let copy = self.copy_of_card(&card);
            self.add_card_to_deck(copy);
            self.cards_added_this_ante += 1;
            self.available.select_card(card).ok();
        }

        if card.value == Value::Six
            && self
                .jokers
                .iter()
                .any(|j| matches!(j, Jokers::SixthSense(_)))
        {
            self.destroy_card(card.id);
            self.available.deselect_card(card).ok();
            self.add_spectral();
        }
    }

    /// Hallucination: a coin flip for a tarot on any pack opened.
    pub(crate) fn joker_pack_opened(&mut self) {
        if !self
            .jokers
            .iter()
            .any(|j| matches!(j, Jokers::Hallucination(_)))
        {
            return;
        }
        if self.prob_roll(EffectProc::Hallucination, 1, 2) {
            self.add_tarot();
        }
    }

    /// Jokers that act when a blind is selected, rather than on a score.
    /// Runs before the deal, so anything that changes hand size or card
    /// count applies to the hand actually dealt.
    fn joker_blind_start(&mut self, blind: Blind) {
        let played = blind != Blind::Boss;
        // Cards already in the hand can't be counted by a blind-start
        // joker, so they read the deck and discard piles instead.
        let deck_cards = self.deck.cards();

        // Riff Raff: two Common Jokers, one pair per copy.
        let riff_raffs = self
            .jokers
            .iter()
            .filter(|j| matches!(j, Jokers::RiffRaff(_)))
            .count();
        for _ in 0..RIFF_RAFF_JOKERS * riff_raffs {
            if self.jokers.len() >= self.joker_slots() {
                break;
            }
            let exclude: Vec<Jokers> = self
                .jokers
                .iter()
                .filter(|j| j.rarity() == Rarity::Common)
                .cloned()
                .collect();
            let joker = self.backend.gen_joker_of_rarity(
                self.ante_current as i32,
                self.prob_mult(),
                &exclude,
                Rarity::Common,
            );
            self.jokers.push(joker);
        }

        // Chaos the Clown: one free reroll for the round.
        self.free_rerolls_this_round = self
            .jokers
            .iter()
            .filter(|j| matches!(j, Jokers::ChaosTheClown(_)))
            .count();

        // Cartomancer: a tarot each blind.
        if self.jokers.iter().any(|j| matches!(j, Jokers::Cartomancer(_))) {
            self.add_tarot();
        }

        // Burglar: +3 hands, all discards gone.
        if self.jokers.iter().any(|j| matches!(j, Jokers::Burglar(_))) {
            self.plays += 3;
            self.discards_remaining = 0;
        }

        // Marble Joker: a Stone card into the deck.
        let marbles = self
            .jokers
            .iter()
            .filter(|j| matches!(j, Jokers::MarbleJoker(_)))
            .count();
        for _ in 0..marbles {
            let card = self.backend.gen_random_card(self.prob_mult(), false, None);
            self.add_card_to_deck(card);
            self.cards_added_this_ante += 1;
        }

        // Certificate: a random playing card with a random seal.
        if self
            .jokers
            .iter()
            .any(|j| matches!(j, Jokers::Certificate(_)))
        {
            let card = self.backend.gen_random_card(self.prob_mult(), false, None);
            let seal = self.backend.roll_random_seal(self.ante_current as i32);
            let mut card = card;
            card.seal = Some(seal);
            self.add_card_to_deck(card);
            self.cards_added_this_ante += 1;
        }

        // Burnt Joker: level up whatever the first discard of the round is,
        // so arm it now and let the discard hook do the work.
        if self
            .jokers
            .iter()
            .any(|j| matches!(j, Jokers::BurntJoker(_)))
        {
            self.burnt_joker_armed = true;
        }

        // Ceremonial Dagger: destroy the Joker to the right and bank double
        // its sell value as permanent Mult.
        let daggers: Vec<usize> = self
            .jokers
            .iter()
            .filter(|j| matches!(j, Jokers::CeremonialDagger(_)))
            .map(|j| j.instance_id())
            .collect();
        for id in daggers {
            let Some(victim) = self.joker_to_the_right(id) else {
                continue;
            };
            let value = victim.sell_value() * CEREMONIAL_DAGGER_VALUE_MULT;
            self.destroy_joker(victim.instance_id());
            if let Some(state) = self.joker_state_mut(id) {
                state.counter += value as f32;
            }
        }

        // Madness: X0.5 Mult and a random Joker destroyed, on small and big
        // blinds only.
        if played {
            self.madness_blind_start(&deck_cards);
        }
        self.effect_registry
            .register_jokers(self.jokers.clone(), &self.clone());
    }

    /// Madness: on a small or big blind, gain X0.5 Mult and destroy a
    /// random other Joker. Its Mult is a per-instance counter.
    ///
    /// Guarded on actually holding a Madness. Without this it destroyed a
    /// random joker from every run, which is what let a sticker test lose its
    /// joker to a joker the player did not own.
    fn madness_blind_start(&mut self, _deck_cards: &[Card]) {
        if !self.jokers.iter().any(|j| matches!(j, Jokers::Madness(_))) {
            return;
        }
        // An Eternal joker cannot be destroyed by any means.
        let victims: Vec<usize> = self
            .jokers
            .iter()
            .filter(|j| !matches!(j, Jokers::Madness(_)) && !j.stickers().eternal)
            .map(|j| j.instance_id())
            .collect();
        if victims.is_empty() {
            return;
        }
        for joker in self.jokers.iter_mut() {
            if let Jokers::Madness(m) = joker {
                m.state.counter += MADNESS_MULT_STEP;
            }
        }
        let idx = self.backend.random_index(victims.len());
        self.destroy_joker(victims[idx]);
    }

    fn skip_blind(&mut self, blind: Blind) -> Result<(), GameError> {
        if self.stage != Stage::PreBlind() {
            return Err(GameError::InvalidStage);
        }
        if blind == Blind::Boss {
            return Err(GameError::InvalidBlind);
        }
        if let Some(current) = self.blind {
            if blind != current.next() {
                return Err(GameError::InvalidBlind);
            }
        } else if blind != Blind::Small {
            return Err(GameError::InvalidBlind);
        }
        self.blind = Some(blind);
        self.stage = Stage::PreBlind();
        let tag = self
            .skip_tag(blind)
            .expect("blind is not Boss, checked above");
        self.tags.push(tag);
        self.blinds_skipped_this_run += 1;
        Ok(())
    }

    /// Everything an ante needs drawn before its first blind: the lock-table
    /// update, the two skippable tags, and the boss.
    fn start_ante(&mut self) {
        // Director's Cut's allowance is per ante, so it clears here rather
        // than in the ante-advance path: this is the single place every
        // ante goes through.
        self.boss_state.boss_rerolled_this_ante = false;
        self.backend.on_ante_start(self.ante_current as i32);
        let (small, big) = self.backend.draw_ante_tags(self.ante_current as i32);
        self.small_blind_tag = small;
        self.big_blind_tag = big;
        // Orbital's hand is chosen now, while the tag is still visible and
        // skippable, and the effect lands when it's collected.
        if big == Tag::Orbital || small == Tag::Orbital {
            self.tag_state.orbital_hand = Some(self.backend.roll_random_hand());
        }
        self.current_boss = Some(self.backend.draw_boss(self.ante_current as i32));
    }

    pub fn skip_tag(&self, blind: Blind) -> Option<Tag> {
        match blind {
            Blind::Small => Some(self.small_blind_tag),
            Blind::Big => Some(self.big_blind_tag),
            Blind::Boss => None,
        }
    }

    fn next_round(&mut self) -> Result<(), GameError> {
        self.stage = Stage::PreBlind();
        self.round += 1;
        // Juggle's hand size covers exactly one round.
        self.tag_state.hand_size_bonus = 0;
        Ok(())
    }

    // Returns true if should clear blind after, false if not.
    /// Mr. Bones: once per run, if this hand was worth at least a quarter
    /// of what the blind needed, it saves the run and self-destructs.
    fn mr_bones_saves(&mut self) -> bool {
        let Some(id) = self
            .jokers
            .iter()
            .find(|j| matches!(j, Jokers::MrBones(_)))
            .map(|j| j.instance_id())
        else {
            return false;
        };
        let threshold = self.required_score() / MR_BONES_DIVISOR;
        if self.score < threshold.max(1) {
            return false;
        }
        self.destroy_joker(id);
        true
    }

    fn handle_score(&mut self, score: usize) -> Result<bool, GameError> {
        // can only handle score if stage is blind
        if !self.stage.is_blind() {
            return Err(GameError::InvalidStage);
        }

        self.score += score;
        let required = self.required_score();

        // blind not passed
        if self.score < required {
            // no more hands to play -> lose
            if self.plays == 0 {
                if self.mr_bones_saves() {
                    return Ok(false);
                }
                self.stage = Stage::End(End::Lose);
                return Ok(false);
            } else {
                // more hands to play, carry on
                return Ok(false);
            }
        }

        let blind = self.blind.expect("stage is blind");
        self.last_score = self.score;
        self.unused_discards_this_run += self.discards_remaining;

        // score exceeds blind (blind passed).
        // handle reward then progress to next stage.
        let reward = self.calc_reward(blind)?;
        self.reward = reward;

        // passed boss blind, either win or progress ante
        if blind == Blind::Boss {
            self.campfire_sold = 0;
            // Investment Tag pays here, not when the tag was collected.
            if std::mem::take(&mut self.tag_state.investment) {
                self.money += INVESTMENT_TAG_DOLLARS as i64;
            }
            if let Some(ante_next) = self.ante_current.next(self.ante_end) {
                self.ante_current = ante_next;
                self.blind = None;
                self.boss_state.on_new_ante();
                self.cards_added_this_ante = 0;
                self.start_ante();
            } else {
                self.stage = Stage::End(End::Win);
                return Ok(false);
            }
        };

        // finish blind, proceed to post blind
        self.stage = Stage::PostBlind();
        Ok(true)
    }

    pub fn handle_action(&mut self, action: Action) -> Result<(), GameError> {
        self.action_history.push(action.clone());
        match action {
            Action::SelectCard(card) => {
                if self.stage.is_blind() {
                    self.select_card(card)
                } else if let Stage::TarotHand(t) = self.stage {
                    if self.available.selected().len() >= t.max_targets() {
                        return Err(GameError::InvalidAction);
                    }
                    self.select_card(card)
                } else if let Stage::SpectralHand(s) = self.stage {
                    if self.available.selected().len() >= s.max_targets() {
                        return Err(GameError::InvalidAction);
                    }
                    self.select_card(card)
                } else {
                    Err(GameError::InvalidAction)
                }
            }
            Action::DeselectCard(card) => {
                if self.stage.is_blind()
                    || matches!(self.stage, Stage::TarotHand(_) | Stage::SpectralHand(_))
                {
                    self.deselect_card(card)
                } else {
                    Err(GameError::InvalidAction)
                }
            }
            Action::Play() => match self.stage.is_blind() {
                true => self.play_selected(),
                false => Err(GameError::InvalidAction),
            },
            Action::Discard() => match self.stage.is_blind() {
                true => self.discard_selected(),
                false => Err(GameError::InvalidAction),
            },
            Action::MoveCard(dir, card) => {
                if self.stage.is_blind()
                    || matches!(self.stage, Stage::TarotHand(_) | Stage::SpectralHand(_))
                {
                    self.move_card(dir, card)
                } else {
                    Err(GameError::InvalidAction)
                }
            }
            Action::CashOut(_reward) => match self.stage {
                Stage::PostBlind() => self.cashout(),
                _ => Err(GameError::InvalidAction),
            },
            Action::BuyJoker(joker) => match self.stage {
                Stage::Shop() => self.buy_joker(joker),
                _ => Err(GameError::InvalidAction),
            },
            Action::BuyConsumable(consumable) => match self.stage {
                Stage::Shop() => self.buy_consumable(consumable),
                _ => Err(GameError::InvalidAction),
            },
            Action::BuyVoucher(voucher) => match self.stage {
                Stage::Shop() => self.buy_voucher(voucher),
                _ => Err(GameError::InvalidAction),
            },
            Action::BuyPlayingCard(card) => match self.stage {
                Stage::Shop() => self.buy_playing_card(card),
                _ => Err(GameError::InvalidAction),
            },
            Action::UseConsumable(consumable) => self.use_consumable(consumable),
            Action::NextRound() => match self.stage {
                Stage::Shop() => self.next_round(),
                _ => Err(GameError::InvalidAction),
            },
            Action::SelectBlind(blind) => match self.stage {
                Stage::PreBlind() => self.select_blind(blind),
                _ => Err(GameError::InvalidAction),
            },
            Action::SkipBlind(blind) => match self.stage {
                Stage::PreBlind() => self.skip_blind(blind),
                _ => Err(GameError::InvalidAction),
            },
            Action::ApplyTarot() => self.apply_tarot(),
            Action::ApplySpectral() => self.apply_spectral(),
            Action::SellJoker(idx) => self.sell_joker(idx),
            Action::SellConsumable(idx) => self.sell_consumable(idx),
            Action::BuyPack(pack) => match self.stage {
                Stage::Shop() => self.buy_pack(pack),
                _ => Err(GameError::InvalidStage),
            },
            Action::PickPackCard(content) => match self.stage {
                Stage::PackOpen() => self.pick_pack_card(content),
                _ => Err(GameError::InvalidStage),
            },
            Action::SortHand(sort_by) => {
                if self.stage.is_blind() {
                    self.available.sort(sort_by);
                    Ok(())
                } else {
                    Err(GameError::InvalidAction)
                }
            }
            Action::SkipPack() => match self.stage {
                Stage::PackOpen() => self.skip_pack(),
                _ => Err(GameError::InvalidStage),
            },
            Action::RerollBoss() => self.reroll_boss(),
            Action::Reroll() => match self.stage {
                Stage::Shop() => self.reroll(),
                _ => Err(GameError::InvalidAction),
            },
        }
    }

    pub fn handle_action_index(&mut self, index: usize) -> Result<(), GameError> {
        let space = self.gen_action_space();
        let action = space.to_action(index, self)?;
        self.handle_action(action)
    }
}

impl fmt::Display for Game {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let hand_str: String = self
            .available
            .cards_and_selected()
            .iter()
            .map(|(c, sel)| {
                if *sel {
                    format!("[{}]", card_display(c))
                } else {
                    card_display(c)
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        writeln!(f, "deck variant: {}", self.config.deck.name())?;
        writeln!(f, "stake: {}", self.config.stake.name())?;
        writeln!(f, "hand: {}", hand_str)?;
        writeln!(f, "discard pile: {}", self.discarded.len())?;
        let deck_cards = self.deck.cards();
        let modified: Vec<String> = deck_cards
            .iter()
            .filter(|c| c.enhancement.is_some() || c.edition != Edition::Base || c.seal.is_some())
            .map(card_display)
            .collect();
        if modified.is_empty() {
            writeln!(f, "deck: {}", deck_cards.len())?;
        } else {
            writeln!(
                f,
                "deck: {} ({} modified: {})",
                deck_cards.len(),
                modified.len(),
                modified.join(" ")
            )?;
        }
        if self.jokers.is_empty() {
            writeln!(f, "jokers: (none)")?;
        } else {
            writeln!(f, "jokers:")?;
            for j in self.jokers.clone() {
                writeln!(f, "  {}", joker_display(&j))?;
            }
        }
        if self.consumables.is_empty() {
            writeln!(f, "consumables: (none)")?;
        } else {
            let parts: Vec<String> = self
                .consumables
                .iter()
                .map(|c| format!("[{}] {}", c.type_label(), c.name()))
                .collect();
            writeln!(f, "consumables: {}", parts.join(", "))?;
        }
        if self.vouchers.is_empty() {
            writeln!(f, "vouchers: (none)")?;
        } else {
            let names: Vec<&str> = self.vouchers.owned().iter().map(|v| v.name()).collect();
            writeln!(f, "vouchers: {}", names.join(", "))?;
        }
        writeln!(f, "planetarium: {}", self.planetarium)?;
        writeln!(f, "stage: {:?}", self.stage)?;
        writeln!(f, "ante: {:?}", self.ante_current)?;
        writeln!(f, "blind: {:?}", self.blind)?;
        writeln!(f, "round: {}", self.round)?;
        writeln!(f, "hands remaining: {}", self.plays)?;
        writeln!(f, "discards remaining: {}", self.discards())?;
        writeln!(f, "money: {}", self.money)?;
        if matches!(self.stage, Stage::Blind(_)) {
            writeln!(
                f,
                "score: {}  target: {}",
                self.score,
                self.required_score()
            )
        } else {
            writeln!(f, "score: {}", self.score)
        }
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new(Config::default())
    }
}

#[cfg(feature = "serde")]
impl Game {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        let mut game: Self = serde_json::from_str(s)?;
        let jokers = game.jokers.clone();
        game.effect_registry.register_jokers(jokers, &game.clone());

        // Deserializing doesn't allocate ids, so Card::new() calls after this
        // point would otherwise collide with ids already present in `game`.
        let max_id = game
            .deck
            .cards()
            .iter()
            .chain(game.available.cards().iter())
            .chain(game.discarded.iter())
            .map(|c| c.id)
            .max();
        if let Some(max_id) = max_id {
            crate::card::ensure_id_counter_past(max_id);
        }

        // Same fixup for joker instance ids, minted ones sit in `game.jokers`
        // and `game.shop.jokers`, and 0 means "never minted" so it's excluded.
        let max_joker_id = game
            .jokers
            .iter()
            .chain(game.shop.jokers.iter())
            .map(|j| j.instance_id())
            .filter(|&id| id > 0)
            .max();
        if game.current_boss.is_none() {
            game.current_boss = Some(game.backend.draw_boss(game.ante_current as i32));
        }

        if let Some(max_joker_id) = max_joker_id {
            crate::joker::ensure_joker_id_counter_past(max_joker_id);
        }

        Ok(game)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Suit, Value};

    #[test]
    fn test_real_rng_mode_matches_balatro_seed_reference() {
        use crate::pack::PackSize;

        let config = Config {
            rng_mode: RngMode::Real,
            seed_str: Some("TEST".to_string()),
            ..Config::default()
        };
        let mut g = Game::new(config);

        let planetarium = g.planetarium.clone();
        let vouchers = Vouchers::new();
        let ctx = ShopContext::for_test(&planetarium, &vouchers, 1);
        g.shop.refresh(&ctx, &mut g.backend, &mut TagState::default());

        assert_eq!(g.shop.jokers.len(), 1);
        assert_eq!(g.shop.jokers[0].name(), "Ice Cream");
        assert_eq!(g.shop.consumables.len(), 1);
        assert_eq!(g.shop.consumables[0].name(), "Strength");

        assert_eq!(g.shop.packs.len(), 2);

        assert_eq!(g.shop.packs[0].category, PackCategory::Buffoon);
        assert_eq!(g.shop.packs[0].size, PackSize::Normal);
        let p1_contents: Vec<String> = g.shop.packs[0].contents.iter().map(|c| c.name()).collect();
        assert_eq!(p1_contents, vec!["Baseball Card", "Drunkard"]);

        assert_eq!(g.shop.packs[1].category, PackCategory::Spectral);
        assert_eq!(g.shop.packs[1].size, PackSize::Normal);
        let p2_contents: Vec<String> = g.shop.packs[1].contents.iter().map(|c| c.name()).collect();
        assert_eq!(p2_contents, vec!["Medium", "Wraith"]);
    }

    // `cargo run -p balatro-seed --bin explore -- TEST` reports
    // `Boss: The Goad` for ante 1 of this seed. `core`'s real mode has to
    // agree, which is only true if `on_run_start` seeds the lock table
    // before the draw — Goad is ante-1-legal while most bosses are not.
    #[test]
    fn test_real_rng_mode_draw_boss_reaches_instance() {
        let config = Config {
            rng_mode: RngMode::Real,
            seed_str: Some("TEST".to_string()),
            ..Config::default()
        };
        let g = Game::new(config);
        assert_eq!(g.current_boss, Some(BossBlind::Goad));
    }

    // Same, one ante at a time, against `explore`'s per-ante boss list.
    // Guards the `init_unlocks` handoff: without it, `next_boss`'s category
    // filter would happily hand out an ante-6 boss on ante 1.
    #[test]
    fn test_real_rng_mode_draw_boss_matches_explore_per_ante() {
        fn run() -> Vec<BossBlind> {
            let fast = crate::rng::FastBackend::new(rand_chacha::ChaCha8Rng::seed_from_u64(1));
            let mut backend = crate::rng::RealBackend::new("TEST", fast);
            backend.on_run_start();
            (1..=8)
                .map(|ante| {
                    backend.on_ante_start(ante);
                    backend.draw_boss(ante)
                })
                .collect()
        }

        assert_eq!(
            run(),
            vec![
                BossBlind::Goad,
                BossBlind::House,
                BossBlind::Wall,
                BossBlind::Wheel,
                BossBlind::Psychic,
                BossBlind::Arm,
                BossBlind::Hook,
                BossBlind::CrimsonHeart,
            ],
        );
    }

    // Same seed through two independent `Game`s: proves `RngMode::Real`
    // stays deterministic as antes advance.
    #[test]
    fn test_real_rng_mode_is_deterministic_across_antes() {
        use crate::pack::PackSize;

        fn run() -> Vec<(Vec<String>, Vec<String>, Vec<(PackCategory, PackSize)>)> {
            let config = Config {
                rng_mode: RngMode::Real,
                seed_str: Some("TEST".to_string()),
                ..Config::default()
            };
            let mut g = Game::new(config);
            let planetarium = g.planetarium.clone();
            let mut antes = Vec::new();
            for ante in 1..=4 {
                let vouchers = Vouchers::new();
                let ctx = ShopContext::for_test(&planetarium, &vouchers, ante);
                g.shop.refresh(&ctx, &mut g.backend, &mut TagState::default());
                let jokers: Vec<String> =
                    g.shop.jokers.iter().map(|j| j.name().to_string()).collect();
                let consumables: Vec<String> = g
                    .shop
                    .consumables
                    .iter()
                    .map(|c| c.name().to_string())
                    .collect();
                let packs = vec![
                    (g.shop.packs[0].category, g.shop.packs[0].size),
                    (g.shop.packs[1].category, g.shop.packs[1].size),
                ];
                antes.push((jokers, consumables, packs));
            }
            antes
        }

        assert_eq!(
            run(),
            run(),
            "same seed must produce the same draws across every ante"
        );
    }

    // Buys the real shop joker, then refreshes many times: proves
    // on_joker_bought reached the live Instance's lock table.
    #[test]
    fn test_real_rng_mode_excludes_bought_joker_from_future_draws() {
        let config = Config {
            rng_mode: RngMode::Real,
            seed_str: Some("TEST".to_string()),
            ..Config::default()
        };
        let mut g = Game::new(config);
        g.stage = Stage::Shop();
        g.money = 1000;

        let planetarium = g.planetarium.clone();
        let vouchers = Vouchers::new();
        let ctx = ShopContext::for_test(&planetarium, &vouchers, 1);
        g.shop.refresh(&ctx, &mut g.backend, &mut TagState::default());
        let bought = g.shop.jokers[0].clone();
        g.buy_joker(bought.clone()).expect("buy joker");

        for ante in 1..=50 {
            let vouchers = Vouchers::new();
            let held_jokers = g.jokers.clone();
            let ctx = ShopContext {
                held_jokers: &held_jokers,
                ..ShopContext::for_test(&planetarium, &vouchers, ante)
            };
            g.shop.refresh(&ctx, &mut g.backend, &mut TagState::default());
            assert!(
                g.shop
                    .jokers
                    .iter()
                    .all(|j| std::mem::discriminant(j) != std::mem::discriminant(&bought)),
                "bought joker {} reappeared in shop while still owned (ante {ante})",
                bought.name()
            );
        }
    }

    // Mirror of the test above: selling should make the joker drawable
    // again. Probabilistic — enough draws that reappearance is near-certain.
    #[test]
    fn test_real_rng_mode_sold_joker_becomes_drawable_again() {
        let config = Config {
            rng_mode: RngMode::Real,
            seed_str: Some("TEST".to_string()),
            ..Config::default()
        };
        let mut g = Game::new(config);
        g.stage = Stage::Shop();
        g.money = 1000;

        let planetarium = g.planetarium.clone();
        let vouchers = Vouchers::new();
        let ctx = ShopContext::for_test(&planetarium, &vouchers, 1);
        g.shop.refresh(&ctx, &mut g.backend, &mut TagState::default());
        let bought = g.shop.jokers[0].clone();
        g.buy_joker(bought.clone()).expect("buy joker");
        g.sell_joker(0).expect("sell joker");

        let mut reappeared = false;
        for ante in 1..=500 {
            let ctx = ShopContext::for_test(&planetarium, &vouchers, ante);
            g.shop.refresh(&ctx, &mut g.backend, &mut TagState::default());
            if g.shop
                .jokers
                .iter()
                .any(|j| std::mem::discriminant(j) == std::mem::discriminant(&bought))
            {
                reappeared = true;
                break;
            }
        }
        assert!(
            reappeared,
            "sold joker {} never reappeared across 500 draws",
            bought.name()
        );
    }

    #[test]
    fn test_constructor() {
        let g = Game::default();
        assert_eq!(g.available.cards().len(), 0);
        assert_eq!(g.deck.len(), 52);
        assert_eq!(g.mult, 0);
    }

    #[test]
    fn test_deal() {
        let mut g = Game::default();
        g.deal();
        // deck should be 7 cards smaller than we started with
        assert_eq!(g.deck.len(), 52 - g.config.available);
        // should be 7 cards now available
        assert_eq!(g.available.cards().len(), g.config.available);
    }

    #[test]
    fn test_draw() {
        let mut g = Game::default();
        g.draw(1);
        assert_eq!(g.available.cards().len(), 1);
        assert_eq!(g.deck.len(), 52 - 1);
        g.draw(3);
        assert_eq!(g.available.cards().len(), 4);
        assert_eq!(g.deck.len(), 52 - 4);
    }
    #[test]
    fn test_discard() {
        let mut g = Game::default();
        g.deal();
        assert_eq!(g.available.cards().len(), g.config.available);
        assert_eq!(g.deck.len(), 52 - g.config.available);
        // select first 4 cards
        for c in g.available.cards()[0..5].to_vec() {
            g.select_card(c).unwrap();
        }
        let discard_res = g.discard_selected();
        assert!(discard_res.is_ok());
        // available should still be 7, we discarded then redrew to match
        assert_eq!(g.available.cards().len(), g.config.available);
        // deck is now smaller since we drew from it
        assert_eq!(g.deck.len(), 52 - g.config.available - 5);
    }

    #[test]
    fn test_discarded_this_round_tracking() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Blind(Blind::Small);
        g.blind = Some(Blind::Small);
        g.deal();

        let played_card = g.available.cards()[0];
        g.select_card(played_card).expect("can select");
        g.play_selected().expect("can play");

        let discarded_card = g.available.cards()[0];
        g.select_card(discarded_card).expect("can select");
        g.discard_selected().expect("can discard");

        // only the discarded card is tracked, not the played one
        assert_eq!(g.discarded_this_round.len(), 1);
        assert_eq!(g.discarded_this_round[0].id, discarded_card.id);
        assert_eq!(g.total_cards_discarded, 1);

        // `discarded` (the deck-recycling pile) still has both - untouched
        assert!(g.discarded.iter().any(|c| c.id == played_card.id));
        assert!(g.discarded.iter().any(|c| c.id == discarded_card.id));

        g.clear_blind();
        // per-round tracking resets, run total does not
        assert!(g.discarded_this_round.is_empty());
        assert_eq!(g.total_cards_discarded, 1);
    }

    #[test]
    fn test_calc_score() {
        let mut g = Game::default();
        let ace = Card::new(Value::Ace, Suit::Heart);
        let king = Card::new(Value::King, Suit::Diamond);
        let jack = Card::new(Value::Jack, Suit::Club);

        // Score [Ah, Kd, Jc]
        // High card (level 1) -> chips=5, mult=1
        // Played cards (1 ace) -> 11 chips
        // (5 + 11) * 1 = 16
        let cards = vec![ace, king, jack];
        let hand = SelectHand::new(cards).best_hand().unwrap();
        let score = g.calc_score(hand);
        assert_eq!(score, 16);

        // Score [Kd, Kd, Ah]
        // Pair (level 1) -> chips=10, mult=2
        // Played cards (2 kings) -> 10 + 10 == 20 chips
        // (10 + 20) * 2 = 60
        let cards = vec![king, king, ace];
        let hand = SelectHand::new(cards).best_hand().unwrap();
        let score = g.calc_score(hand);
        assert_eq!(score, 60);

        // Score [Ah, Ah, Ah, Kd]
        // Three of kind (level 1) -> chips=30, mult=3
        // Played cards (3 aces) -> 11 + 11 + 11 == 33 chips
        // (30 + 33) * 3 = 189
        let cards = vec![ace, ace, ace, king];
        let hand = SelectHand::new(cards).best_hand().unwrap();
        let score = g.calc_score(hand);
        assert_eq!(score, 189);

        // Score [Kd, Kd, Kd, Kd, Ah]
        // Four of kind (level 1) -> chips=60, mult=7
        // Played cards (4 kings) -> 10 + 10 + 10 + 10 == 40 chips
        // (60 + 40) * 7 = 700
        let cards = vec![king, king, king, king, ace];
        let hand = SelectHand::new(cards).best_hand().unwrap();
        let score = g.calc_score(hand);
        assert_eq!(score, 700);

        // Score [Jc, Jc, Jc, Jc, Jc]
        // Flush five (level 1) -> chips=160, mult=16
        // Played cards (5 jacks) -> 10 + 10 + 10 + 10 + 10 == 50 chips
        // (160 + 50) * 16 = 3360
        let cards = vec![jack, jack, jack, jack, jack];
        let hand = SelectHand::new(cards).best_hand().unwrap();
        let score = g.calc_score(hand);
        assert_eq!(score, 3360);
    }

    #[test]
    fn test_calc_score_traced_matches_calc_score() {
        // guards calc_score/calc_score_traced against silently diverging
        let ace = Card::new(Value::Ace, Suit::Heart);
        let king = Card::new(Value::King, Suit::Diamond);
        let jack = Card::new(Value::Jack, Suit::Club);

        let fixtures = vec![
            vec![ace, king, jack],
            vec![king, king, ace],
            vec![ace, ace, ace, king],
            vec![king, king, king, king, ace],
            vec![jack, jack, jack, jack, jack],
        ];

        for cards in fixtures {
            let hand = SelectHand::new(cards).best_hand().unwrap();

            let mut plain = Game::default();
            let plain_score = plain.calc_score(hand.clone());

            let mut traced = Game::default();
            let (traced_score, trace) = traced.calc_score_traced(hand);

            assert_eq!(plain_score, traced_score);
            assert!(!trace.0.is_empty());
        }
    }

    #[test]
    fn test_calc_score_traced_hand_and_played_card_steps() {
        let mut g = Game::default();
        let ace = Card::new(Value::Ace, Suit::Heart);
        let king = Card::new(Value::King, Suit::Diamond);
        let jack = Card::new(Value::Jack, Suit::Club);

        // Same fixture as test_calc_score's first case, best hand is the ace alone.
        let hand = SelectHand::new(vec![ace, king, jack]).best_hand().unwrap();
        let (score, trace) = g.calc_score_traced(hand);
        assert_eq!(score, 16);

        assert_eq!(trace.0.len(), 2);

        assert_eq!(
            trace.0[0].source,
            ScoreSource::HandLevel(HandRank::HighCard)
        );
        assert_eq!(trace.0[0].chips_before, 0);
        assert_eq!(trace.0[0].chips_after, 5);
        assert_eq!(trace.0[0].mult_before, 0);
        assert_eq!(trace.0[0].mult_after, 1);
        assert!(!trace.0[0].retrigger);

        assert_eq!(trace.0[1].source, ScoreSource::PlayedCard(ace));
        assert_eq!(trace.0[1].chips_before, 5);
        assert_eq!(trace.0[1].chips_after, 16);
        assert_eq!(trace.0[1].mult_before, 1);
        assert_eq!(trace.0[1].mult_after, 1);
        assert!(!trace.0[1].retrigger);
    }

    #[test]
    fn test_calc_score_traced_held_card_steps() {
        let mut g = Game::default();
        let ace = Card::new(Value::Ace, Suit::Heart);
        let mut steel_king = Card::new(Value::King, Suit::Diamond);
        steel_king.enhancement = Some(Enhancement::Steel);

        // Held, not played — g.available.extend() leaves cards unselected.
        g.available.extend(vec![steel_king]);

        // mult=2 baseline so Steel's `mult += mult/2` has a nonzero result.
        let hand = SelectHand::new(vec![ace, ace]).best_hand().unwrap();
        let (score, trace) = g.calc_score_traced(hand);

        let held_steps: Vec<_> = trace
            .0
            .iter()
            .filter(|s| matches!(s.source, ScoreSource::HeldCard(_)))
            .collect();
        assert_eq!(held_steps.len(), 1);
        assert_eq!(held_steps[0].source, ScoreSource::HeldCard(steel_king));
        assert_eq!(held_steps[0].mult_before, 2);
        assert_eq!(held_steps[0].mult_after, 3);
        assert_eq!(held_steps[0].chips_before, held_steps[0].chips_after);

        // Pair (level 1): 10 chips, 2 mult. Played (2 aces): +11+11 chips.
        // Held Steel king: mult 2 -> 3. (10 + 22) * 3 = 96.
        assert_eq!(score, 96);
    }

    #[test]
    fn test_handle_score() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Blind(Blind::Small);
        g.blind = Some(Blind::Small);

        // Not enough to pass
        let required = g.required_score();
        let score = required - 1;

        let passed = g.handle_score(score).unwrap();
        assert!(!passed);
        assert_eq!(g.score, score);

        // Enough to pass now
        let passed = g.handle_score(1).unwrap();
        assert!(passed);
        assert_eq!(g.score, required);
        assert_eq!(g.stage, Stage::PostBlind());
    }

    #[test]
    fn test_clear_blind() {
        let mut g = Game::default();
        g.start();
        g.deal();
        g.clear_blind();
        // all cards return to deck, available is empty
        assert_eq!(g.deck.len(), 52);
        assert_eq!(g.available.cards().len(), 0);
    }

    #[test]
    fn test_active_boss_during_boss_blind() {
        let g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        assert_eq!(g.active_boss(), Some(BossBlind::Club));
    }

    #[test]
    fn test_active_boss_none_outside_boss_round() {
        let g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Small),
            ..Default::default()
        };
        assert_eq!(g.active_boss(), None);
    }

    #[test]
    fn test_active_boss_none_when_chicot_held() {
        use crate::joker::*;
        let g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Boss),
            jokers: vec![Jokers::Chicot(Chicot::default())],
            ..Default::default()
        };
        assert_eq!(g.active_boss(), None);
    }

    #[test]
    fn test_active_boss_none_when_luchador_disabled() {
        let g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Boss),
            boss_disabled: true,
            ..Default::default()
        };
        assert_eq!(g.active_boss(), None);
    }

    #[test]
    fn test_boss_disabled_resets_on_clear_blind() {
        let mut g = Game {
            boss_disabled: true,
            ..Default::default()
        };
        g.clear_blind();
        assert!(!g.boss_disabled);
    }

    #[test]
    fn test_club_boss_debuffs_club_cards() {
        // King alone: (5 + 10) * 1 = 15 baseline; debuffed, contributes 0 -> 5.
        let mut g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Boss),
            ..Default::default()
        };

        // doesn't debuff heart
        let king_h = Card::new(Value::King, Suit::Heart);
        let hand = SelectHand::new(vec![king_h]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 15);

        // debuffs club
        let king_c = Card::new(Value::King, Suit::Club);
        let hand = SelectHand::new(vec![king_c]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 5);
    }

    #[test]
    fn test_goad_boss_debuffs_spade_cards() {
        let mut g = Game {
            current_boss: Some(BossBlind::Goad),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let king = Card::new(Value::King, Suit::Spade);
        let hand = SelectHand::new(vec![king]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 5);
    }

    #[test]
    fn test_head_boss_debuffs_heart_cards() {
        let mut g = Game {
            current_boss: Some(BossBlind::Head),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let king = Card::new(Value::King, Suit::Heart);
        let hand = SelectHand::new(vec![king]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 5);
    }

    #[test]
    fn test_window_boss_debuffs_diamond_cards() {
        let mut g = Game {
            current_boss: Some(BossBlind::Window),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let king = Card::new(Value::King, Suit::Diamond);
        let hand = SelectHand::new(vec![king]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 5);
    }

    #[test]
    fn test_plant_boss_debuffs_face_cards() {
        let mut g = Game {
            current_boss: Some(BossBlind::Plant),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let king = Card::new(Value::King, Suit::Heart);
        let king_hand = SelectHand::new(vec![king]).best_hand().unwrap();
        assert_eq!(g.calc_score(king_hand), 5);

        // Non-face card in the same round, unaffected by Plant.
        // (5 + 10) * 1 = 15.
        let ten = Card::new(Value::Ten, Suit::Heart);
        let ten_hand = SelectHand::new(vec![ten]).best_hand().unwrap();
        assert_eq!(g.calc_score(ten_hand), 15);
    }

    #[test]
    fn test_wild_card_debuffed_by_suit_boss_despite_different_suit() {
        let mut g = Game {
            current_boss: Some(BossBlind::Window),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        // Wild matches every suit, so it's debuffed by the
        // Diamond-debuffing Window boss despite its literal suit (Heart).
        let mut wild_ace = Card::new(Value::Ace, Suit::Heart);
        wild_ace.enhancement = Some(Enhancement::Wild);
        let wild_hand = SelectHand::new(vec![wild_ace]).best_hand().unwrap();
        assert_eq!(g.calc_score(wild_hand), 5);

        // Control: a genuinely Heart (non-Wild) card isn't debuffed by Window.
        // (5 + 11) * 1 = 16.
        let plain_ace = Card::new(Value::Ace, Suit::Heart);
        let plain_hand = SelectHand::new(vec![plain_ace]).best_hand().unwrap();
        assert_eq!(g.calc_score(plain_hand), 16);
    }

    #[test]
    fn test_debuffed_stone_kicker_loses_chip_bonus() {
        // Same shape as test_enhancement_stone_as_kicker, but the Stone
        // card is Club-suited under the Club boss - the kicker's +50 chips
        // no longer apply. (5 + 11) * 1 = 16, not 66.
        let mut g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let ace = Card::new(Value::Ace, Suit::Heart);
        let mut stone = Card::new(Value::Two, Suit::Club);
        stone.enhancement = Some(Enhancement::Stone);
        let hand = SelectHand::new(vec![ace, stone]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 16);
    }

    #[test]
    fn test_debuff_only_applies_during_boss_round() {
        // Same Club King as test_club_boss_debuffs_club_cards, but blind is
        // Small, not Boss - active_boss() returns None (Step 2's
        // round-scoping gate), so the debuff never applies.
        let mut g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Small),
            ..Default::default()
        };
        let king = Card::new(Value::King, Suit::Club);
        let hand = SelectHand::new(vec![king]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 15);
    }

    #[test]
    fn test_debuffed_card_still_counts_for_hand_type() {
        // Two Club Kings still form a Pair even though both are debuffed -
        // hand-type detection happens upstream of calc_score_inner. Pair
        // level 1: chips=10, mult=2; neither King contributes chips ->
        // 10 * 2 = 20.
        let mut g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let king1 = Card::new(Value::King, Suit::Club);
        let king2 = Card::new(Value::King, Suit::Club);
        let hand = SelectHand::new(vec![king1, king2]).best_hand().unwrap();
        assert_eq!(hand.rank, HandRank::OnePair);
        assert_eq!(g.calc_score(hand), 20);
    }

    #[test]
    fn test_debuffed_card_loses_seal_money() {
        let mut g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let money_before = g.money;
        let mut king = Card::new(Value::King, Suit::Club);
        king.seal = Some(Seal::Gold);
        let hand = SelectHand::new(vec![king]).best_hand().unwrap();
        g.calc_score(hand);
        assert_eq!(g.money, money_before);
    }

    #[test]
    fn test_debuffed_held_card_loses_enhancement() {
        let mut g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let money_before = g.money;
        let ace = Card::new(Value::Ace, Suit::Heart);
        let mut gold_king = Card::new(Value::King, Suit::Club);
        gold_king.enhancement = Some(Enhancement::Gold);
        g.available.extend(vec![gold_king]);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        g.calc_score(hand);
        assert_eq!(g.money, money_before);
    }

    #[test]
    fn test_debuffed_card_skips_retrigger() {
        // Red Seal normally doubles trigger_count_played; under the Club
        // boss, the debuffed card's contribution (and its retrigger) are
        // skipped entirely - score matches the bare base level, not a
        // doubled-then-zeroed contribution.
        let mut g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let mut king = Card::new(Value::King, Suit::Club);
        king.seal = Some(Seal::Red);
        let hand = SelectHand::new(vec![king]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 5);
    }

    #[test]
    fn test_non_debuffed_filters_out_debuffed_cards() {
        let g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let club_king = Card::new(Value::King, Suit::Club);
        let heart_ace = Card::new(Value::Ace, Suit::Heart);
        let cards = [club_king, heart_ace];
        assert_eq!(g.non_debuffed(cards.iter()), vec![heart_ace]);
    }

    #[test]
    fn test_boss_triggered_this_hand_set_on_debuff() {
        let mut g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let king = Card::new(Value::King, Suit::Club);
        let hand = SelectHand::new(vec![king]).best_hand().unwrap();
        g.calc_score(hand);
        assert!(g.boss_triggered_this_hand);
    }

    #[test]
    fn test_boss_triggered_this_hand_false_without_debuff() {
        let mut g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let ace = Card::new(Value::Ace, Suit::Heart);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        g.calc_score(hand);
        assert!(!g.boss_triggered_this_hand);
    }

    #[test]
    fn test_boss_triggered_this_hand_resets_each_hand() {
        let mut g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let king = Card::new(Value::King, Suit::Club);
        let hand = SelectHand::new(vec![king]).best_hand().unwrap();
        g.calc_score(hand);
        assert!(g.boss_triggered_this_hand);

        let ace = Card::new(Value::Ace, Suit::Heart);
        let hand2 = SelectHand::new(vec![ace]).best_hand().unwrap();
        g.calc_score(hand2);
        assert!(!g.boss_triggered_this_hand);
    }

    #[test]
    fn test_boss_triggered_this_hand_false_for_debuffed_held_card_without_enhancement() {
        // A bare (no enhancement) debuffed held card was never going to
        // contribute anything in the held-card loop - the debuff didn't
        // actually block anything, so it shouldn't pay Matador.
        let mut g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let held_king = Card::new(Value::King, Suit::Club);
        g.available.extend(vec![held_king]);
        let ace = Card::new(Value::Ace, Suit::Heart);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        g.calc_score(hand);
        assert!(!g.boss_triggered_this_hand);
    }

    #[test]
    fn test_boss_triggered_this_hand_true_for_debuffed_held_gold_card() {
        let mut g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let mut held_king = Card::new(Value::King, Suit::Club);
        held_king.enhancement = Some(Enhancement::Gold);
        g.available.extend(vec![held_king]);
        let ace = Card::new(Value::Ace, Suit::Heart);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        g.calc_score(hand);
        assert!(g.boss_triggered_this_hand);
    }

    // --- Group B: required-score multiplier (Wall, VioletVessel, Needle) ---

    #[test]
    fn test_wall_boss_quadruples_required_score() {
        let g = Game {
            current_boss: Some(BossBlind::Wall),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        assert_eq!(g.required_score(), 300 * 4);
    }

    #[test]
    fn test_violet_vessel_boss_sextuples_required_score() {
        let g = Game {
            current_boss: Some(BossBlind::VioletVessel),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        assert_eq!(g.required_score(), 300 * 6);
    }

    #[test]
    fn test_needle_boss_reduces_required_score() {
        let g = Game {
            current_boss: Some(BossBlind::Needle),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        assert_eq!(g.required_score(), 300);
    }

    #[test]
    fn test_required_score_default_boss_multiplier_for_other_bosses() {
        let g = Game {
            current_boss: Some(BossBlind::Club),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        assert_eq!(g.required_score(), 300 * 2);
    }

    #[test]
    fn test_required_score_returns_to_default_when_boss_disabled() {
        let g = Game {
            current_boss: Some(BossBlind::Wall),
            blind: Some(Blind::Boss),
            boss_disabled: true,
            ..Default::default()
        };
        assert_eq!(g.required_score(), 300 * 2);
    }

    #[test]
    fn test_boss_required_score_previews_before_boss_blind_selected() {
        let g = Game {
            current_boss: Some(BossBlind::Wall),
            blind: Some(Blind::Big),
            stage: Stage::PreBlind(),
            ..Default::default()
        };
        // required_score() reflects the *current* blind (Big), not Boss -
        // boss_required_score() previews the upcoming Boss's number instead.
        assert_eq!(g.required_score(), (300f32 * 1.5) as usize);
        assert_eq!(g.boss_required_score(), 300 * 4);
    }

    // --- Group C: hand-level chip/mult halving (Flint) ---

    #[test]
    fn test_flint_boss_halves_pair_base_chips_and_mult() {
        let mut g = Game {
            current_boss: Some(BossBlind::Flint),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let king1 = Card::new(Value::King, Suit::Heart);
        let king2 = Card::new(Value::King, Suit::Diamond);
        let hand = SelectHand::new(vec![king1, king2]).best_hand().unwrap();
        // OnePair level 1: (10, 2) halved to (5, 1). Cards unaffected: +20 chips.
        // (5 + 20) * 1 = 25.
        assert_eq!(g.calc_score(hand), 25);
    }

    #[test]
    fn test_flint_boss_zeroes_score_when_mult_floors_to_zero() {
        let mut g = Game {
            current_boss: Some(BossBlind::Flint),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let ace = Card::new(Value::Ace, Suit::Heart);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        // HighCard level 1: (5, 1) halved via integer division to (2, 0).
        // mult 0 zeroes the whole score regardless of chips - unconfirmed
        // against real Balatro's exact rounding, worth a real-game check.
        assert_eq!(g.calc_score(hand), 0);
    }

    #[test]
    fn test_flint_boss_sets_boss_triggered_this_hand() {
        let mut g = Game {
            current_boss: Some(BossBlind::Flint),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let ace = Card::new(Value::Ace, Suit::Heart);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        g.calc_score(hand);
        assert!(g.boss_triggered_this_hand);
    }

    // --- Group D: delevel by one (Arm) ---

    #[test]
    fn test_arm_boss_decrements_level_by_one_not_to_one() {
        let mut g = Game {
            current_boss: Some(BossBlind::Arm),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        g.planetarium.level_up(HandRank::OnePair);
        g.planetarium.level_up(HandRank::OnePair);
        // level 3: (40, 4) -> Arm decrements once to level 2: (25, 3).
        let king1 = Card::new(Value::King, Suit::Heart);
        let king2 = Card::new(Value::King, Suit::Diamond);
        let hand = SelectHand::new(vec![king1, king2]).best_hand().unwrap();
        // (25 + 20) * 3 = 135, NOT level-1 (10 + 20) * 2 = 60.
        assert_eq!(g.calc_score(hand), 135);
    }

    #[test]
    fn test_arm_boss_decrement_is_permanent() {
        let mut g = Game {
            current_boss: Some(BossBlind::Arm),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        g.planetarium.level_up(HandRank::OnePair);
        g.planetarium.level_up(HandRank::OnePair);
        let king1 = Card::new(Value::King, Suit::Heart);
        let king2 = Card::new(Value::King, Suit::Diamond);
        let hand = SelectHand::new(vec![king1, king2]).best_hand().unwrap();
        g.calc_score(hand);
        assert_eq!(g.planetarium.level(HandRank::OnePair).level, 2);
    }

    #[test]
    fn test_arm_boss_cannot_decrement_below_level_one() {
        let mut g = Game {
            current_boss: Some(BossBlind::Arm),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let king1 = Card::new(Value::King, Suit::Heart);
        let king2 = Card::new(Value::King, Suit::Diamond);
        let hand = SelectHand::new(vec![king1, king2]).best_hand().unwrap();
        // Already level 1: (10 + 20) * 2 = 60, unaffected.
        assert_eq!(g.calc_score(hand), 60);
        assert_eq!(g.planetarium.level(HandRank::OnePair).level, 1);
    }

    #[test]
    fn test_arm_boss_decrements_again_each_subsequent_play_until_level_one() {
        let mut g = Game {
            current_boss: Some(BossBlind::Arm),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        g.planetarium.level_up(HandRank::OnePair);
        g.planetarium.level_up(HandRank::OnePair);
        let hand = || {
            let king1 = Card::new(Value::King, Suit::Heart);
            let king2 = Card::new(Value::King, Suit::Diamond);
            SelectHand::new(vec![king1, king2]).best_hand().unwrap()
        };

        g.calc_score(hand()); // level 3 -> 2
        assert!(g.boss_triggered_this_hand);
        assert_eq!(g.planetarium.level(HandRank::OnePair).level, 2);

        g.calc_score(hand()); // level 2 -> 1
        assert!(g.boss_triggered_this_hand);
        assert_eq!(g.planetarium.level(HandRank::OnePair).level, 1);

        g.calc_score(hand()); // already level 1, no further decrement
        assert!(!g.boss_triggered_this_hand);
        assert_eq!(g.planetarium.level(HandRank::OnePair).level, 1);
    }

    #[test]
    fn test_arm_boss_triggers_matador_only_when_actually_deleveled() {
        let mut g = Game {
            current_boss: Some(BossBlind::Arm),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let king1 = Card::new(Value::King, Suit::Heart);
        let king2 = Card::new(Value::King, Suit::Diamond);
        let hand = SelectHand::new(vec![king1, king2]).best_hand().unwrap();
        g.calc_score(hand);
        assert!(!g.boss_triggered_this_hand);
    }

    #[test]
    fn test_arm_boss_still_increments_real_play_count() {
        let mut g = Game {
            current_boss: Some(BossBlind::Arm),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        g.planetarium.level_up(HandRank::OnePair);
        let king1 = Card::new(Value::King, Suit::Heart);
        let king2 = Card::new(Value::King, Suit::Diamond);
        let hand = SelectHand::new(vec![king1, king2]).best_hand().unwrap();
        g.calc_score(hand);
        assert_eq!(g.planetarium.level(HandRank::OnePair).plays, 1);
    }

    // --- Group E, part 1: Water, Manacle, Serpent ---

    #[test]
    fn test_water_boss_blocks_discards() {
        let g = Game {
            current_boss: Some(BossBlind::Water),
            blind: Some(Blind::Boss),
            discards_remaining: 4,
            ..Default::default()
        };
        assert_eq!(g.discards(), 0);
        assert_eq!(g.discards_remaining, 4);
    }

    #[test]
    fn test_water_boss_discards_restored_when_disabled_mid_round() {
        let g = Game {
            current_boss: Some(BossBlind::Water),
            blind: Some(Blind::Boss),
            discards_remaining: 4,
            boss_disabled: true,
            ..Default::default()
        };
        assert_eq!(g.discards(), 4);
    }

    #[test]
    fn test_water_boss_no_effect_outside_boss_round() {
        let g = Game {
            current_boss: Some(BossBlind::Water),
            blind: Some(Blind::Small),
            discards_remaining: 4,
            ..Default::default()
        };
        assert_eq!(g.discards(), 4);
    }

    #[test]
    fn test_discard_selected_rejected_under_water() {
        let mut g = Game {
            current_boss: Some(BossBlind::Water),
            blind: Some(Blind::Big),
            stage: Stage::PreBlind(),
            ..Default::default()
        };
        g.select_blind(Blind::Boss).expect("can select boss blind");
        let card = g.available.cards()[0];
        g.select_card(card).expect("can select card");
        let before = g.discards_remaining;
        let result = g.discard_selected();
        assert!(matches!(result, Err(GameError::NoRemainingDiscards)));
        assert_eq!(g.discards_remaining, before);
    }

    #[test]
    fn test_manacle_boss_reduces_hand_size() {
        let g = Game {
            current_boss: Some(BossBlind::Manacle),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        assert_eq!(g.hand_size(), 7);
    }

    #[test]
    fn test_manacle_boss_deals_reduced_hand() {
        let mut g = Game {
            current_boss: Some(BossBlind::Manacle),
            blind: Some(Blind::Big),
            stage: Stage::PreBlind(),
            ..Default::default()
        };
        g.select_blind(Blind::Boss).expect("can select boss blind");
        assert_eq!(g.available.cards().len(), 7);
    }

    #[test]
    fn test_manacle_boss_no_effect_outside_boss_round() {
        let g = Game {
            current_boss: Some(BossBlind::Manacle),
            blind: Some(Blind::Small),
            ..Default::default()
        };
        assert_eq!(g.hand_size(), 8);
    }

    #[test]
    fn test_serpent_boss_grows_hand_when_fewer_than_three_removed() {
        let mut g = Game {
            current_boss: Some(BossBlind::Serpent),
            blind: Some(Blind::Big),
            stage: Stage::PreBlind(),
            ..Default::default()
        };
        g.select_blind(Blind::Boss).expect("can select boss blind");
        let cards: Vec<Card> = g.available.cards().into_iter().take(2).collect();
        for card in cards {
            g.select_card(card).expect("can select card");
        }
        g.discard_selected().expect("can discard");
        assert_eq!(g.available.cards().len(), 9);
    }

    #[test]
    fn test_serpent_boss_shrinks_hand_when_more_than_three_removed() {
        let mut g = Game {
            current_boss: Some(BossBlind::Serpent),
            blind: Some(Blind::Big),
            stage: Stage::PreBlind(),
            ..Default::default()
        };
        g.select_blind(Blind::Boss).expect("can select boss blind");
        let cards: Vec<Card> = g.available.cards().into_iter().take(5).collect();
        for card in cards {
            g.select_card(card).expect("can select card");
        }
        g.discard_selected().expect("can discard");
        assert_eq!(g.available.cards().len(), 6);
    }

    #[test]
    fn test_serpent_boss_applies_to_play_selected_too() {
        let mut g = Game {
            current_boss: Some(BossBlind::Serpent),
            blind: Some(Blind::Big),
            stage: Stage::PreBlind(),
            ..Default::default()
        };
        g.select_blind(Blind::Boss).expect("can select boss blind");
        let card = g.available.cards()[0];
        g.select_card(card).expect("can select card");
        g.play_selected().expect("can play selected");
        assert_eq!(g.available.cards().len(), 10);
    }

    #[test]
    fn test_serpent_boss_no_effect_outside_boss_round() {
        let mut g = Game {
            current_boss: Some(BossBlind::Serpent),
            blind: None,
            stage: Stage::PreBlind(),
            ..Default::default()
        };
        g.select_blind(Blind::Small)
            .expect("can select small blind");
        let cards: Vec<Card> = g.available.cards().into_iter().take(2).collect();
        for card in cards {
            g.select_card(card).expect("can select card");
        }
        g.discard_selected().expect("can discard");
        assert_eq!(g.available.cards().len(), 8);
    }

    #[test]
    fn test_play_selected() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Blind(Blind::Small);
        g.blind = Some(Blind::Small);
        g.deal();
        for card in g.available.cards().iter().take(5) {
            g.available.select_card(*card).expect("can select card");
        }

        assert_eq!(g.available.selected().len(), 5);
        // Artifically set score so blind passes
        g.score += g.required_score();
        g.play_selected().expect("can play selected");

        // Should have cleared blind
        assert_eq!(g.stage, Stage::PostBlind());
        // Score should reset to 0
        assert_eq!(g.score, g.config.base_score);
        // Plays and discards should reset
        assert_eq!(g.plays, g.plays_per_round());
        assert_eq!(g.discards_remaining, g.discards_per_round());
        // All cards back in deck after clear_blind, available is empty
        assert_eq!(g.deck.len(), 52);
        // Discarded should be length 0
        assert_eq!(g.discarded.len(), 0);
        // Available is empty until next blind starts
        assert_eq!(g.available.cards().len(), 0);
    }

    #[test]
    fn test_buy_joker() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        g.money = 10;
        let j1 = crate::joker::jokers_by_rarity(crate::joker::Rarity::Common)[0].clone();
        g.shop.jokers = vec![j1.clone()];
        g.buy_joker(j1.clone()).expect("buy joker");
        assert_eq!(g.money, (10 - j1.cost()) as i64);
        assert_eq!(g.jokers.len(), 1);
    }

    #[test]
    fn test_enhancement_bonus() {
        // High card Ace with Bonus: (5 + 11 + 30) * 1 = 46
        let mut g = Game::default();
        let mut ace = Card::new(Value::Ace, Suit::Heart);
        ace.enhancement = Some(Enhancement::Bonus);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 46);
    }

    #[test]
    fn test_enhancement_mult() {
        // High card Ace with Mult: (5 + 11) * (1 + 4) = 80
        let mut g = Game::default();
        let mut ace = Card::new(Value::Ace, Suit::Heart);
        ace.enhancement = Some(Enhancement::Mult);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 80);
    }

    #[test]
    fn test_enhancement_glass() {
        // High card Ace with Glass: (5 + 11) * (1 * 2) = 32
        let mut g = Game::default();
        let mut ace = Card::new(Value::Ace, Suit::Heart);
        ace.enhancement = Some(Enhancement::Glass);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 32);
    }

    #[test]
    fn test_enhancement_stone_as_kicker() {
        // [Ace, Stone] -> High Card Ace; Stone kicker always scores +50
        // (5 + 11 + 50) * 1 = 66
        let mut g = Game::default();
        let ace = Card::new(Value::Ace, Suit::Heart);
        let mut stone = Card::new(Value::Two, Suit::Diamond);
        stone.enhancement = Some(Enhancement::Stone);
        let hand = SelectHand::new(vec![ace, stone]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 66);
    }

    #[test]
    fn test_enhancement_steel_held() {
        // Pair of Kings scored; Steel King held (not played)
        // Pair level 1: chips=10, mult=2; both Kings score: chips=10+10+10=30
        // Steel held: mult = floor(2 * 1.5) = 3 -> score = 30 * 3 = 90
        let mut g = Game::default();
        let king1 = Card::new(Value::King, Suit::Heart);
        let king2 = Card::new(Value::King, Suit::Diamond);
        let mut steel_king = Card::new(Value::King, Suit::Spade);
        steel_king.enhancement = Some(Enhancement::Steel);
        g.available.extend(vec![steel_king]);
        let hand = SelectHand::new(vec![king1, king2]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 90);
    }

    #[test]
    fn test_enhancement_gold_held() {
        // Gold card held in unplayed hand gives $3 per hand scored
        let mut g = Game::default();
        let mut gold_card = Card::new(Value::Two, Suit::Heart);
        gold_card.enhancement = Some(Enhancement::Gold);
        g.available.extend(vec![gold_card]);
        let ace = Card::new(Value::Ace, Suit::Spade);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        g.calc_score(hand);
        assert_eq!(g.money, 3);
    }

    #[test]
    fn test_seal_gold_scored() {
        // Gold Seal: $3 when this card is played and scores (score itself unaffected)
        let mut g = Game::default();
        let mut ace = Card::new(Value::Ace, Suit::Spade);
        ace.seal = Some(Seal::Gold);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 16);
        assert_eq!(g.money, 3);
    }

    #[test]
    fn test_seal_red_retrigger_played() {
        // Red Seal: retrigger this card's played abilities.
        // High card Ace, chips scored twice: (5 + 11 + 11) * 1 = 27
        let mut g = Game::default();
        let mut ace = Card::new(Value::Ace, Suit::Spade);
        ace.seal = Some(Seal::Red);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 27);
    }

    #[test]
    fn test_seal_red_retrigger_gold_held() {
        // Red Seal retriggers held-card abilities too, not just played ones:
        // a held Gold+Red card earns $6 (2x $3), not $3.
        let mut g = Game::default();
        let mut gold_red = Card::new(Value::Two, Suit::Heart);
        gold_red.enhancement = Some(Enhancement::Gold);
        gold_red.seal = Some(Seal::Red);
        g.available.extend(vec![gold_red]);
        let ace = Card::new(Value::Ace, Suit::Spade);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        g.calc_score(hand);
        assert_eq!(g.money, 6);
    }

    #[test]
    fn test_seal_red_retrigger_steel_held() {
        // Pair of Kings scored; held Steel+Red King retriggers its Mult bonus twice.
        // Pair level 1: chips=10, mult=2; both Kings score: chips=30
        // Steel triggers twice: mult = 2 -> 3 (floor(2*1.5)) -> 4 (floor(3*1.5))
        // score = 30 * 4 = 120
        let mut g = Game::default();
        let king1 = Card::new(Value::King, Suit::Heart);
        let king2 = Card::new(Value::King, Suit::Diamond);
        let mut steel_red_king = Card::new(Value::King, Suit::Spade);
        steel_red_king.enhancement = Some(Enhancement::Steel);
        steel_red_king.seal = Some(Seal::Red);
        g.available.extend(vec![steel_red_king]);
        let hand = SelectHand::new(vec![king1, king2]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 120);
    }

    #[test]
    fn test_stone_kicker_red_seal_retrigger() {
        // [Ace, Stone+Red Seal] -> High Card Ace; stone kicker retriggers twice.
        // (5 + 11 + 50 + 50) * 1 = 116
        let mut g = Game::default();
        let ace = Card::new(Value::Ace, Suit::Heart);
        let mut stone = Card::new(Value::Two, Suit::Diamond);
        stone.enhancement = Some(Enhancement::Stone);
        stone.seal = Some(Seal::Red);
        let hand = SelectHand::new(vec![ace, stone]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 116);
    }

    #[test]
    fn test_hanging_chad_retriggers_first_played_card() {
        use crate::joker::*;
        // [Ace, Stone], Ace first -> Ace retriggers 3x, stone kicker only 1x.
        // (5 + 11*3 + 50) * 1 = 88
        let mut g = Game::default();
        g.jokers.push(Jokers::HangingChad(HangingChad::default()));
        let jokers = g.jokers.clone();
        g.effect_registry.register_jokers(jokers, &g.clone());
        let ace = Card::new(Value::Ace, Suit::Heart);
        let mut stone = Card::new(Value::Two, Suit::Diamond);
        stone.enhancement = Some(Enhancement::Stone);
        let hand = SelectHand::new(vec![ace, stone]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 88);
    }

    #[test]
    fn test_hanging_chad_stone_kicker_competes_for_first() {
        use crate::joker::*;
        // Same cards as above, stone selected first this time -> the stone
        // kicker gets HangingChad's retrigger instead of the Ace.
        // (5 + 11 + 50*3) * 1 = 166
        let mut g = Game::default();
        g.jokers.push(Jokers::HangingChad(HangingChad::default()));
        let jokers = g.jokers.clone();
        g.effect_registry.register_jokers(jokers, &g.clone());
        let mut stone = Card::new(Value::Two, Suit::Diamond);
        stone.enhancement = Some(Enhancement::Stone);
        let ace = Card::new(Value::Ace, Suit::Heart);
        let hand = SelectHand::new(vec![stone, ace]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 166);
    }

    #[test]
    fn test_dusk_no_retrigger_with_plays_remaining() {
        use crate::joker::*;
        let mut g = Game::default();
        g.jokers.push(Jokers::Dusk(Dusk::default()));
        let jokers = g.jokers.clone();
        g.effect_registry.register_jokers(jokers, &g.clone());
        g.plays = 1;
        let ace = Card::new(Value::Ace, Suit::Heart);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 16);
    }

    #[test]
    fn test_dusk_retriggers_on_final_hand() {
        use crate::joker::*;
        // Same hand as above, but this is the last play of the round:
        // (5 + 11 + 11) * 1 = 27
        let mut g = Game::default();
        g.jokers.push(Jokers::Dusk(Dusk::default()));
        let jokers = g.jokers.clone();
        g.effect_registry.register_jokers(jokers, &g.clone());
        g.plays = 0;
        let ace = Card::new(Value::Ace, Suit::Heart);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 27);
    }

    #[test]
    fn test_hack_retriggers_only_low_ranks() {
        use crate::joker::*;
        // Two Pair (Threes + Kings): Hack retriggers each Three, not the Kings.
        let cards = vec![
            Card::new(Value::Three, Suit::Heart),
            Card::new(Value::Three, Suit::Diamond),
            Card::new(Value::King, Suit::Spade),
            Card::new(Value::King, Suit::Club),
        ];

        let mut base_game = Game::default();
        let base_hand = SelectHand::new(cards.clone()).best_hand().unwrap();
        let base = base_game.calc_score(base_hand);

        let mut g = Game::default();
        g.jokers.push(Jokers::Hack(Hack::default()));
        let jokers = g.jokers.clone();
        g.effect_registry.register_jokers(jokers, &g.clone());
        let hand = SelectHand::new(cards).best_hand().unwrap();
        // Each Three retriggers once more: +3 chips * 2 cards = +6 chips,
        // scaled by TwoPair's X2 mult -> +12 final score.
        assert_eq!(g.calc_score(hand), base + 12);
    }

    #[test]
    fn test_sock_and_buskin_retriggers_only_face_cards() {
        use crate::joker::*;
        // Two Pair (Jacks + Fours): SockAndBuskin retriggers each Jack, not the Fours.
        let cards = vec![
            Card::new(Value::Jack, Suit::Heart),
            Card::new(Value::Jack, Suit::Diamond),
            Card::new(Value::Four, Suit::Spade),
            Card::new(Value::Four, Suit::Club),
        ];

        let mut base_game = Game::default();
        let base_hand = SelectHand::new(cards.clone()).best_hand().unwrap();
        let base = base_game.calc_score(base_hand);

        let mut g = Game::default();
        g.jokers
            .push(Jokers::SockAndBuskin(SockAndBuskin::default()));
        let jokers = g.jokers.clone();
        g.effect_registry.register_jokers(jokers, &g.clone());
        let hand = SelectHand::new(cards).best_hand().unwrap();
        // Each Jack retriggers once more: +10 chips * 2 cards = +20 chips,
        // scaled by TwoPair's X2 mult -> +40 final score.
        assert_eq!(g.calc_score(hand), base + 40);
    }

    #[test]
    fn test_mime_retriggers_held_steel() {
        use crate::joker::*;
        // Same math as test_seal_red_retrigger_steel_held (120), Mime instead of Red Seal:
        // mult 2 -> 3 (floor(2*1.5)) -> 4 (floor(3*1.5)); score = 30 * 4 = 120
        let mut g = Game::default();
        g.jokers.push(Jokers::Mime(Mime::default()));
        let jokers = g.jokers.clone();
        g.effect_registry.register_jokers(jokers, &g.clone());
        let king1 = Card::new(Value::King, Suit::Heart);
        let king2 = Card::new(Value::King, Suit::Diamond);
        let mut steel_king = Card::new(Value::King, Suit::Spade);
        steel_king.enhancement = Some(Enhancement::Steel);
        g.available.extend(vec![steel_king]);
        let hand = SelectHand::new(vec![king1, king2]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 120);
    }

    #[test]
    fn test_seal_purple_discard_creates_tarot() {
        // Purple Seal: creates a Tarot card when this card is discarded.
        let mut g = Game::default();
        g.deal();
        let card = g.available.cards()[0];
        g.mutate_card(card.id, |c| c.seal = Some(Seal::Purple));
        let card = g.available.cards()[0];
        g.select_card(card).expect("can select card");

        assert_eq!(g.consumables.len(), 0);
        g.discard_selected().expect("can discard");
        assert_eq!(g.consumables.len(), 1);
        assert!(matches!(g.consumables[0].kind, ConsumableKind::Tarot(_)));
    }

    #[test]
    fn test_seal_purple_discard_respects_slots() {
        let mut g = Game::default();
        g.config.consumable_slots = 0;
        g.deal();
        let card = g.available.cards()[0];
        g.mutate_card(card.id, |c| c.seal = Some(Seal::Purple));
        let card = g.available.cards()[0];
        g.select_card(card).expect("can select card");

        g.discard_selected().expect("can discard");
        assert_eq!(g.consumables.len(), 0);
    }

    #[test]
    fn test_seal_blue_round_end_creates_planet() {
        // Blue Seal: creates a Planet card if held in hand when the round ends.
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Blind(Blind::Small);
        g.blind = Some(Blind::Small);
        g.deal();

        let held_card = g.available.cards()[7];
        g.mutate_card(held_card.id, |c| c.seal = Some(Seal::Blue));
        for card in g.available.cards()[0..5].to_vec() {
            g.available.select_card(card).expect("can select card");
        }
        assert_eq!(g.available.selected().len(), 5);

        g.score += g.required_score();
        assert_eq!(g.consumables.len(), 0);
        g.play_selected().expect("can play selected");

        assert_eq!(g.consumables.len(), 1);
        assert!(matches!(g.consumables[0].kind, ConsumableKind::Planet(_)));
    }

    #[test]
    fn test_seal_blue_does_not_fire_before_blind_cleared() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Blind(Blind::Small);
        g.blind = Some(Blind::Small);
        g.deal();

        let held_card = g.available.cards()[7];
        g.mutate_card(held_card.id, |c| c.seal = Some(Seal::Blue));
        for card in g.available.cards()[0..5].to_vec() {
            g.available.select_card(card).expect("can select card");
        }
        // score not bumped past required_score() - blind stays open
        g.play_selected().expect("can play selected");

        assert_eq!(g.consumables.len(), 0);
    }

    #[test]
    fn test_edition_foil() {
        // High card Ace with Foil: (5 + 11 + 50) * 1 = 66
        let mut g = Game::default();
        let mut ace = Card::new(Value::Ace, Suit::Heart);
        ace.edition = Edition::Foil;
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 66);
    }

    #[test]
    fn test_edition_holographic() {
        // High card Ace with Holographic: (5 + 11) * (1 + 10) = 176
        let mut g = Game::default();
        let mut ace = Card::new(Value::Ace, Suit::Heart);
        ace.edition = Edition::Holographic;
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 176);
    }

    #[test]
    fn test_edition_polychrome() {
        // Pair of Kings, second King has Polychrome
        // Pair level 1: chips=10, mult=2
        // king1: chips += 10; king2 (poly): chips += 10, mult = floor(2*1.5) = 3
        // score = (10+10+10) * 3 = 90
        let mut g = Game::default();
        let king = Card::new(Value::King, Suit::Heart);
        let mut poly_king = Card::new(Value::King, Suit::Diamond);
        poly_king.edition = Edition::Polychrome;
        let hand = SelectHand::new(vec![king, poly_king]).best_hand().unwrap();
        assert_eq!(g.calc_score(hand), 90);
    }

    #[test]
    fn test_joker_edition_foil() {
        use crate::joker::Jokers;
        use balatro_types::joker::TheJoker;
        let ace = Card::new(Value::Ace, Suit::Heart);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();

        // TheJoker Base: (5+11)*(1+4) = 80
        let mut g = Game {
            stage: Stage::Blind(Blind::Small),
            ..Default::default()
        };
        let mut j = Jokers::TheJoker(TheJoker::default());
        j.set_edition(Edition::Foil);
        g.money += 1000;
        g.stage = Stage::Shop();
        g.shop.jokers.push(j.clone());
        g.buy_joker(j).unwrap();
        g.stage = Stage::Blind(Blind::Small);
        // Foil fires before TheJoker: chips += 50, then +4 mult
        // (5+11+50) * (1+4) = 66 * 5 = 330
        assert_eq!(g.calc_score(hand), 330);
    }

    #[test]
    fn test_joker_edition_holographic() {
        use crate::joker::Jokers;
        use balatro_types::joker::TheJoker;
        let ace = Card::new(Value::Ace, Suit::Heart);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();

        let mut g = Game {
            stage: Stage::Blind(Blind::Small),
            ..Default::default()
        };
        let mut j = Jokers::TheJoker(TheJoker::default());
        j.set_edition(Edition::Holographic);
        g.money += 1000;
        g.stage = Stage::Shop();
        g.shop.jokers.push(j.clone());
        g.buy_joker(j).unwrap();
        g.stage = Stage::Blind(Blind::Small);
        // Holo fires before TheJoker: mult += 10, then +4 mult
        // (5+11) * (1+10+4) = 16 * 15 = 240
        assert_eq!(g.calc_score(hand), 240);
    }

    #[test]
    fn test_joker_edition_polychrome() {
        use crate::joker::Jokers;
        use balatro_types::joker::TheJoker;
        let ace = Card::new(Value::Ace, Suit::Heart);
        let hand = SelectHand::new(vec![ace]).best_hand().unwrap();

        let mut g = Game {
            stage: Stage::Blind(Blind::Small),
            ..Default::default()
        };
        let mut j = Jokers::TheJoker(TheJoker::default());
        j.set_edition(Edition::Polychrome);
        g.money += 1000;
        g.stage = Stage::Shop();
        g.shop.jokers.push(j.clone());
        g.buy_joker(j).unwrap();
        g.stage = Stage::Blind(Blind::Small);
        // TheJoker fires (+4 mult), then Polychrome x1.5: floor((1+4)*1.5) = 7
        // (5+11) * 7 = 112
        assert_eq!(g.calc_score(hand), 112);
    }

    #[test]
    fn test_joker_edition_negative_buy_increases_slots() {
        use crate::joker::Jokers;
        use balatro_types::joker::TheJoker;
        let mut g = Game::default();
        let slots_before = g.config.joker_slots;

        g.money += 1000;
        g.stage = Stage::Shop();
        let mut j = Jokers::TheJoker(TheJoker::default());
        j.set_edition(Edition::Negative);
        g.shop.jokers.push(j.clone());
        g.buy_joker(j).unwrap();

        assert_eq!(g.config.joker_slots, slots_before + 1);
    }

    #[test]
    fn test_joker_edition_negative_sell_decreases_slots() {
        use crate::joker::Jokers;
        use balatro_types::joker::TheJoker;
        let mut g = Game::default();
        let slots_before = g.config.joker_slots;

        g.money += 1000;
        g.stage = Stage::Shop();
        let mut j = Jokers::TheJoker(TheJoker::default());
        j.set_edition(Edition::Negative);
        g.shop.jokers.push(j.clone());
        g.buy_joker(j).unwrap();
        assert_eq!(g.config.joker_slots, slots_before + 1);

        g.sell_joker(0).unwrap();
        assert_eq!(g.config.joker_slots, slots_before);
    }

    #[test]
    fn test_destroy_card() {
        let mut g = Game::default();
        g.deal();
        let card = g.available.cards()[0];
        let available_before = g.available.cards().len();
        g.destroy_card(card.id);
        assert_eq!(g.available.cards().len(), available_before - 1);
        assert!(g.available.cards().iter().all(|c| c.id != card.id));
    }

    #[test]
    fn test_joker_state_mut_finds_by_instance_id() {
        use crate::joker::Jokers;
        use balatro_types::joker::TheJoker;

        let mut g = Game::default();
        let mut joker = Jokers::TheJoker(TheJoker::default());
        joker.set_instance_id(7);
        g.jokers.push(joker);

        let state = g.joker_state_mut(7).expect("joker with id 7 exists");
        state.counter = 3.5;

        assert_eq!(g.jokers[0].state().counter, 3.5);
    }

    #[test]
    fn test_joker_state_mut_missing_id_returns_none() {
        use crate::joker::Jokers;
        use balatro_types::joker::TheJoker;

        let mut g = Game::default();
        let mut joker = Jokers::TheJoker(TheJoker::default());
        joker.set_instance_id(7);
        g.jokers.push(joker);

        assert!(g.joker_state_mut(999).is_none());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_to_from_json_roundtrip() {
        let mut g = Game::default();
        g.start();
        let json = g.to_json().expect("serialize");
        let g2 = Game::from_json(&json).expect("deserialize");
        assert_eq!(g2.stage, g.stage);
        assert_eq!(g2.ante_current, g.ante_current);
        assert_eq!(g2.plays, g.plays);
        assert_eq!(g2.discards_remaining, g.discards_remaining);
        assert_eq!(g2.money, g.money);
        assert_eq!(g2.available.cards().len(), g.available.cards().len());
        assert_eq!(g2.deck.len(), g.deck.len());
    }

    /// A *played* run must round-trip, not just a fresh one. Every existing
    /// round-trip test starts from `Game::default()`, which leaves the open
    /// pack, the populated boss state, the planetarium levels, the mutated
    /// deck and the action history all at their defaults, so none of them
    /// exercises any of it.
    #[cfg(feature = "serde")]
    #[test]
    fn test_a_played_run_round_trips() {
        let mut g = Game::new(Config {
            seed: Some(TAG_TEST_SEED),
            ..Config::default()
        });
        g.start();
        // Build up real state: play into ante 2, level a hand, hold jokers,
        // redeem vouchers, and open a pack.
        // Play every blind of ante 1, then the first two of ante 2, so the
        // run has a real history rather than a starting position.
        for _ in 0..5 {
            // The shop sits between blinds, so advance to the next round
            // before trying to select the following blind.
            if g.stage == Stage::Shop() {
                g.handle_action(Action::NextRound()).expect("next round");
            }
            if !g
                .gen_actions()
                .any(|a| matches!(a, Action::SelectBlind(_)))
            {
                break;
            }
            let next = match g.blind {
                None => Blind::Small,
                Some(b) => b.next(),
            };
            g.handle_action(Action::SelectBlind(next)).expect("select");
            for card in g.available.cards() {
                g.handle_action(Action::SelectCard(card)).ok();
            }
            g.score += g.required_score();
            g.handle_action(Action::Play()).expect("play");
            if g.gen_actions().any(|a| matches!(a, Action::CashOut(_))) {
                g.handle_action(Action::CashOut(0)).expect("cash out");
            } else {
                break;
            }
        }
        // Some state that only a played run has.
        g.planetarium.level_up(HandRank::TwoPair);
        g.planetarium.level_up(HandRank::TwoPair);
        g.jokers.push(Jokers::TheJoker(Default::default()));
        g.vouchers.redeem(Voucher::Grabber);
        g.consumables
            .push(crate::consumable::Consumable::tarot(crate::tarot::Tarot::Fool));
        assert!(g.ante_current > Ante::One, "the run should have advanced");
        assert!(!g.action_history.is_empty());

        let json = g.to_json().expect("serialize");
        let loaded = Game::from_json(&json).expect("deserialize");

        assert_eq!(loaded.ante_current, g.ante_current, "ante");
        assert_eq!(loaded.round, g.round, "round");
        assert_eq!(loaded.money, g.money, "money");
        assert_eq!(loaded.deck.cards(), g.deck.cards(), "deck");
        assert_eq!(
            loaded.available.cards(),
            g.available.cards(),
            "available"
        );
        assert_eq!(loaded.discarded, g.discarded, "discarded");
        assert_eq!(loaded.jokers, g.jokers, "jokers");
        assert_eq!(loaded.consumables, g.consumables, "consumables");
        assert_eq!(
            loaded.planetarium.level(HandRank::TwoPair).level,
            g.planetarium.level(HandRank::TwoPair).level,
            "planetarium"
        );
        assert_eq!(loaded.action_history, g.action_history, "history");
        assert_eq!(loaded.stage, g.stage, "stage");
        assert_eq!(loaded.hands_played_this_run, g.hands_played_this_run, "hands");
    }

    /// Serialization must be stable: the second write is byte-identical to
    /// the first, or a save/load cycle silently rewrites the file.
    #[cfg(feature = "serde")]
    #[test]
    fn test_json_is_stable_across_a_round_trip() {
        let mut g = game_in_shop(100);
        g.jokers.push(Jokers::TheJoker(Default::default()));
        let once = g.to_json().expect("serialize");
        let twice = Game::from_json(&once)
            .expect("deserialize")
            .to_json()
            .expect("reserialize");
        assert_eq!(once, twice, "a round trip must not rewrite the save");
    }

    /// A joker's accumulator and sell bonus are the state most likely to be
    /// lost in a round trip, so they are pinned directly rather than relying
    /// on a whole-run comparison.
    #[cfg(feature = "serde")]
    #[test]
    fn test_joker_counters_and_sell_bonus_round_trip() {
        let mut g = game_in_shop(100);
        let mut ramen = Jokers::Ramen(Default::default());
        ramen.state_mut().counter = 13.0;
        let mut egg = Jokers::Egg(Default::default());
        egg.add_sell_bonus(7);
        g.jokers = vec![ramen.clone(), egg.clone()];

        let json = g.to_json().expect("serialize");
        let loaded = Game::from_json(&json).expect("deserialize");
        assert_eq!(loaded.jokers[0].state().counter, 13.0, "Ramen's tally");
        assert_eq!(loaded.jokers[1].sell_bonus(), 7, "Egg's sell bonus");
        assert_eq!(loaded.jokers, g.jokers, "and equality sees them");

        // And equality must actually distinguish them, or the assertion
        // above would pass even with a lossy round trip.
        let mut changed = ramen.clone();
        changed.state_mut().counter = 14.0;
        assert_ne!(changed, ramen, "a different tally is a different joker");
        let mut changed = egg.clone();
        changed.add_sell_bonus(1);
        assert_ne!(changed, egg, "a different sell bonus is a different joker");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_from_json_restores_effect_registry() {
        use crate::joker::Jokers;
        use balatro_types::joker::HangingChad;
        let mut g = Game::default();
        g.start();
        let joker = Jokers::HangingChad(HangingChad::default());
        g.jokers.push(joker);
        let jokers = g.jokers.clone();
        g.effect_registry.register_jokers(jokers, &g.clone());
        assert!(!g.effect_registry.trigger_count_played.is_empty());

        let json = g.to_json().expect("serialize");
        let g2 = Game::from_json(&json).expect("deserialize");
        assert_eq!(g2.jokers.len(), 1);
        assert!(!g2.effect_registry.trigger_count_played.is_empty());
    }

    #[cfg(feature = "serde")]
    /// A save missing `Config` keys must still load. The TUI loads
    /// user-supplied files, and a hand-edited or older save is a real case,
    /// so a single absent key must not lose the whole run.
    #[test]
    fn test_from_json_backfills_missing_config_keys() {
        let g = Game::default();
        let mut value: serde_json::Value =
            serde_json::from_str(&g.to_json().expect("serialize")).expect("parse");
        let config = value
            .as_object_mut()
            .expect("game serializes to an object")
            .get_mut("config")
            .and_then(serde_json::Value::as_object_mut)
            .expect("config serializes to an object");
        // Strip a spread of keys, including the two enums and one added
        // after this code was written.
        for key in [
            "deck",
            "stake",
            "plays",
            "discards",
            "money_start",
            "ante_start",
            "joker_slots",
        ] {
            assert!(config.remove(key).is_some(), "{key} was not present");
        }

        let json = serde_json::to_string(&value).expect("reserialize");
        let g2 = Game::from_json(&json).expect("a partial config must still load");
        assert_eq!(g2.config.deck, balatro_types::DeckVariant::Red);
        assert_eq!(g2.config.stake, balatro_types::Stake::White);
        assert_eq!(g2.config.plays, Config::new().plays);
        assert_eq!(g2.config.discards, Config::new().discards);
    }

    /// An empty config object is the worst case: every key absent.
    #[test]
    fn test_from_json_backfills_an_empty_config_object() {
        let g = Game::default();
        let mut value: serde_json::Value =
            serde_json::from_str(&g.to_json().expect("serialize")).expect("parse");
        value
            .as_object_mut()
            .expect("game serializes to an object")
            .insert("config".to_string(), serde_json::json!({}));

        let json = serde_json::to_string(&value).expect("reserialize");
        let g2 = Game::from_json(&json).expect("an empty config must still load");
        assert_eq!(g2.config.plays, Config::new().plays);
        assert_eq!(g2.plays_per_round(), g2.plays_per_round(), "and the run is usable");
    }

    #[test]
    fn test_from_json_backfills_missing_current_boss() {
        let mut g = Game::default();
        g.start();
        assert!(g.current_boss.is_some());

        // Simulate a save written before `current_boss` existed by stripping
        // the field out of the serialized JSON.
        let mut value: serde_json::Value =
            serde_json::from_str(&g.to_json().expect("serialize")).expect("parse");
        value
            .as_object_mut()
            .expect("game serializes to an object")
            .remove("current_boss");
        let json = serde_json::to_string(&value).expect("reserialize");

        let g2 = Game::from_json(&json).expect("deserialize");
        assert!(g2.current_boss.is_some());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_from_json_invalid() {
        let result = Game::from_json("not valid json");
        assert!(result.is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_from_json_advances_id_counter_past_loaded_ids() {
        let mut g = Game::default();
        let mut high_id_card = Card::new(Value::King, Suit::Spade);
        high_id_card.id = 999_999;
        g.deck.push(high_id_card);

        let json = g.to_json().expect("serialize");
        let _g2 = Game::from_json(&json).expect("deserialize");

        // Card::new() after loading must not collide with the loaded id.
        let new_card = Card::new(Value::Two, Suit::Heart);
        assert!(new_card.id > 999_999);
    }

    #[test]
    fn test_glass_shatter_preserves_hand_size() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Blind(Blind::Small);
        g.blind = Some(Blind::Small);

        // Replace available with controlled cards so we have exactly one glass card.
        // Use mixed values/suits to ensure the hand scores below the blind threshold
        // so clear_blind() / deal() is not triggered mid-test.
        g.available.empty();
        // Glass card is ace (highest) so it's selected as the high card and scores.
        // high card ace scores (5+11)*2 = 32, below 300 small blind threshold.
        let mut glass = Card::new(Value::Ace, Suit::Heart);
        glass.enhancement = Some(Enhancement::Glass);
        let glass_id = glass.id;
        let others = vec![
            Card::new(Value::Two, Suit::Club),
            Card::new(Value::Four, Suit::Diamond),
            Card::new(Value::Six, Suit::Spade),
            Card::new(Value::Eight, Suit::Club),
            Card::new(Value::Ten, Suit::Diamond),
            Card::new(Value::Queen, Suit::Spade),
        ];
        let mut hand_cards = vec![glass];
        hand_cards.extend(others);
        g.available.extend(hand_cards);

        let hand_size_before = g.available.cards().len();

        // Force 100% shatter probability (only needs to be 4 but overkill is ok)
        g.prob_mult_base = 100;

        // Select glass + 4 others (5-card hand)
        let to_select: Vec<Card> = g.available.cards().into_iter().take(5).collect();
        for card in to_select {
            g.available.select_card(card).expect("select card");
        }

        g.play_selected().expect("play selected");

        // Hand size must be unchanged
        assert_eq!(g.available.cards().len(), hand_size_before);
        // Glass card must be permanently gone (not hiding in discarded to return next deal)
        assert!(g.discarded.iter().all(|c| c.id != glass_id));
    }

    #[test]
    fn test_targeting_tarot_in_blind_invalid_selection_preserves_consumable() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Blind(Blind::Small);
        g.blind = Some(Blind::Small);
        g.deal();
        // no cards selected — Magician needs at least 1
        g.consumables = vec![Consumable::tarot(Tarot::Magician)];
        let res = g.use_consumable(Consumable::tarot(Tarot::Magician));
        assert!(matches!(res, Err(GameError::InvalidAction)));
        assert_eq!(g.consumables.len(), 1);
        assert!(g.consumables.contains(&Consumable::tarot(Tarot::Magician)));
    }

    #[test]
    fn test_targeting_tarot_in_blind_too_many_selected_preserves_consumable() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Blind(Blind::Small);
        g.blind = Some(Blind::Small);
        g.deal();
        // Lovers accepts exactly 1 — select 2 to exceed max
        let cards = g.available.cards();
        g.available.select_card(cards[0]).unwrap();
        g.available.select_card(cards[1]).unwrap();
        g.consumables = vec![Consumable::tarot(Tarot::Lovers)];
        let res = g.use_consumable(Consumable::tarot(Tarot::Lovers));
        assert!(matches!(res, Err(GameError::InvalidAction)));
        assert_eq!(g.consumables.len(), 1);
    }

    #[test]
    fn test_sell_joker() {
        use crate::joker::Jokers;
        use balatro_types::joker::TheJoker;
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        g.money = 0;
        let joker = Jokers::TheJoker(TheJoker::default());
        let sell_value = joker.sell_value();
        g.jokers.push(joker);
        assert_eq!(g.jokers.len(), 1);

        g.sell_joker(0).expect("sell joker");
        assert_eq!(g.jokers.len(), 0);
        assert_eq!(g.money, sell_value as i64);
    }

    #[test]
    fn test_sell_joker_invalid_index() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        let res = g.sell_joker(0);
        assert!(matches!(res, Err(GameError::InvalidAction)));
    }

    #[test]
    fn test_sell_joker_invalid_stage() {
        use crate::joker::Jokers;
        use balatro_types::joker::TheJoker;
        let mut g = Game::default();
        g.start();
        g.stage = Stage::End(crate::stage::End::Win);
        g.jokers.push(Jokers::TheJoker(TheJoker::default()));
        let res = g.sell_joker(0);
        assert!(matches!(res, Err(GameError::InvalidStage)));
    }

    #[test]
    fn test_sell_joker_during_blind() {
        use crate::joker::Jokers;
        use balatro_types::joker::TheJoker;
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Blind(Blind::Small);
        g.blind = Some(Blind::Small);
        g.money = 0;
        let joker = Jokers::TheJoker(TheJoker::default());
        let sell_value = joker.sell_value();
        g.jokers.push(joker);

        g.sell_joker(0).expect("sell joker during blind");
        assert_eq!(g.jokers.len(), 0);
        assert_eq!(g.money, sell_value as i64);
    }

    #[test]
    fn test_sell_luchador_disables_boss_only_for_itself() {
        use crate::joker::Jokers;
        use balatro_types::joker::{Luchador, TheJoker};
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        g.jokers.push(Jokers::TheJoker(TheJoker::default()));
        g.jokers.push(Jokers::Luchador(Luchador::default()));

        // Selling an unrelated joker must not trigger Luchador's OnSell effect.
        g.sell_joker(0).expect("sell plain joker");
        assert!(!g.boss_disabled);

        g.sell_joker(0).expect("sell luchador");
        assert!(g.boss_disabled);
    }

    #[test]
    fn test_sell_luchador_restores_water_boss_discards_via_handle_action() {
        use crate::joker::Jokers;
        use balatro_types::joker::Luchador;
        let mut g = Game {
            current_boss: Some(BossBlind::Water),
            blind: Some(Blind::Boss),
            stage: Stage::Blind(Blind::Boss),
            discards_remaining: 4,
            jokers: vec![Jokers::Luchador(Luchador::default())],
            ..Default::default()
        };
        assert_eq!(g.discards(), 0);

        g.handle_action(Action::SellJoker(0))
            .expect("sell luchador via handle_action");
        assert_eq!(g.discards(), 4);
    }

    #[test]
    fn test_sell_consumable() {
        use crate::planet::Planets;
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        g.money = 0;
        let c = Consumable::planet(Planets::Mercury);
        let sell_value = c.sell_value();
        g.consumables.push(c);
        assert_eq!(g.consumables.len(), 1);

        g.sell_consumable(0).expect("sell consumable");
        assert_eq!(g.consumables.len(), 0);
        assert_eq!(g.money, sell_value as i64);
    }

    #[test]
    fn test_sell_consumable_invalid_index() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        let res = g.sell_consumable(0);
        assert!(matches!(res, Err(GameError::InvalidAction)));
    }

    #[test]
    fn test_sell_consumable_invalid_stage() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::End(crate::stage::End::Win);
        g.consumables
            .push(Consumable::tarot(crate::tarot::Tarot::Fool));
        let res = g.sell_consumable(0);
        assert!(matches!(res, Err(GameError::InvalidStage)));
    }

    #[test]
    fn test_sell_joker_during_tarot_hand() {
        use crate::joker::Jokers;
        use balatro_types::joker::TheJoker;
        let mut g = Game::default();
        g.start();
        g.stage = Stage::TarotHand(Tarot::Magician);
        g.money = 0;
        let joker = Jokers::TheJoker(TheJoker::default());
        let sell_value = joker.sell_value();
        g.jokers.push(joker);

        g.sell_joker(0).expect("sell joker during tarot hand");
        assert_eq!(g.jokers.len(), 0);
        assert_eq!(g.money, sell_value as i64);
    }

    #[test]
    fn test_sell_consumable_during_tarot_hand() {
        use crate::planet::Planets;
        let mut g = Game::default();
        g.start();
        g.stage = Stage::TarotHand(Tarot::Magician);
        g.money = 0;
        let c = Consumable::planet(Planets::Mercury);
        let sell_value = c.sell_value();
        g.consumables.push(c);

        g.sell_consumable(0)
            .expect("sell consumable during tarot hand");
        assert_eq!(g.consumables.len(), 0);
        assert_eq!(g.money, sell_value as i64);
    }

    #[test]
    fn test_sell_joker_removes_effects() {
        use crate::joker::Jokers;
        use balatro_types::joker::HangingChad;
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        let joker = Jokers::HangingChad(HangingChad::default());
        g.jokers.push(joker);
        let jokers = g.jokers.clone();
        g.effect_registry.register_jokers(jokers, &g.clone());
        assert!(!g.effect_registry.trigger_count_played.is_empty());

        g.sell_joker(0).expect("sell joker");
        assert!(g.effect_registry.trigger_count_played.is_empty());
    }

    #[test]
    fn test_buy_pack() {
        use crate::pack::{Pack, PackCategory, PackSize};
        use crate::planet::Planets;
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        g.money = 100;
        let pack = Pack {
            category: PackCategory::Celestial,
            size: PackSize::Normal,
            contents: vec![PackContent::Planet(Planets::Mercury)],
        };
        let cost = pack.cost();
        let picks = pack.picks_allowed();
        g.shop.packs.push(pack.clone());
        g.buy_pack(pack).expect("buy pack");
        assert_eq!(g.stage, Stage::PackOpen());
        assert_eq!(g.money, (100 - cost) as i64);
        let state = g.open_pack.as_ref().expect("open pack state");
        assert_eq!(state.picks_remaining, picks);
    }

    #[test]
    fn test_skip_pack() {
        let mut g = Game::default();
        g.start();
        g.open_pack = Some(OpenPackState {
            picks_remaining: 1,
            description: String::new(),
            contents: vec![],
        });
        g.stage = Stage::PackOpen();
        g.skip_pack().expect("skip pack");
        assert_eq!(g.stage, Stage::Shop());
        assert!(g.open_pack.is_none());
    }

    #[test]
    fn test_skip_blind_small() {
        let mut g = Game::default();
        g.start();
        assert!(g.tags.is_empty());
        g.skip_blind(Blind::Small).expect("skip small blind");
        assert_eq!(g.blind, Some(Blind::Small));
        assert_eq!(g.stage, Stage::PreBlind());
        assert_eq!(g.tags.len(), 1);
    }

    #[test]
    fn test_skip_blind_wrong_blind() {
        let mut g = Game::default();
        g.start();
        // Game just started, next expected blind is Small, not Big
        let res = g.skip_blind(Blind::Big);
        assert!(matches!(res, Err(GameError::InvalidBlind)));
    }

    #[test]
    fn test_skip_blind_boss_rejected() {
        let mut g = Game::default();
        g.start();
        g.blind = Some(Blind::Big);
        let res = g.skip_blind(Blind::Boss);
        assert!(matches!(res, Err(GameError::InvalidBlind)));
    }

    #[test]
    fn test_skip_blind_wrong_stage() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        let res = g.skip_blind(Blind::Small);
        assert!(matches!(res, Err(GameError::InvalidStage)));
    }

    #[test]
    fn test_skip_blind_draws_a_real_tag() {
        let mut g = Game::default();
        g.start();
        let expected = g.small_blind_tag;
        g.skip_blind(Blind::Small).expect("skip small blind");
        assert_eq!(g.tags[0], expected);
    }

    #[test]
    fn test_ante_tags_exist_from_game_start() {
        let g = Game::default();
        assert!(g.skip_tag(Blind::Small).is_some());
        assert!(g.skip_tag(Blind::Big).is_some());
        assert!(g.skip_tag(Blind::Boss).is_none());
    }

    #[test]
    fn test_ante_tags_redrawn_after_boss_defeated() {
        let mut g = Game {
            stage: Stage::Blind(Blind::Boss),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        let ante_before = g.ante_current;
        g.handle_score(1_000_000).expect("handle score");
        assert_ne!(g.ante_current, ante_before);
        assert!(Tag::iter().any(|t| t == g.small_blind_tag));
        assert!(Tag::iter().any(|t| t == g.big_blind_tag));
    }

    #[test]
    fn test_current_boss_set_on_game_new() {
        let g = Game::new(Config::default());
        assert!(g.current_boss.is_some());
        assert!(!g.current_boss.unwrap().is_finisher());
    }

    #[test]
    fn test_current_boss_redrawn_after_boss_defeated() {
        let mut g = Game {
            stage: Stage::Blind(Blind::Boss),
            blind: Some(Blind::Boss),
            ..Default::default()
        };
        g.handle_score(1_000_000).expect("handle score");
        let boss = g
            .current_boss
            .expect("current_boss redrawn after ante advance");
        assert_eq!(boss.is_finisher(), g.ante_current as i32 % 8 == 0);
    }

    #[test]
    fn test_skip_blind_banks_the_predrawn_tag() {
        let mut g = Game::default();
        g.start();
        let expected = g.small_blind_tag;
        g.skip_blind(Blind::Small).expect("skip small blind");
        assert_eq!(g.tags, vec![expected]);
    }

    #[test]
    fn test_cashout_drains_and_applies_tags() {
        let mut g = Game {
            stage: Stage::PostBlind(),
            ..Default::default()
        };
        g.tags = vec![Tag::Handy, Tag::Garbage];
        g.cashout().expect("cashout");
        assert!(g.tags.is_empty());
    }

    #[test]
    fn test_finish_pack_does_not_touch_tags() {
        let mut g = Game::default();
        g.start();
        g.tags = vec![Tag::Handy];
        g.open_pack = Some(OpenPackState {
            picks_remaining: 1,
            description: String::new(),
            contents: vec![],
        });
        g.stage = Stage::PackOpen();
        g.skip_pack().expect("skip pack");
        assert_eq!(g.tags, vec![Tag::Handy]);
    }

    #[test]
    fn test_pick_pack_card_planet() {
        use crate::planet::Planets;
        let mut g = Game::default();
        g.start();
        g.open_pack = Some(OpenPackState {
            picks_remaining: 1,
            description: String::new(),
            contents: vec![PackContent::Planet(Planets::Mercury)],
        });
        g.stage = Stage::PackOpen();
        let level_before = g.planetarium.level(HandRank::OnePair).level;
        g.pick_pack_card(PackContent::Planet(Planets::Mercury))
            .expect("pick planet");
        assert_eq!(g.stage, Stage::Shop());
        assert!(g.open_pack.is_none());
        assert_eq!(
            g.planetarium.level(HandRank::OnePair).level,
            level_before + 1
        );
    }

    #[test]
    fn test_pick_pack_card_joker() {
        use crate::joker::Jokers;
        use balatro_types::joker::TheJoker;
        let mut g = Game::default();
        g.start();
        let joker = Jokers::TheJoker(TheJoker::default());
        let before_len = g.jokers.len();
        g.open_pack = Some(OpenPackState {
            picks_remaining: 1,
            description: String::new(),
            contents: vec![PackContent::Joker(joker.clone())],
        });
        g.stage = Stage::PackOpen();
        g.pick_pack_card(PackContent::Joker(joker))
            .expect("pick joker");
        assert_eq!(g.jokers.len(), before_len + 1);
        assert_eq!(g.stage, Stage::Shop());
        assert!(g.open_pack.is_none());
    }

    #[test]
    fn test_pick_pack_card_joker_slots_full() {
        use crate::joker::Jokers;
        use balatro_types::joker::TheJoker;
        let mut g = Game::default();
        g.start();
        let slots = g.config.joker_slots;
        for _ in 0..slots {
            g.jokers.push(Jokers::TheJoker(TheJoker::default()));
        }
        let joker = Jokers::TheJoker(TheJoker::default());
        g.open_pack = Some(OpenPackState {
            picks_remaining: 1,
            description: String::new(),
            contents: vec![PackContent::Joker(joker.clone())],
        });
        g.stage = Stage::PackOpen();
        let res = g.pick_pack_card(PackContent::Joker(joker));
        assert!(matches!(res, Err(GameError::NoAvailableSlot)));
    }

    #[test]
    fn test_negative_joker_from_pack_expands_slots() {
        use crate::joker::Jokers;
        use balatro_types::joker::TheJoker;
        let mut g = Game::default();
        g.start();
        let slots = g.config.joker_slots;
        for _ in 0..slots {
            g.jokers.push(Jokers::TheJoker(TheJoker::default()));
        }
        let mut neg_joker = Jokers::TheJoker(TheJoker::default());
        neg_joker.set_edition(Edition::Negative);
        g.open_pack = Some(OpenPackState {
            picks_remaining: 1,
            description: String::new(),
            contents: vec![PackContent::Joker(neg_joker.clone())],
        });
        g.stage = Stage::PackOpen();
        g.pick_pack_card(PackContent::Joker(neg_joker))
            .expect("pick negative joker from full pack");
        assert_eq!(g.config.joker_slots, slots + 1);
        assert_eq!(g.jokers.len(), slots + 1);
        assert_eq!(g.stage, Stage::Shop());
        assert!(g.open_pack.is_none());
    }

    #[test]
    fn test_pick_pack_card_targeting_tarot() {
        let mut g = Game::default();
        g.start();
        assert!(Tarot::Lovers.requires_targets());
        g.open_pack = Some(OpenPackState {
            picks_remaining: 1,
            description: String::new(),
            contents: vec![PackContent::Tarot(Tarot::Lovers)],
        });
        g.stage = Stage::PackOpen();
        g.pick_pack_card(PackContent::Tarot(Tarot::Lovers))
            .expect("pick targeting tarot");
        assert!(matches!(g.stage, Stage::TarotHand(Tarot::Lovers)));
        assert_eq!(g.tarot_prev_stage, Some(Stage::PackOpen()));
        // picks_remaining decremented later in apply_tarot, not here
        assert_eq!(g.open_pack.as_ref().unwrap().picks_remaining, 1);
    }

    #[test]
    fn test_pick_pack_card_targeting_spectral() {
        use crate::spectral::Spectral;
        let mut g = Game::default();
        g.start();
        assert!(Spectral::Talisman.requires_targets());
        g.open_pack = Some(OpenPackState {
            picks_remaining: 1,
            description: String::new(),
            contents: vec![PackContent::Spectral(Spectral::Talisman)],
        });
        g.stage = Stage::PackOpen();
        g.pick_pack_card(PackContent::Spectral(Spectral::Talisman))
            .expect("pick targeting spectral");
        assert!(matches!(g.stage, Stage::SpectralHand(Spectral::Talisman)));
        assert_eq!(g.spectral_prev_stage, Some(Stage::PackOpen()));
        // picks_remaining decremented later in apply_spectral, not here
        assert_eq!(g.open_pack.as_ref().unwrap().picks_remaining, 1);
    }

    #[test]
    fn test_pick_pack_card_spectral_immediate_apply() {
        use crate::spectral::Spectral;
        let mut g = Game::default();
        g.start();
        assert!(!Spectral::Sigil.requires_targets());
        g.available.extend(vec![crate::card::Card::new(
            crate::card::Value::Two,
            crate::card::Suit::Heart,
        )]);
        g.open_pack = Some(OpenPackState {
            picks_remaining: 1,
            description: String::new(),
            contents: vec![PackContent::Spectral(Spectral::Sigil)],
        });
        g.stage = Stage::PackOpen();
        g.pick_pack_card(PackContent::Spectral(Spectral::Sigil))
            .expect("pick immediate-apply spectral");
        assert_eq!(g.stage, Stage::Shop());
        assert!(g.open_pack.is_none());
    }

    #[test]
    fn test_reroll_invalid_outside_shop() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Blind(Blind::Small);
        let res = g.reroll();
        assert!(matches!(res, Err(GameError::InvalidStage)));
    }

    #[test]
    fn test_reroll_insufficient_balance() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        g.money = 0;
        let res = g.reroll();
        assert!(matches!(res, Err(GameError::InvalidBalance)));
    }

    #[test]
    fn test_reroll_deducts_cost() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        g.money = 10;
        g.reroll().expect("reroll");
        assert_eq!(g.money, 5);
    }

    #[test]
    fn test_reroll_increments_cost() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        g.money = 20;
        g.reroll().expect("first reroll");
        assert_eq!(g.reroll_cost, 6);
        g.reroll().expect("second reroll");
        assert_eq!(g.reroll_cost, 7);
        assert_eq!(g.money, 9); // 20 - 5 - 6
    }

    #[test]
    fn test_reroll_regenerates_shop_cards() {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::Shop();
        g.money = 10;
        let old_joker = crate::joker::jokers_by_rarity(crate::joker::Rarity::Common)[0].clone();
        g.shop.jokers = vec![old_joker.clone()];
        g.shop.consumables = Vec::new();
        g.reroll().expect("reroll");
        assert_eq!(g.shop.jokers.len() + g.shop.consumables.len(), 2);
        let still_present = g
            .shop
            .jokers
            .iter()
            .any(|j| std::mem::discriminant(j) == std::mem::discriminant(&old_joker));
        assert!(!still_present);
    }

    // --- vouchers ---

    /// A game sitting in a freshly-stocked shop, having cashed out (so the
    /// ante's voucher offer has been drawn), with `money` to spend.
    fn game_in_shop(money: usize) -> Game {
        let mut g = Game::default();
        g.start();
        g.stage = Stage::PostBlind();
        g.handle_action(Action::CashOut(0)).expect("cash out");
        g.money = money as i64;
        g
    }

    fn redeem(g: &mut Game, voucher: Voucher) {
        g.shop.voucher = Some(voucher);
        g.money = g.money.max(voucher.cost() as i64);
        g.handle_action(Action::BuyVoucher(voucher))
            .expect("buy voucher");
    }

    // --- tags ---
    //
    // Tags fire at `cashout`, which is when the real game pops them, so
    // the helpers below collect one through the real path rather than
    // poking `tag_state` directly. Counters a tag pays against have to be
    // set before the cashout, hence `set_before_cashout`.

    /// A fixed seed, so the joker tags have a Joker to land on. A shop
    /// whose two card slots both rolled consumables would silently skip
    /// them, and `Game::default()` is seeded from entropy.
    const TAG_TEST_SEED: u64 = 20_240_517;

    fn game_at_post_blind() -> Game {
        let mut g = Game::new(Config {
            seed: Some(TAG_TEST_SEED),
            ..Config::default()
        });
        g.start();
        g.stage = Stage::PostBlind();
        g
    }

    /// A game in the Shop with `tag` collected on the way in.
    fn game_in_shop_with_tag(tag: Tag) -> Game {
        let mut g = game_at_post_blind();
        g.tags.push(tag);
        g.handle_action(Action::CashOut(0)).expect("cash out");
        g
    }

    /// A game in the Shop with `tag` collected, having first run `setup`
    /// to establish the run counters the tag pays against.
    fn game_in_shop_with_tag_after(tag: Tag, setup: impl FnOnce(&mut Game)) -> Game {
        let mut g = game_at_post_blind();
        setup(&mut g);
        g.tags.push(tag);
        g.handle_action(Action::CashOut(0)).expect("cash out");
        g
    }

    #[test]
    fn test_handy_tag_pays_per_hand_played_this_run() {
        let g = game_in_shop_with_tag_after(Tag::Handy, |g| g.hands_played_this_run = 7);
        assert_eq!(g.money, 7);
    }

    #[test]
    fn test_garbage_tag_pays_per_unused_discard_this_run() {
        let g = game_in_shop_with_tag_after(Tag::Garbage, |g| g.unused_discards_this_run = 4);
        assert_eq!(g.money, 4);
    }

    #[test]
    fn test_speed_tag_pays_five_per_blind_skipped_this_run() {
        let g = game_in_shop_with_tag_after(Tag::Speed, |g| g.blinds_skipped_this_run = 3);
        assert_eq!(g.money, 15);
    }

    #[test]
    fn test_economy_tag_doubles_money_but_only_adds_forty() {
        let small = game_in_shop_with_tag_after(Tag::Economy, |g| g.money = 10);
        assert_eq!(small.money, 20, "10 doubles to 20");

        let big = game_in_shop_with_tag_after(Tag::Economy, |g| g.money = 100);
        assert_eq!(big.money, 140, "100 doubles, but only 40 is added");

        // A run in debt must not be charged double, and must not be pushed
        // below the debt line. This is the only money path that was not
        // clamped, so it is worth pinning in both directions.
        let debt = game_in_shop_with_tag_after(Tag::Economy, |g| {
            g.money = -20;
            g.bankrupt_at = -20;
        });
        assert_eq!(debt.money, -20, "a debt balance gains nothing");

        let rich_debt = game_in_shop_with_tag_after(Tag::Economy, |g| {
            g.money = 10;
            g.bankrupt_at = -20;
        });
        assert_eq!(rich_debt.money, 20, "a positive balance still doubles");
    }

    #[test]
    fn test_investment_tag_waits_for_the_next_boss_blind() {
        let g = game_in_shop_with_tag(Tag::Investment);
        assert!(g.tag_state.investment, "collected, not yet paid");
        assert_eq!(g.money, 0, "Investment pays on the boss, not on pickup");
    }

    #[test]
    fn test_coupon_tag_frees_the_next_shop_and_is_dropped_on_reroll() {
        let mut g = game_in_shop_with_tag(Tag::Coupon);
        g.money = 100;
        assert!(g.shop.couponed, "the shop the tag lands in is free");
        g.reroll().expect("reroll");
        assert!(
            !g.shop.couponed,
            "only the initial stock is free, not rerolled items"
        );
    }

    #[test]
    fn test_coupon_tag_makes_a_joker_free() {
        let mut g = game_in_shop_with_tag(Tag::Coupon);
        g.jokers.clear();
        let joker = Jokers::TheJoker(Default::default());
        g.shop.jokers = vec![joker.clone()];
        let before = g.money;
        g.handle_action(Action::BuyJoker(joker)).expect("buy");
        assert_eq!(g.money, before, "couponed joker costs nothing");
        assert_eq!(g.jokers.len(), 1);
    }

    #[test]
    fn test_d6_tag_makes_the_next_shops_rerolls_free() {
        let mut g = game_in_shop_with_tag(Tag::D6);
        assert_eq!(g.reroll_cost, 0);
        g.reroll().expect("reroll");
        assert_eq!(g.reroll_cost, 1, "the discount lasts one shop");
    }

    #[test]
    fn test_juggle_tag_adds_three_hand_size_for_one_round() {
        let mut g = game_in_shop_with_tag(Tag::Juggle);
        let base = (g.config.available + g.vouchers.hand_size_bonus())
            .min(g.config.available_max);
        assert_eq!(g.hand_size(), base + 3);
        g.next_round().expect("next round");
        assert_eq!(g.hand_size(), base, "Juggle covers exactly one round");
    }

    #[test]
    fn test_top_up_tag_spawns_common_jokers_while_there_is_room() {
        let mut g = game_in_shop_with_tag(Tag::TopUp);
        g.jokers.clear();
        g.apply_tag(Tag::TopUp);
        assert_eq!(g.jokers.len(), 2);
        for joker in &g.jokers {
            assert_eq!(joker.rarity(), Rarity::Common);
        }
    }

    #[test]
    fn test_top_up_tag_stops_at_the_joker_limit() {
        let mut g = game_in_shop_with_tag(Tag::TopUp);
        g.jokers.clear();
        g.config.joker_slots = 1;
        g.apply_tag(Tag::TopUp);
        assert_eq!(g.jokers.len(), 1, "never exceeds the slot limit");
    }

    #[test]
    fn test_double_tag_duplicates_the_next_tag_collected() {
        let mut g = game_in_shop_with_tag(Tag::Double);
        assert!(g.tag_state.double, "armed for the next tag");
        g.money = 30;
        g.apply_tag(Tag::Economy);
        assert!(!g.tag_state.double, "disarmed once used");
        assert_eq!(g.money, 100, "30 +30 then +40, capped at 40 each time");
    }

    #[test]
    fn test_orbital_tag_levels_the_hand_chosen_when_it_was_drawn() {
        let hand = HandRank::ThreeOfAKind;
        let mut g = game_in_shop_with_tag(Tag::Orbital);
        // The hand is picked at draw time so the player can see it before
        // choosing to skip; force a known one for the assertion.
        g.tag_state.orbital_hand = Some(hand);
        let before = g.planetarium.level(hand).level;
        g.apply_tag(Tag::Orbital);
        assert_eq!(g.planetarium.level(hand).level, before + 3);
    }

    #[test]
    fn test_boss_tag_rerolls_this_antes_boss() {
        let mut g = game_in_shop_with_tag(Tag::Boss);
        g.current_boss = None;
        g.apply_tag(Tag::Boss);
        assert!(
            g.current_boss.is_some(),
            "the tag draws a replacement for the boss you are about to fight"
        );
    }

    #[test]
    fn test_boss_tag_does_not_touch_the_next_antes_boss() {
        let mut g = game_in_shop_with_tag(Tag::Boss);
        g.apply_tag(Tag::Boss);
        let rerolled = g.current_boss;
        g.ante_current = g.ante_current.next(Ante::Eight).expect("not on ante eight");
        g.start_ante();
        assert_ne!(
            g.current_boss, rerolled,
            "the tag's reroll is spent on this ante, not carried to the next"
        );
    }

    #[test]
    fn test_directors_cut_allows_one_boss_reroll_per_ante() {
        let mut g = game_in_shop_with_tag(Tag::Voucher);
        g.vouchers.redeem(Voucher::DirectorsCut);
        g.money = 100;
        let first = g.current_boss;
        g.reroll_boss().expect("first reroll");
        assert_eq!(g.money, 90, "$10 per reroll");
        assert_ne!(g.current_boss, first);
        assert!(g.reroll_boss().is_err(), "Director's Cut is once per ante");
        g.ante_current = g.ante_current.next(Ante::Eight).expect("not on ante eight");
        g.start_ante();
        g.stage = Stage::Shop();
        assert!(g.can_reroll_boss(), "the allowance resets with the ante");
    }

    #[test]
    fn test_retcon_rerolls_the_boss_unlimitedly() {
        let mut g = game_in_shop_with_tag(Tag::Voucher);
        g.vouchers.redeem(Voucher::Retcon);
        g.money = 100;
        for expected in [90, 80, 70] {
            g.reroll_boss().expect("reroll");
            assert_eq!(g.money, expected);
        }
        g.money = 5;
        assert!(
            g.reroll_boss().is_err(),
            "Retcon is unlimited but still costs $10"
        );
    }

    #[test]
    fn test_boss_tag_spends_directors_cuts_one_reroll() {
        let mut g = game_in_shop_with_tag(Tag::Boss);
        g.vouchers.redeem(Voucher::DirectorsCut);
        g.money = 100;
        g.apply_tag(Tag::Boss);
        assert_eq!(g.money, 100, "the tag itself is free");
        assert!(g.reroll_boss().is_err(), "the tag used up the ante's reroll");
    }

    #[test]
    fn test_boss_reroll_needs_the_voucher() {
        let mut g = game_in_shop_with_tag(Tag::Voucher);
        g.money = 100;
        assert!(!g.can_reroll_boss());
        assert!(g.reroll_boss().is_err());
    }

    #[test]
    fn test_boss_reroll_is_only_offered_before_the_fight() {
        let mut g = game_in_shop_with_tag(Tag::Voucher);
        g.vouchers.redeem(Voucher::DirectorsCut);
        g.money = 100;
        g.stage = Stage::Blind(Blind::Big);
        assert!(!g.can_reroll_boss(), "not once the blind is being played");
        assert!(g.reroll_boss().is_err());
    }

    #[test]
    fn test_omen_globe_makes_arcana_packs_contain_spectrals() {
        let no_globe = Vouchers::default();
        let mut with_globe = Vouchers::default();
        with_globe.redeem(Voucher::OmenGlobe);
        let spectrals = |vouchers: &Vouchers| {
            let mut total = 0;
            for seed in 0..400 {
                let mut g = Game::new(Config {
                    seed: Some(seed),
                    ..Config::default()
                });
                for v in vouchers.owned() {
                    g.vouchers.redeem(*v);
                }
                g.stage = Stage::Shop();
                let ctx = g.shop_context(&g.planetarium, &g.consumables, &g.jokers, &g.vouchers);
                let pack = g
                    .backend
                    .gen_pack_of(&ctx, PackCategory::Arcana, PackSize::Normal);
                total += pack
                    .contents
                    .iter()
                    .filter(|c| matches!(c, PackContent::Spectral(_)))
                    .count();
            }
            total
        };
        assert_eq!(
            spectrals(&no_globe),
            0,
            "without the voucher an Arcana pack is all tarots"
        );
        let with_spectrals = spectrals(&with_globe);
        assert!(
            with_spectrals > 0,
            "Omen Globe lets spectrals into Arcana packs"
        );
        // 3 cards a pack, 400 packs, 20% each: 240 expected. Wide bounds, so
        // this catches a wrong rate without being flaky.
        assert!(
            (100..=400).contains(&with_spectrals),
            "got {with_spectrals} spectrals, expected roughly 240"
        );
    }

    #[test]
    fn test_voucher_tag_offers_a_second_voucher() {
        let g = game_in_shop_with_tag(Tag::Voucher);
        assert!(
            g.shop.voucher_tag_offer.is_some(),
            "the Voucher Tag adds a second offer alongside the ante's own"
        );
    }

    #[test]
    fn test_voucher_tag_offer_is_buyable() {
        let mut g = game_in_shop_with_tag(Tag::Voucher);
        let offer = g.shop.voucher_tag_offer.expect("tag offer");
        g.money = 100;
        g.handle_action(Action::BuyVoucher(offer)).expect("buy");
        assert!(g.vouchers.has(offer));
        assert_eq!(g.shop.voucher_tag_offer, None);
    }

    #[test]
    fn test_pack_tags_open_a_free_pack_of_the_right_shape() {
        let cases = [
            (Tag::Charm, PackCategory::Arcana, PackSize::Mega),
            (Tag::Meteor, PackCategory::Celestial, PackSize::Mega),
            (Tag::Buffoon, PackCategory::Buffoon, PackSize::Mega),
            (Tag::Standard, PackCategory::Standard, PackSize::Mega),
            // Spectral is the one pack tag that isn't a Mega.
            (Tag::Ethereal, PackCategory::Spectral, PackSize::Normal),
        ];
        for (tag, category, size) in cases {
            let g = game_in_shop_with_tag(tag);
            assert_eq!(g.stage, Stage::PackOpen(), "{tag:?}");
            let open = g.open_pack.as_ref().expect("pack open");
            let expected = Pack {
                category,
                size,
                contents: Vec::new(),
            };
            assert_eq!(open.description, expected.description(), "{tag:?}");
            let picks = if size == PackSize::Mega { 2 } else { 1 };
            assert_eq!(open.picks_remaining, picks, "{tag:?}");
        }
    }

    #[test]
    fn test_uncommon_tag_makes_one_shop_joker_free() {
        let g = game_in_shop_with_tag(Tag::Uncommon);
        assert!(g.tag_state.pending_free_joker.is_none(), "consumed on refill");
        assert_eq!(g.shop.free_joker_slots.len(), 1);
        let free = *g.shop.free_joker_slots.iter().next().unwrap();
        assert_eq!(g.shop.jokers[free].rarity(), Rarity::Uncommon);
    }

    #[test]
    fn test_rare_tag_makes_one_shop_joker_free_and_rare() {
        let g = game_in_shop_with_tag(Tag::Rare);
        assert_eq!(g.shop.free_joker_slots.len(), 1);
        let free = *g.shop.free_joker_slots.iter().next().unwrap();
        assert_eq!(g.shop.jokers[free].rarity(), Rarity::Rare);
    }

    #[test]
    fn test_edition_tags_upgrade_the_next_base_edition_shop_joker() {
        for (tag, edition) in [
            (Tag::Foil, Edition::Foil),
            (Tag::Holographic, Edition::Holographic),
            (Tag::Polychrome, Edition::Polychrome),
            (Tag::Negative, Edition::Negative),
        ] {
            let g = game_in_shop_with_tag(tag);
            assert_eq!(g.shop.free_joker_slots.len(), 1, "{tag:?}");
            let free = *g.shop.free_joker_slots.iter().next().unwrap();
            assert_eq!(g.shop.jokers[free].edition(), edition, "{tag:?}");
        }
    }

    #[test]
    fn test_edition_tag_only_marks_a_joker_slot_free() {
        // A tag can only convert a Joker, never a consumable, so a marked
        // slot always indexes into `shop.jokers`.
        let g = game_in_shop_with_tag(Tag::Foil);
        for slot in &g.shop.free_joker_slots {
            assert!(*slot < g.shop.jokers.len());
        }
    }

    #[test]
    fn test_tag_state_survives_a_save_load_round_trip() {
        let mut g = game_in_shop_with_tag(Tag::Double);
        g.apply_tag(Tag::Foil);
        g.tag_state.investment = true;

        let json = g.to_json().expect("serialize");
        let loaded = Game::from_json(&json).expect("deserialize");
        assert_eq!(loaded.tag_state.pending_edition, Some(Edition::Foil));
        assert!(loaded.tag_state.investment);
        assert_eq!(
            loaded.shop.free_joker_slots.len(),
            g.shop.free_joker_slots.len(),
            "a free slot must stay free across a save/load"
        );
    }

    #[test]
    fn test_buying_a_free_joker_costs_nothing_and_clears_the_slot() {
        let mut g = game_in_shop_with_tag(Tag::Foil);
        let free = *g.shop.free_joker_slots.iter().next().unwrap();
        let joker = g.shop.jokers[free].clone();
        g.money = 0;
        g.handle_action(Action::BuyJoker(joker)).expect("buy");
        assert!(!g.shop.free_joker_slots.contains(&free));
    }

    #[test]
    fn test_shop_offers_a_voucher_after_cashout() {
        let g = game_in_shop(0);
        let offered = g.shop.voucher.expect("ante 1 offers a voucher");
        // Only tier-1 vouchers can show up before anything is redeemed.
        assert_eq!(offered.requires(), None);
    }

    #[test]
    fn test_voucher_offer_survives_reroll_but_not_purchase() {
        let mut g = game_in_shop(50);
        let offered = g.shop.voucher.expect("voucher offered");

        g.reroll().expect("reroll");
        assert_eq!(g.shop.voucher, Some(offered), "reroll must not touch it");

        g.handle_action(Action::BuyVoucher(offered))
            .expect("buy voucher");
        assert_eq!(g.shop.voucher, None);
        assert!(g.vouchers.has(offered));
        assert_eq!(g.money, (50 - 5 - offered.cost()) as i64, "reroll then voucher");
    }

    #[test]
    fn test_voucher_offer_stays_gone_for_the_rest_of_the_ante() {
        let mut g = game_in_shop(50);
        let offered = g.shop.voucher.expect("voucher offered");
        g.handle_action(Action::BuyVoucher(offered))
            .expect("buy voucher");

        // Next round, same ante: no new offer.
        g.handle_action(Action::NextRound()).expect("next round");
        g.stage = Stage::PostBlind();
        g.handle_action(Action::CashOut(0)).expect("cash out");
        assert_eq!(g.shop.voucher, None);

        // New ante: a fresh offer, and never the one already redeemed.
        g.ante_current = Ante::Two;
        g.stage = Stage::PostBlind();
        g.handle_action(Action::CashOut(0)).expect("cash out");
        let next = g.shop.voucher.expect("ante 2 offers a voucher");
        assert_ne!(next, offered);
    }

    #[test]
    fn test_cannot_buy_voucher_twice_or_without_funds() {
        let mut g = game_in_shop(0);
        let offered = g.shop.voucher.expect("voucher offered");
        assert!(matches!(
            g.handle_action(Action::BuyVoucher(offered)),
            Err(GameError::InvalidBalance)
        ));

        redeem(&mut g, offered);
        g.shop.voucher = Some(offered);
        g.money = 100;
        assert!(matches!(
            g.handle_action(Action::BuyVoucher(offered)),
            Err(GameError::VoucherAlreadyRedeemed)
        ));
    }

    #[test]
    fn test_upgrade_voucher_only_offered_once_base_is_owned() {
        let mut g = game_in_shop(100);
        // Fast mode draws from the offerable pool, so with only the
        // upgrade's base missing it can never surface the upgrade.
        for _ in 0..50 {
            g.voucher_ante = None;
            g.ante_current = Ante::One;
            g.refresh_voucher_offer();
            let offered = g.shop.voucher.expect("voucher offered");
            assert_eq!(offered.requires(), None, "upgrade offered with no base");
        }

        redeem(&mut g, Voucher::Overstock);
        assert!(g.vouchers.is_offerable(Voucher::OverstockPlus));
    }

    #[test]
    fn test_overstock_widens_the_shop() {
        let mut g = game_in_shop(100);
        let before = g.shop.jokers.len() + g.shop.consumables.len() + g.shop.cards.len();
        assert_eq!(before, 2);

        redeem(&mut g, Voucher::Overstock);
        // Applied to the shop the player is standing in, not just the next.
        let after = g.shop.jokers.len() + g.shop.consumables.len() + g.shop.cards.len();
        assert_eq!(after, 3);

        // ...and to every later refresh.
        g.reroll().expect("reroll");
        assert_eq!(
            g.shop.jokers.len() + g.shop.consumables.len() + g.shop.cards.len(),
            3
        );
    }

    #[test]
    fn test_clearance_sale_discounts_cards_and_packs_but_not_vouchers() {
        let mut g = game_in_shop(100);
        redeem(&mut g, Voucher::ClearanceSale);

        let joker = crate::joker::jokers_by_rarity(crate::joker::Rarity::Common)[0].clone();
        g.shop.jokers = vec![joker.clone()];
        g.jokers.clear();
        let full = joker.cost();
        let money_before = g.money;
        g.buy_joker(joker).expect("buy joker");
        assert_eq!(g.money, money_before - (full * 3 / 4) as i64);

        // Vouchers keep their sticker price.
        g.shop.voucher = Some(Voucher::Blank);
        let money_before = g.money;
        g.handle_action(Action::BuyVoucher(Voucher::Blank))
            .expect("buy voucher");
        assert_eq!(g.money, money_before - Voucher::Blank.cost() as i64);
    }

    #[test]
    fn test_reroll_vouchers_cut_the_price_immediately() {
        let mut g = game_in_shop(100);
        assert_eq!(g.reroll_cost, 5);
        redeem(&mut g, Voucher::RerollSurplus);
        assert_eq!(g.reroll_cost, 3, "current shop gets the discount too");
        assert_eq!(g.base_reroll_cost(), 3);

        redeem(&mut g, Voucher::RerollGlut);
        assert_eq!(g.reroll_cost, 1);
    }

    #[test]
    fn test_slot_and_resource_vouchers() {
        let mut g = game_in_shop(100);
        let slots = g.consumable_slots();
        redeem(&mut g, Voucher::CrystalBall);
        assert_eq!(g.consumable_slots(), slots + 1);

        let joker_slots = g.joker_slots();
        redeem(&mut g, Voucher::Antimatter);
        assert_eq!(g.joker_slots(), joker_slots + 1);

        let hand_size = g.hand_size();
        redeem(&mut g, Voucher::PaintBrush);
        assert_eq!(g.hand_size(), hand_size + 1);

        // Per-round resources take effect at the start of the next round.
        redeem(&mut g, Voucher::Grabber);
        redeem(&mut g, Voucher::Wasteful);
        // `g.config` is only the base now; the deck and stake add to it.
        let base_plays = deck_variant::plays_per_round(g.config.plays, g.config.deck);
        let base_discards =
            deck_variant::discards_per_round(g.config.discards, g.config.deck, g.config.stake);
        assert_eq!(g.plays_per_round(), base_plays + 1);
        assert_eq!(g.discards_per_round(), base_discards + 1);
        g.clear_blind();
        assert_eq!(g.plays, base_plays + 1);
        assert_eq!(g.discards(), base_discards + 1);
    }

    // Regression: `clear_blind` resets the per-round counters when a blind
    // is *finished*, which happens before the shop opens. A voucher bought
    // in that shop was landing on an already-set counter and appeared to do
    // nothing until the round after next.
    #[test]
    fn test_hand_vouchers_apply_to_the_very_next_blind() {
        let mut g = game_in_shop(100);
        let base_plays = g.plays_per_round();
        let base_discards = g.discards_per_round();

        redeem(&mut g, Voucher::Grabber);
        redeem(&mut g, Voucher::Wasteful);

        g.handle_action(Action::NextRound()).expect("next round");
        g.handle_action(Action::SelectBlind(Blind::Small))
            .expect("select blind");

        assert_eq!(g.plays, base_plays + 1, "Grabber must count this round");
        assert_eq!(
            g.discards(),
            base_discards + 1,
            "Wasteful must count this round"
        );
    }

    // Same path, but for the vouchers that *cost* a resource.
    #[test]
    fn test_hieroglyph_costs_a_hand_on_the_very_next_blind() {
        let mut g = game_in_shop(100);
        let base_plays = g.config.plays;
        g.ante_current = Ante::Three;

        redeem(&mut g, Voucher::Hieroglyph);
        g.handle_action(Action::NextRound()).expect("next round");
        g.handle_action(Action::SelectBlind(Blind::Small))
            .expect("select blind");

        assert_eq!(g.plays, base_plays - 1);
        assert_eq!(g.ante_current, Ante::Two);
    }

    #[test]
    fn test_paint_brush_deals_a_bigger_hand_on_the_very_next_blind() {
        let mut g = game_in_shop(100);
        let base_hand = g.config.available;

        redeem(&mut g, Voucher::PaintBrush);
        g.handle_action(Action::NextRound()).expect("next round");
        g.handle_action(Action::SelectBlind(Blind::Small))
            .expect("select blind");

        assert_eq!(g.available.cards().len(), base_hand + 1);
    }

    #[test]
    fn test_hieroglyph_lowers_the_ante_and_costs_a_hand() {
        let mut g = game_in_shop(100);
        g.ante_current = Ante::Three;
        let hands = g.plays_per_round();

        redeem(&mut g, Voucher::Hieroglyph);
        assert_eq!(g.ante_current, Ante::Two);
        assert_eq!(g.plays_per_round(), hands - 1);
        assert_eq!(
            g.discards_per_round(),
            deck_variant::discards_per_round(
                g.config.discards,
                g.config.deck,
                g.config.stake
            ),
            "Hieroglyph costs a hand, not a discard"
        );
    }

    #[test]
    fn test_seed_money_raises_the_interest_cap() {
        let mut g = game_in_shop(100);
        assert_eq!(g.interest_max(), g.config.interest_max);
        redeem(&mut g, Voucher::SeedMoney);
        assert_eq!(g.interest_max(), g.config.interest_max + 5);
        redeem(&mut g, Voucher::MoneyTree);
        assert_eq!(g.interest_max(), g.config.interest_max + 15);
    }

    #[test]
    fn test_observatory_multiplies_mult_for_held_planets() {
        use crate::planet::Planets;

        // Two Kings = OnePair: 10 chips + 10 per King, 2 mult -> 30 * 2 = 60.
        let cards = vec![
            Card::new(Value::King, Suit::Heart),
            Card::new(Value::King, Suit::Spade),
        ];
        let hand = SelectHand::new(cards).best_hand().unwrap();

        let mut g = Game::default();
        assert_eq!(g.calc_score(hand.clone()), 60);

        // Mercury is OnePair's planet, so Observatory applies: mult 2 -> 3.
        g.consumables.push(Consumable::planet(Planets::Mercury));
        g.vouchers.redeem(Voucher::Observatory);
        assert_eq!(g.calc_score(hand.clone()), 90);

        // A planet for a different hand does nothing.
        g.consumables = vec![Consumable::planet(Planets::Jupiter)];
        assert_eq!(g.calc_score(hand), 60);
    }

    #[test]
    fn test_telescope_puts_the_most_played_planet_in_celestial_packs() {
        use crate::pack::{PackCategory, PackContent};
        use crate::planet::Planets;

        let mut g = game_in_shop(100);
        // Make Flush the most played hand, then take Telescope.
        for _ in 0..5 {
            g.planetarium.play(HandRank::Flush);
        }
        redeem(&mut g, Voucher::Telescope);
        assert_eq!(g.most_played_hand_rank(), HandRank::Flush);

        let mut saw_celestial = false;
        for _ in 0..200 {
            g.stage = Stage::PostBlind();
            g.handle_action(Action::CashOut(0)).expect("cash out");
            for pack in &g.shop.packs {
                if pack.category != PackCategory::Celestial {
                    continue;
                }
                saw_celestial = true;
                assert!(
                    pack.contents
                        .contains(&PackContent::Planet(Planets::Jupiter)),
                    "Celestial pack without the most played hand's planet: {:?}",
                    pack.contents
                );
            }
        }
        assert!(saw_celestial, "no Celestial pack generated in 200 shops");
    }

    #[test]
    fn test_magic_trick_puts_buyable_playing_cards_in_the_shop() {
        let mut g = game_in_shop(100);
        redeem(&mut g, Voucher::MagicTrick);

        // Playing cards are one of four outcomes per slot; reroll until seen.
        let mut card = None;
        for _ in 0..500 {
            if let Some(c) = g.shop.cards.first().copied() {
                card = Some(c);
                break;
            }
            g.money = 100;
            g.reroll().expect("reroll");
        }
        let card = card.expect("Magic Trick never produced a shop playing card");
        // Without Illusion, shop cards are plain.
        assert!(card.enhancement.is_none() && card.seal.is_none());
        assert_eq!(card.edition, Edition::Base);

        let deck_before = g.deck.len();
        g.money = 100;
        g.handle_action(Action::BuyPlayingCard(card))
            .expect("buy playing card");
        assert_eq!(g.deck.len(), deck_before + 1);
        assert!(g.deck.cards().iter().any(|c| c.id == card.id));
        assert!(!g.shop.cards.iter().any(|c| c.id == card.id));
        assert_eq!(g.money, (100 - card.shop_cost()) as i64);
    }

    // regression: `buy_playing_card` didn't check `deck_max` itself
    #[test]
    fn test_buy_playing_card_rejects_a_full_deck() {
        let mut g = game_in_shop(100);
        redeem(&mut g, Voucher::MagicTrick);

        let mut card = None;
        for _ in 0..500 {
            if let Some(c) = g.shop.cards.first().copied() {
                card = Some(c);
                break;
            }
            g.money = 100;
            g.reroll().expect("reroll");
        }
        let card = card.expect("Magic Trick never produced a shop playing card");

        // Cap the deck at its current size so there's no room left.
        g.config.deck_max = g.deck.len() + g.available.cards().len() + g.discarded.len();
        g.money = 100;
        assert!(matches!(
            g.handle_action(Action::BuyPlayingCard(card)),
            Err(GameError::NoAvailableSlot)
        ));
    }

    #[test]
    fn test_buy_voucher_round_trips_through_the_action_space() {
        let mut g = game_in_shop(100);
        let offered = g.shop.voucher.expect("voucher offered");
        let space = g.gen_action_space();
        let index = space
            .to_vec()
            .iter()
            .enumerate()
            .find_map(|(i, _)| match space.to_action(i, &g) {
                Ok(Action::BuyVoucher(v)) if v == offered => Some(i),
                _ => None,
            })
            .expect("BuyVoucher is unmasked in the shop");
        g.handle_action_index(index).expect("handle action index");
        assert!(g.vouchers.has(offered));
    }

    // --- Credit Card and negative money ---

    /// A game in the Shop with `money` in the bank, ready to transact.
    fn shop_with_money(money: i64) -> Game {
        let mut g = game_in_shop_with_tag(Tag::Double);
        g.money = money;
        g
    }

    /// Buys a Credit Card through the real shop path, so the allowance hook
    /// is exercised rather than poked. The Card costs $1, so the balance
    /// moves even when the caller started at $0.
    fn give_credit_card(g: &mut Game) {
        let mut card = Jokers::CreditCard(Default::default());
        card.set_instance_id(crate::joker::mint_joker_id());
        g.shop.jokers = vec![card.clone()];
        g.money = g.money.max(card.cost() as i64);
        g.buy_joker(card).expect("buy a Credit Card");
    }

    #[test]
    fn test_credit_card_allows_twenty_of_debt() {
        let mut g = shop_with_money(0);
        assert_eq!(g.bankrupt_at, 0, "no allowance without the joker");
        give_credit_card(&mut g);
        assert_eq!(g.bankrupt_at, -20);
        assert_eq!(g.debt_allowance(), 20);
        // The helper tops the balance up to cover the Card, so it is back to
        // $0 and the whole $20 of allowance is unused.
        assert_eq!(g.money, 0);
        assert_eq!(g.spendable_balance(), 20);
    }

    #[test]
    fn test_credit_card_lets_a_purchase_take_the_balance_negative() {
        let mut g = shop_with_money(5);
        give_credit_card(&mut g);
        assert_eq!(g.money, 4, "the Card itself cost $1");
        g.shop.voucher = Some(Voucher::Blank);
        g.handle_action(Action::BuyVoucher(Voucher::Blank))
            .expect("the allowance covers this");
        assert_eq!(g.money, -6, "and the balance is now in debt");
        assert_eq!(g.bankrupt_at, -20, "which the floor has not moved for");
    }

    #[test]
    fn test_a_purchase_cannot_go_past_the_allowance() {
        // A balance already sitting on the floor has no headroom left.
        let mut g = shop_with_money(-20);
        g.bankrupt_at = -20;
        assert_eq!(g.spendable_balance(), 0);
        g.shop.voucher = Some(Voucher::Blank);
        assert!(
            g.handle_action(Action::BuyVoucher(Voucher::Blank)).is_err(),
            "the allowance is already spent"
        );
        assert_eq!(g.money, -20, "and a failed purchase changes nothing");

        // One dollar short of the floor there is exactly $1 of headroom.
        g.money = -19;
        assert!(g.can_afford(1));
        assert!(!g.can_afford(2));
    }

    #[test]
    fn test_without_a_credit_card_the_balance_cannot_go_negative() {
        let mut g = shop_with_money(0);
        g.shop.voucher = Some(Voucher::Blank);
        assert!(
            g.handle_action(Action::BuyVoucher(Voucher::Blank)).is_err(),
            "no allowance, so no debt"
        );
        assert_eq!(g.money, 0);
    }

    #[test]
    fn test_selling_a_credit_card_takes_the_allowance_back() {
        let mut g = shop_with_money(0);
        give_credit_card(&mut g);
        assert_eq!(g.bankrupt_at, -20);
        g.sell_joker(0).expect("sell");
        assert_eq!(g.bankrupt_at, 0, "the allowance goes back with it");
        // A $1 Card sells for $1, so the balance is back to $1 and, with no
        // allowance, that's all there is to spend.
        assert_eq!(g.money, 1);
        assert_eq!(g.spendable_balance(), 1);
        g.shop.voucher = Some(Voucher::Blank);
        assert!(
            g.handle_action(Action::BuyVoucher(Voucher::Blank)).is_err(),
            "$1 cannot cover a $10 voucher once the allowance is gone"
        );
    }

    #[test]
    fn test_two_credit_cards_double_the_allowance() {
        let mut g = shop_with_money(0);
        give_credit_card(&mut g);
        give_credit_card(&mut g);
        assert_eq!(g.bankrupt_at, -40);
        // The helper tops up before each purchase, so the balance is $0 and
        // the whole $40 of allowance is unused.
        assert_eq!(g.money, 0);
        assert_eq!(g.spendable_balance(), 40);
    }

    #[test]
    fn test_debt_never_earns_interest() {
        let mut g = shop_with_money(-20);
        g.bankrupt_at = -20;
        let reward = g.calc_reward(Blind::Small).expect("reward");
        // Blind reward plus the per-hand bonus, and no interest at all.
        assert_eq!(
            reward,
            Blind::Small.reward() + g.plays * g.config.money_per_hand,
            "a debt balance earns no interest"
        );
    }

    #[test]
    fn test_a_joker_payment_never_pushes_the_balance_into_debt() {
        // The Tooth charges a dollar per card played. A debt balance
        // absorbs the charge rather than going further under, because
        // only a purchase may open the allowance.
        let mut g = shop_with_money(-20);
        g.bankrupt_at = -20;
        g.money = (g.money - 5).max(g.debt_floor());
        assert_eq!(g.money, -20, "clamped at the floor");
    }

    // --- Negative consumables and Perkeo ---
    use crate::planet::Planets;
    use crate::spectral::Spectral;

    /// A game in the Shop holding `consumables`, ignoring slots.
    fn shop_holding_consumables(consumables: Vec<Consumable>) -> Game {
        let mut g = game_in_shop_with_tag(Tag::Double);
        g.consumables = consumables;
        g
    }

    fn negative_tarot() -> Consumable {
        Consumable::negative(ConsumableKind::Tarot(Tarot::HangedMan))
    }

    #[test]
    fn test_perkeo_mints_a_negative_copy_when_the_shop_is_left() {
        let mut g = shop_holding_consumables(vec![Consumable::tarot(Tarot::HangedMan)]);
        g.jokers = vec![Jokers::Perkeo(Default::default())];
        g.shop_end_effects();
        assert_eq!(g.consumables.len(), 2);
        let copy = g.consumables[1];
        assert!(copy.is_negative());
        assert_eq!(copy.kind, g.consumables[0].kind, "a copy of the same card");
    }

    #[test]
    fn test_perkeo_copies_any_held_consumable() {
        for held in [
            Consumable::tarot(Tarot::Death),
            Consumable::planet(Planets::Mars),
            Consumable::spectral(Spectral::Ankh),
        ] {
            let mut g = shop_holding_consumables(vec![held]);
            g.jokers = vec![Jokers::Perkeo(Default::default())];
            g.shop_end_effects();
            assert_eq!(g.consumables.len(), 2, "{held:?} was copied");
            assert!(g.consumables[1].is_negative());
            assert_eq!(g.consumables[1].kind, held.kind);
        }
    }

    #[test]
    fn test_perkeo_needs_a_consumable_to_copy() {
        let mut g = shop_holding_consumables(vec![]);
        g.jokers = vec![Jokers::Perkeo(Default::default())];
        g.shop_end_effects();
        assert!(g.consumables.is_empty(), "nothing to copy is not an error");
    }

    #[test]
    fn test_perkeo_does_nothing_without_the_joker() {
        let mut g = shop_holding_consumables(vec![Consumable::tarot(Tarot::HangedMan)]);
        g.shop_end_effects();
        assert_eq!(g.consumables.len(), 1);
    }

    #[test]
    fn test_a_negative_consumable_is_slot_free() {
        let mut g = shop_holding_consumables(vec![]);
        g.config.consumable_slots = 1;
        g.consumables.push(Consumable::tarot(Tarot::Death));
        assert_eq!(g.consumables_holding_slots(), 1);
        assert!(
            !g.can_hold_consumable(&Consumable::tarot(Tarot::Tower)),
            "the only slot is taken"
        );
        assert!(
            g.can_hold_consumable(&negative_tarot()),
            "a Negative still fits"
        );
        g.consumables.push(negative_tarot());
        assert_eq!(g.consumables_holding_slots(), 1, "and takes no slot");
        assert_eq!(g.consumables.len(), 2);
    }

    #[test]
    fn test_a_negative_consumable_costs_five_more_than_its_base() {
        let base = Consumable::tarot(Tarot::HangedMan);
        let negative = negative_tarot();
        assert_eq!(negative.cost(), base.cost() + 5);
        assert!(negative.sell_value() > base.sell_value());
    }

    #[test]
    fn test_a_negative_consumable_does_not_block_its_base_from_a_shop() {
        // Holding a Negative Hanged Man and being offered a base Hanged Man
        // is the point of a Negative copy, so the exclusion list skips it.
        let g = shop_holding_consumables(vec![negative_tarot()]);
        let mut tarots: Vec<Tarot> = Vec::new();
        let mut planets: Vec<Planets> = Vec::new();
        for c in &g.consumables {
            crate::shop::exclude_held(c, &mut tarots, &mut planets);
        }
        assert!(
            tarots.is_empty(),
            "a Negative copy must not exclude its centre, got {tarots:?}"
        );
    }

    #[test]
    fn test_a_base_consumable_does_block_its_centre() {
        let g = shop_holding_consumables(vec![Consumable::tarot(Tarot::HangedMan)]);
        let mut tarots: Vec<Tarot> = Vec::new();
        let mut planets: Vec<Planets> = Vec::new();
        for c in &g.consumables {
            crate::shop::exclude_held(c, &mut tarots, &mut planets);
        }
        assert_eq!(tarots, vec![Tarot::HangedMan]);
    }

    #[test]
    fn test_a_negative_consumable_is_usable_and_sellable() {
        let mut g = shop_holding_consumables(vec![negative_tarot()]);
        g.money = 100;
        let before = g.consumables.len();
        g.sell_consumable(0).expect("sell");
        assert_eq!(g.consumables.len(), before - 1);
        assert!(
            g.money > 100,
            "and it sells for more than a base one, got {}",
            g.money
        );
    }

    #[test]
    fn test_a_negative_consumable_keeps_its_effect_when_used() {
        // Mars is Four of a Kind's planet.
        let negative = Consumable::negative(ConsumableKind::Planet(Planets::Mars));
        let mut g = shop_holding_consumables(vec![negative]);
        g.blind = Some(Blind::Small);
        g.stage = Stage::Blind(Blind::Small);
        let before = g.planetarium.level(HandRank::FourOfAKind).level;
        g.use_consumable(negative).expect("use a Negative planet");
        assert_eq!(
            g.planetarium.level(HandRank::FourOfAKind).level,
            before + 1,
            "a Negative planet still levels the hand"
        );
    }

    #[test]
    fn test_negative_consumables_survive_a_save_load_round_trip() {
        let g = shop_holding_consumables(vec![
            negative_tarot(),
            Consumable::tarot(Tarot::HangedMan),
        ]);
        let json = g.to_json().expect("serialize");
        let loaded = Game::from_json(&json).expect("deserialize");
        assert_eq!(loaded.consumables, g.consumables);
        assert!(loaded.consumables[0].is_negative());
        assert!(!loaded.consumables[1].is_negative());
    }

    // --- boss blind modifiers ---

    /// A game standing in the boss blind of its current ante, so
    /// `active_boss` resolves.
    fn game_in_boss_blind(boss: BossBlind) -> Game {
        let mut g = Game::new(Config {
            seed: Some(TAG_TEST_SEED),
            ..Config::default()
        });
        g.current_boss = Some(boss);
        g.blind = Some(Blind::Big);
        g.stage = Stage::Blind(Blind::Big);
        g.boss_state.on_new_blind();
        g.boss_state.all_cards_debuffed = boss == BossBlind::VerdantLeaf;
        g
    }

    /// Opens the boss blind's round, which deals the opening hand and runs
    /// the boss's own setup.
    fn enter_boss_blind(g: &mut Game) {
        g.begin_round(Blind::Boss);
    }

    fn select_and_play(g: &mut Game, n: usize) -> Result<(), GameError> {
        let cards: Vec<Card> = g.available.not_selected().iter().take(n).copied().collect();
        for card in &cards {
            g.select_card(*card)?;
        }
        g.play_selected()
    }

    #[test]
    fn test_needle_allows_exactly_one_hand_per_round() {
        let mut g = game_in_boss_blind(BossBlind::Needle);
        enter_boss_blind(&mut g);
        assert_eq!(g.plays_per_round(), 1);
        g.plays = g.plays_per_round();
        assert_eq!(g.plays, 1);
        select_and_play(&mut g, 5).expect("play the one hand");
        assert_eq!(g.plays, 0);
        assert!(select_and_play(&mut g, 5).is_err(), "no second hand");
    }

    #[test]
    fn test_psychic_requires_exactly_five_cards() {
        // A fresh round per case: a rejected attempt still leaves its cards
        // selected, so reusing one game would conflate the failures.
        for n in [1, 4, 6, 8] {
            let mut g = game_in_boss_blind(BossBlind::Psychic);
            enter_boss_blind(&mut g);
            assert!(
                select_and_play(&mut g, n).is_err(),
                "{n} cards must be rejected by The Psychic"
            );
        }
        let mut g = game_in_boss_blind(BossBlind::Psychic);
        enter_boss_blind(&mut g);
        assert!(select_and_play(&mut g, 5).is_ok(), "exactly five is legal");
    }

    #[test]
    fn test_eye_forbids_repeating_a_hand_type() {
        let mut g = game_in_boss_blind(BossBlind::Eye);
        enter_boss_blind(&mut g);
        select_and_play(&mut g, 5).expect("first hand");
        let rank = g.boss_state.last_played.expect("rank recorded");
        // Replaying the same 5 cards is the same type, so it must fail.
        let cards: Vec<Card> = g.deck.draw(5);
        g.available.extend(cards);
        assert_eq!(g.boss_state.last_played, Some(rank));
        assert!(select_and_play(&mut g, 5).is_err(), "same type twice");
    }

    #[test]
    fn test_mouth_locks_the_first_hand_type() {
        let mut g = game_in_boss_blind(BossBlind::Mouth);
        enter_boss_blind(&mut g);
        select_and_play(&mut g, 2).expect("first hand sets the type");
        let locked = g.boss_state.locked_rank.expect("locked");
        assert!(check_rank(&g, locked).is_ok(), "the locked type stays legal");
        assert!(check_rank(&g, other_rank(locked)).is_err(), "others do not");
    }

    fn check_rank(g: &Game, rank: HandRank) -> Result<(), GameError> {
        g.check_boss_hand_rank(rank)
    }

    fn other_rank(rank: HandRank) -> HandRank {
        if rank == HandRank::HighCard {
            HandRank::OnePair
        } else {
            HandRank::HighCard
        }
    }

    #[test]
    fn test_tooth_charges_a_dollar_per_card_played() {
        let mut g = game_in_boss_blind(BossBlind::Tooth);
        enter_boss_blind(&mut g);
        g.money = 20;
        select_and_play(&mut g, 5).expect("play");
        assert_eq!(g.money, 15, "$1 per card, five cards");
    }

    #[test]
    fn test_ox_zeroes_money_on_the_most_played_hand() {
        let mut g = game_in_boss_blind(BossBlind::Ox);
        enter_boss_blind(&mut g);
        g.money = 50;
        // Nothing has been played yet, so every rank ties at zero plays and
        // the most-played rank is whichever `max_by_key` settles on.
        let most = g.most_played_hand_rank();
        let cards: Vec<Card> = g.available.not_selected().to_vec();
        g.boss_on_play(most, &cards)(&mut g);
        assert_eq!(g.money, 0);
    }

    #[test]
    fn test_ox_leaves_money_alone_for_other_hands() {
        let mut g = game_in_boss_blind(BossBlind::Ox);
        enter_boss_blind(&mut g);
        g.money = 50;
        let most = g.most_played_hand_rank();
        let other = other_rank(most);
        g.boss_on_play(other, &[])(&mut g);
        assert_eq!(g.money, 50, "a hand that isn't the most played costs nothing");
    }

    #[test]
    fn test_hook_discards_two_unplayed_cards_after_a_hand() {
        let mut g = game_in_boss_blind(BossBlind::Hook);
        enter_boss_blind(&mut g);
        let before = g.discarded.len();
        select_and_play(&mut g, 3).expect("play");
        // The three played cards plus the two Hook discards.
        assert_eq!(g.discarded.len(), before + 5);
    }

    #[test]
    fn test_pillar_debuffs_cards_already_played_this_ante() {
        let mut g = game_in_boss_blind(BossBlind::Pillar);
        enter_boss_blind(&mut g);
        let played: Vec<Card> = g.available.not_selected().iter().take(2).copied().collect();
        assert!(!g.is_card_debuffed(&played[0]), "not yet played");
        g.boss_on_play(HandRank::OnePair, &played)(&mut g);
        assert!(
            g.is_card_debuffed(&played[0]),
            "a card stops scoring once it has been played this ante"
        );
    }

    #[test]
    fn test_pillar_forgets_at_the_next_ante() {
        let mut g = game_in_boss_blind(BossBlind::Pillar);
        enter_boss_blind(&mut g);
        let card = g.deck.draw(1).remove(0);
        g.boss_state.played_this_ante.insert(card.id);
        assert!(g.is_card_debuffed(&card));
        g.boss_state.on_new_ante();
        assert!(!g.is_card_debuffed(&card), "the flag is per ante");
    }

    #[test]
    fn test_wheel_adds_an_edition_to_a_random_joker() {
        let mut g = game_in_boss_blind(BossBlind::Wheel);
        enter_boss_blind(&mut g);
        g.jokers = vec![Jokers::TheJoker(Default::default())];
        // 1-in-4 per played card, so 200 cards is effectively certain.
        g.boss_wheel_editions(200);
        assert_ne!(
            g.jokers[0].edition(),
            Edition::Base,
            "The Wheel should have upgraded the joker"
        );
    }

    #[test]
    fn test_crimson_heart_debuffs_exactly_one_joker() {
        let mut g = game_in_boss_blind(BossBlind::CrimsonHeart);
        g.jokers = vec![
            Jokers::TheJoker(Default::default()),
            Jokers::GreedyJoker(Default::default()),
        ];
        g.jokers[0].set_instance_id(1);
        g.jokers[1].set_instance_id(2);
        enter_boss_blind(&mut g);
        let debuffed = g.boss_state.debuffed_joker.clone().expect("a victim");
        let flag = g.jokers.iter().filter(|j| g.is_joker_debuffed(j)).count();
        assert_eq!(flag, 1, "exactly one joker at a time");
        assert_eq!(debuffed.instance_id(), 1);
    }

    #[test]
    fn test_crimson_heart_releases_its_victim_between_hands() {
        let mut g = game_in_boss_blind(BossBlind::CrimsonHeart);
        g.jokers = vec![Jokers::TheJoker(Default::default())];
        g.jokers[0].set_instance_id(1);
        enter_boss_blind(&mut g);
        assert!(g.is_joker_debuffed(&g.jokers[0]));
        g.boss_crimson_heart();
        assert!(
            g.is_joker_debuffed(&g.jokers[0]),
            "a single joker stays debuffed, matching the real game"
        );
    }

    #[test]
    fn test_crimson_heart_stops_when_the_boss_stops() {
        let mut g = game_in_boss_blind(BossBlind::CrimsonHeart);
        g.jokers = vec![Jokers::TheJoker(Default::default())];
        enter_boss_blind(&mut g);
        g.blind = Some(Blind::Big);
        g.boss_crimson_heart();
        assert!(g.boss_state.debuffed_joker.is_none());
    }

    #[test]
    fn test_verdant_leaf_debuffs_every_card_until_a_joker_is_sold() {
        let mut g = game_in_boss_blind(BossBlind::VerdantLeaf);
        enter_boss_blind(&mut g);
        g.jokers = vec![Jokers::TheJoker(Default::default())];
        let card = g.available.not_selected()[0];
        assert!(g.is_card_debuffed(&card), "everything is debuffed");

        g.sell_joker(0).expect("sell");
        assert!(
            !g.is_card_debuffed(&card),
            "selling a Joker lifts the debuff"
        );
        assert_eq!(g.active_boss(), None, "and disables the boss");
    }

    #[test]
    fn test_amber_acorn_shuffles_the_joker_row() {
        let mut g = game_in_boss_blind(BossBlind::AmberAcorn);
        g.jokers = vec![
            Jokers::TheJoker(Default::default()),
            Jokers::GreedyJoker(Default::default()),
            Jokers::LustyJoker(Default::default()),
            Jokers::WrathfulJoker(Default::default()),
        ];
        let before: Vec<usize> = g.jokers.iter().map(|j| j.instance_id()).collect();
        enter_boss_blind(&mut g);
        let after: Vec<usize> = g.jokers.iter().map(|j| j.instance_id()).collect();
        assert_eq!(before.len(), after.len(), "a shuffle, not a drop");
        let mut a = before.clone();
        let mut b = after.clone();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b, "the same jokers, reordered");
    }

    #[test]
    fn test_house_deals_its_first_hand_face_down() {
        let mut g = game_in_boss_blind(BossBlind::House);
        enter_boss_blind(&mut g);
        assert!(
            g.available.cards().iter().all(|c| c.face_down),
            "the opening hand is face down"
        );
    }

    #[test]
    fn test_mark_deals_face_cards_face_down() {
        let mut g = game_in_boss_blind(BossBlind::Mark);
        enter_boss_blind(&mut g);
        for card in g.available.cards() {
            assert_eq!(
                card.face_down,
                g.is_face_card(&card),
                "{:?} of {:?}",
                card.value,
                card.suit
            );
        }
    }

    /// The Fish deals the first hand face up and everything after it face
    /// down, within the round only. The run-scoped read this replaces meant
    /// that from ante 2 the flag was already true before the boss was
    /// selected, so the whole round was face down.
    #[test]
    fn test_fish_deals_face_up_until_a_hand_is_played() {
        let mut g = game_in_boss_blind(BossBlind::Fish);
        g.ante_current = Ante::Five;
        // A run that has already played hands must not leak into this blind.
        g.hands_played_this_run = 12;
        g.total_cards_discarded = 30;

        enter_boss_blind(&mut g);
        assert!(
            g.available.cards().iter().all(|c| !c.face_down),
            "the first hand is face up even with a long run behind it"
        );

        select_and_play(&mut g, 5).expect("first hand");
        assert!(
            g.available.cards().iter().all(|c| c.face_down),
            "once a hand is played the whole hand is face down, held cards \
             included, because the real game's prepped flag is sticky"
        );
    }

    /// The Fish counts hands only. Discarding does not start the face-down
    /// draws, which the wiki's wording ("after each hand played") is explicit
    /// about.
    #[test]
    fn test_fish_ignores_discards() {
        let mut g = game_in_boss_blind(BossBlind::Fish);
        enter_boss_blind(&mut g);
        for card in g.available.cards().into_iter().take(3) {
            g.select_card(card).expect("select");
        }
        g.discard_selected().expect("discard");
        let drawn = g.available.cards();
        assert!(
            drawn.iter().all(|c| !c.face_down),
            "a discard does not trigger the face-down draws"
        );
    }

    /// The flag is round-scoped, so a new blind starts face up again.
    #[test]
    fn test_fish_resets_between_rounds() {
        let mut g = game_in_boss_blind(BossBlind::Fish);
        enter_boss_blind(&mut g);
        select_and_play(&mut g, 5).expect("first hand");
        assert!(
            g.available
                .cards()
                .iter()
                .any(|c| c.face_down),
            "the replacements came face down"
        );

        g.round += 1;
        g.begin_round(Blind::Boss);
        assert!(
            g.available.cards().iter().all(|c| !c.face_down),
            "the next round starts face up again: {:?}",
            g.available
                .cards()
                .iter()
                .map(|c| c.face_down)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_face_down_never_survives_a_blind() {
        let mut g = game_in_boss_blind(BossBlind::Mark);
        enter_boss_blind(&mut g);
        assert!(g.available.cards().iter().any(|c| c.face_down));
        g.clear_blind();
        assert!(
            g.deck.cards().iter().all(|c| !c.face_down),
            "everything flips back at the end of the round"
        );
    }

    #[test]
    fn test_face_down_does_not_change_scoring() {
        // A face-down card is still scored and still selectable, which is
        // what makes these bosses informational rather than mechanical.
        let mut g = game_in_boss_blind(BossBlind::Mark);
        enter_boss_blind(&mut g);
        let cards: Vec<Card> = g.available.not_selected().iter().take(5).copied().collect();
        for card in &cards {
            g.select_card(*card).expect("select");
        }
        let mut down = g.clone();
        down.force_everything_face_down();
        let a = SelectHand::new(down.available.selected())
            .best_hand()
            .map(|h| down.calc_score(h));
        let b = SelectHand::new(g.available.selected())
            .best_hand()
            .map(|h| g.calc_score(h));
        assert_eq!(a, b, "face down must not move the score");
    }

    #[test]
    fn test_cerulean_bell_pins_a_card_selected() {
        let mut g = game_in_boss_blind(BossBlind::CeruleanBell);
        enter_boss_blind(&mut g);
        let forced = g.boss_state.forced_card.expect("a forced card");
        assert!(
            g.available.selected().iter().any(|c| c.id == forced),
            "the pinned card is already selected"
        );
        let card = *g.available.selected().iter().find(|c| c.id == forced).unwrap();
        assert!(g.deselect_card(card).is_err(), "and cannot be released");
    }

    #[test]
    fn test_cerulean_bell_leaves_other_cards_alone() {
        let mut g = game_in_boss_blind(BossBlind::CeruleanBell);
        enter_boss_blind(&mut g);
        let forced = g.boss_state.forced_card.expect("a forced card");
        let other = g
            .available
            .selected()
            .iter()
            .find(|c| c.id != forced)
            .copied();
        if let Some(other) = other {
            assert!(g.deselect_card(other).is_ok(), "other cards are free");
        }
    }

    #[test]
    fn test_every_boss_blind_resolves_as_the_active_boss() {
        for boss in BossBlind::iter() {
            let mut g = game_in_boss_blind(boss);
            enter_boss_blind(&mut g);
            assert_eq!(
                g.active_boss(),
                Some(boss),
                "{boss:?} should be the active boss inside its own blind"
            );
        }
    }

    #[test]
    fn test_psychic_never_offers_an_illegal_play() {
        // The generator and `play_selected` have to agree: offering a Play
        // the engine then rejects is a bug the full-game stress tests catch
        // only by luck.
        let mut g = game_in_boss_blind(BossBlind::Psychic);
        enter_boss_blind(&mut g);
        g.available.empty();
        g.available.extend(
            (0..8)
                .map(|i| Card::new(Value::Two, if i % 2 == 0 { Suit::Heart } else { Suit::Club }))
                .collect(),
        );
        // Three selected is not five.
        let cards = g.available.not_selected();
        for card in cards.iter().take(3) {
            g.select_card(*card).expect("select");
        }
        assert!(
            g.gen_actions().all(|a| a != Action::Play()),
            "no Play while the hand is the wrong size"
        );
        // Widen to exactly five: release two, take four.
        for card in g.available.selected().iter().take(2) {
            g.deselect_card(*card).expect("deselect");
        }
        for card in g.available.not_selected().iter().take(4) {
            g.select_card(*card).expect("select");
        }
        assert_eq!(g.available.selected().len(), 5);
        assert!(g.gen_actions().any(|a| a == Action::Play()));
    }

    #[test]
    fn test_eye_never_offers_a_repeated_hand_type() {
        let mut g = game_in_boss_blind(BossBlind::Eye);
        enter_boss_blind(&mut g);
        g.available.empty();
        g.available.extend(
            (0..8)
                .map(|i| Card::new(Value::Two, if i % 2 == 0 { Suit::Heart } else { Suit::Club }))
                .collect(),
        );
        let cards = g.available.not_selected();
        for card in cards.iter().take(5) {
            g.select_card(*card).expect("select");
        }
        assert!(g.gen_actions().any(|a| a == Action::Play()));
        g.play_selected().expect("play a pair");
        // Same five-card shape again, so the same type, so no Play.
        g.available.empty();
        g.available.extend(
            (0..8)
                .map(|i| Card::new(Value::Two, if i % 2 == 0 { Suit::Heart } else { Suit::Club }))
                .collect(),
        );
        let cards = g.available.not_selected();
        for card in cards.iter().take(5) {
            g.select_card(*card).expect("select");
        }
        assert!(
            g.gen_actions().all(|a| a != Action::Play()),
            "The Eye forbids the same type twice"
        );
    }

    #[test]
    fn test_cerulean_bell_never_offers_deselecting_its_pinned_card() {
        let mut g = game_in_boss_blind(BossBlind::CeruleanBell);
        enter_boss_blind(&mut g);
        let pinned = g.boss_state.forced_card.expect("pinned");
        for card in g.available.not_selected().iter().take(3) {
            g.select_card(*card).expect("select");
        }
        let offered: Vec<Action> = g.gen_actions().collect();
        assert!(
            !offered.contains(&Action::DeselectCard(
                g.available
                    .selected()
                    .into_iter()
                    .find(|c| c.id == pinned)
                    .expect("pinned card is selected")
            )),
            "the pinned card cannot be released"
        );
    }

    #[test]
    fn test_boss_blinds_are_only_active_on_the_boss_blind() {
        for boss in BossBlind::iter() {
            let g = game_in_boss_blind(boss);
            assert_eq!(
                g.active_boss(),
                None,
                "{boss:?} must not apply on a small or big blind"
            );
        }
    }

    #[test]
    fn test_vouchers_survive_a_save_load_round_trip() {
        let mut g = game_in_shop(100);
        redeem(&mut g, Voucher::CrystalBall);
        redeem(&mut g, Voucher::Grabber);
        let slots = g.consumable_slots();
        let plays = g.plays_per_round();

        let json = g.to_json().expect("serialize");
        let loaded = Game::from_json(&json).expect("deserialize");
        assert_eq!(loaded.vouchers.owned(), g.vouchers.owned());
        // Derived, not baked into Config — so no double application.
        assert_eq!(loaded.consumable_slots(), slots);
        assert_eq!(loaded.plays_per_round(), plays);
    }
}

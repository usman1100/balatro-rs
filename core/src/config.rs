use balatro_types::{DeckVariant, Stake};
#[cfg(feature = "python")]
use pyo3::prelude::*;

const DEFAULT_ROUND_START: usize = 0;
// The real base is 4 hands; the Blue Deck adds a fifth.
const DEFAULT_PLAYS: usize = 4;
// The real game's `get_starting_params` base is 3 discards; the Red Deck
// adds the fourth. Kept as the base so a deck's own modifier is not
// double-counted.
const DEFAULT_DISCARDS: usize = 3;
const DEFAULT_MONEY_START: usize = 0;
// Not enforced as a cap on the balance. The real game has no `money_max`:
// nothing in its source clamps the dollars, and money only ever goes up
// during a run. Kept as a field because the config and the python bindings
// expose it, and because a save may carry it.
const DEFAULT_MONEY_MAX: usize = 500;
const DEFAULT_REWARD_BASE: usize = 0;
const DEFAULT_MONEY_PER_HAND: usize = 1;
const DEFAULT_INTEREST_RATE: f32 = 0.2;
const DEFAULT_INTEREST_MAX: usize = 5;
const DEFAULT_BASE_MULT: usize = 0;
const DEFAULT_BASE_CHIPS: usize = 0;
const DEFAULT_BASE_SCORE: usize = 0;
const DEFAULT_ANTE_START: usize = 1;
const DEFAULT_ANTE_END: usize = 8;
const DEFAULT_JOKER_SLOTS: usize = 5;
const DEFAULT_JOKER_SLOTS_MAX: usize = 10;
const DEFAULT_AVAILABLE: usize = 8;
const DEFAULT_AVAILABLE_MAX: usize = 24; // arbitrary
const DEFAULT_STORE_CONSUMABLE_SLOTS_MAX: usize = 4;
const DEFAULT_DECK_MAX: usize = 100;
// Likewise unenforced: the real game has no discard-pile cap, the pile is
// bounded by the deck size anyway.
const DEFAULT_DISCARDED_MAX: usize = 100;
const DEFAULT_SELECTED_MAX: usize = 5;
const DEFAULT_CONSUMABLE_SLOTS: usize = 2;

/// Which RNG backend `Shop` generation uses. `Fast` is `core`'s original
/// `rand_chacha`-based generation (unchanged). `Real` is a byte-accurate
/// port of the real Balatro seed algorithm (`balatro-seed`), producing the
/// same shop/pack contents the real game would for a given seed. Only
/// affects shop-item/pack generation — deck shuffling, probability rolls,
/// and the skip-blind tag draw are unaffected either way.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RngMode {
    #[default]
    Fast,
    Real,
}

/// Every field is `serde(default)`, so a save written by an older build —
/// or a hand-edited one — loads with the missing keys filled in from
/// `Config::new` rather than failing outright. The TUI loads user-supplied
/// files, which makes partial saves a real case rather than a theoretical
/// one.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "python", pyclass)]
#[derive(Debug, Clone)]
pub struct Config {
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::round_start"))]
    pub round_start: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::plays"))]
    pub plays: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::discards"))]
    pub discards: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::money_start"))]
    pub money_start: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::money_max"))]
    pub money_max: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::reward_base"))]
    pub reward_base: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::money_per_hand"))]
    pub money_per_hand: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::interest_rate"))]
    pub interest_rate: f32,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::interest_max"))]
    pub interest_max: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::base_mult"))]
    pub base_mult: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::base_chips"))]
    pub base_chips: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::base_score"))]
    pub base_score: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::ante_start"))]
    pub ante_start: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::ante_end"))]
    pub ante_end: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::joker_slots"))]
    pub joker_slots: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::joker_slots_max"))]
    pub joker_slots_max: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::selected_max"))]
    pub selected_max: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::available"))]
    pub available: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::available_max"))]
    pub available_max: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::store_consumable_slots_max"))]
    pub store_consumable_slots_max: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::consumable_slots"))]
    pub consumable_slots: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::deck_max"))]
    pub deck_max: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::discarded_max"))]
    pub discarded_max: usize,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::seed"))]
    pub seed: Option<u64>,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::seed_str"))]
    pub seed_str: Option<String>,
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::rng_mode"))]
    pub rng_mode: RngMode,
    /// The starting deck, which decides the run's opening card set and
    /// modifier. Defaults to Red, which is a plain 52-card deck.
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::deck"))]
    pub deck: DeckVariant,
    /// The difficulty stake. Each stake's modifier stacks on the ones below
    /// it. Defaults to White, which adds nothing.
    #[cfg_attr(feature = "serde", serde(default = "field_defaults::stake"))]
    pub stake: Stake,
}

impl Config {
    pub fn new() -> Self {
        Config {
            round_start: DEFAULT_ROUND_START,
            plays: DEFAULT_PLAYS,
            discards: DEFAULT_DISCARDS,
            money_start: DEFAULT_MONEY_START,
            money_max: DEFAULT_MONEY_MAX,
            reward_base: DEFAULT_REWARD_BASE,
            money_per_hand: DEFAULT_MONEY_PER_HAND,
            interest_rate: DEFAULT_INTEREST_RATE,
            interest_max: DEFAULT_INTEREST_MAX,
            base_mult: DEFAULT_BASE_MULT,
            base_chips: DEFAULT_BASE_CHIPS,
            base_score: DEFAULT_BASE_SCORE,
            ante_start: DEFAULT_ANTE_START,
            ante_end: DEFAULT_ANTE_END,
            joker_slots: DEFAULT_JOKER_SLOTS,
            joker_slots_max: DEFAULT_JOKER_SLOTS_MAX,
            selected_max: DEFAULT_SELECTED_MAX,
            available: DEFAULT_AVAILABLE,
            available_max: DEFAULT_AVAILABLE_MAX,
            store_consumable_slots_max: DEFAULT_STORE_CONSUMABLE_SLOTS_MAX,
            consumable_slots: DEFAULT_CONSUMABLE_SLOTS,
            deck_max: DEFAULT_DECK_MAX,
            discarded_max: DEFAULT_DISCARDED_MAX,
            seed: None,
            seed_str: None,
            deck: DeckVariant::Red,
            stake: Stake::White,
            rng_mode: RngMode::default(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

/// Field-level defaults for `serde(default)`.
///
/// A field-level `default` builds that one field's value, and a bare
/// `usize::default()` would be 0 — which for `plays` or `discards` is a
/// different game rather than the same game with a missing key. These
/// mirror `Config::new` so a partial save round-trips to the default run.
mod field_defaults {
    pub(super) fn round_start() -> usize {
        super::DEFAULT_ROUND_START
    }
    pub(super) fn plays() -> usize {
        super::DEFAULT_PLAYS
    }
    pub(super) fn discards() -> usize {
        super::DEFAULT_DISCARDS
    }
    pub(super) fn money_start() -> usize {
        super::DEFAULT_MONEY_START
    }
    pub(super) fn money_max() -> usize {
        super::DEFAULT_MONEY_MAX
    }
    pub(super) fn reward_base() -> usize {
        super::DEFAULT_REWARD_BASE
    }
    pub(super) fn money_per_hand() -> usize {
        super::DEFAULT_MONEY_PER_HAND
    }
    pub(super) fn interest_rate() -> f32 {
        super::DEFAULT_INTEREST_RATE
    }
    pub(super) fn interest_max() -> usize {
        super::DEFAULT_INTEREST_MAX
    }
    pub(super) fn base_mult() -> usize {
        super::DEFAULT_BASE_MULT
    }
    pub(super) fn base_chips() -> usize {
        super::DEFAULT_BASE_CHIPS
    }
    pub(super) fn base_score() -> usize {
        super::DEFAULT_BASE_SCORE
    }
    pub(super) fn ante_start() -> usize {
        super::DEFAULT_ANTE_START
    }
    pub(super) fn ante_end() -> usize {
        super::DEFAULT_ANTE_END
    }
    pub(super) fn joker_slots() -> usize {
        super::DEFAULT_JOKER_SLOTS
    }
    pub(super) fn joker_slots_max() -> usize {
        super::DEFAULT_JOKER_SLOTS_MAX
    }
    pub(super) fn selected_max() -> usize {
        super::DEFAULT_SELECTED_MAX
    }
    pub(super) fn available() -> usize {
        super::DEFAULT_AVAILABLE
    }
    pub(super) fn available_max() -> usize {
        super::DEFAULT_AVAILABLE_MAX
    }
    pub(super) fn store_consumable_slots_max() -> usize {
        super::DEFAULT_STORE_CONSUMABLE_SLOTS_MAX
    }
    pub(super) fn consumable_slots() -> usize {
        super::DEFAULT_CONSUMABLE_SLOTS
    }
    pub(super) fn deck_max() -> usize {
        super::DEFAULT_DECK_MAX
    }
    pub(super) fn discarded_max() -> usize {
        super::DEFAULT_DISCARDED_MAX
    }
    pub(super) fn seed() -> Option<u64> {
        None
    }
    pub(super) fn seed_str() -> Option<String> {
        None
    }
    pub(super) fn deck() -> super::DeckVariant {
        super::DeckVariant::Red
    }
    pub(super) fn stake() -> super::Stake {
        super::Stake::White
    }
    pub(super) fn rng_mode() -> super::RngMode {
        super::RngMode::default()
    }
}

#[cfg(feature = "python")]
#[pymethods]
impl Config {
    #[new]
    fn py_new() -> Self {
        Self::default()
    }

    #[getter]
    fn get_ante_end(&mut self) -> usize {
        self.ante_end
    }

    #[setter]
    fn set_ante_end(&mut self, i: usize) {
        self.ante_end = i;
    }

    #[getter]
    fn get_plays(&mut self) -> usize {
        self.plays
    }

    #[setter]
    fn set_plays(&mut self, i: usize) {
        self.plays = i;
    }

    #[getter]
    fn get_discards(&mut self) -> usize {
        self.discards
    }

    #[setter]
    fn set_discards(&mut self, i: usize) {
        self.discards = i;
    }

    #[getter]
    fn get_selected_max(&mut self) -> usize {
        self.selected_max
    }

    #[setter]
    fn set_selected_max(&mut self, i: usize) {
        self.selected_max = i;
    }

    #[getter]
    fn get_deck_max(&mut self) -> usize {
        self.deck_max
    }

    #[setter]
    fn set_deck_max(&mut self, i: usize) {
        self.deck_max = i;
    }

    #[getter]
    fn get_discarded_max(&mut self) -> usize {
        self.discarded_max
    }

    #[setter]
    fn set_discarded_max(&mut self, i: usize) {
        self.discarded_max = i;
    }

    #[getter]
    fn get_available_max(&mut self) -> usize {
        self.available_max
    }

    #[setter]
    fn set_available_max(&mut self, i: usize) {
        self.available_max = i;
    }

    #[getter]
    fn get_joker_slots(&mut self) -> usize {
        self.joker_slots
    }

    #[setter]
    fn set_joker_slots(&mut self, i: usize) {
        self.joker_slots = i;
    }

    #[getter]
    fn get_joker_slots_max(&mut self) -> usize {
        self.joker_slots_max
    }

    #[setter]
    fn set_joker_slots_max(&mut self, i: usize) {
        self.joker_slots_max = i;
    }

    #[getter]
    fn get_money_max(&mut self) -> usize {
        self.money_max
    }

    #[setter]
    fn set_money_max(&mut self, i: usize) {
        self.money_max = i;
    }
    #[getter]
    fn get_consumable_slots(&mut self) -> usize {
        self.consumable_slots
    }

    #[setter]
    fn set_consumable_slots(&mut self, i: usize) {
        self.consumable_slots = i;
    }

    #[getter]
    fn get_stage_max(&self) -> usize {
        9
    }

    #[getter]
    fn get_seed(&self) -> Option<u64> {
        self.seed
    }

    #[setter]
    fn set_seed(&mut self, v: Option<u64>) {
        self.seed = v;
    }

    #[getter]
    fn get_seed_str(&self) -> Option<String> {
        self.seed_str.clone()
    }

    #[setter]
    fn set_seed_str(&mut self, v: Option<String>) {
        self.seed_str = v;
    }
}

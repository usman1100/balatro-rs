use crate::action::Action;
use crate::card::{Card, Edition, Enhancement, Seal, Suit, Value};
use crate::consumable::{Consumable, ConsumableKind};
use crate::error::GameError;
use crate::joker::{jokers_by_rarity, Jokers, Rarity};
use crate::pack::{Pack, PackCategory, PackContent, PackSize};
use crate::planet::{Planetarium, Planets};
use crate::rank::HandRank;
use crate::rng::{consumable_to_pack_content, Backend, GeneratedItem, RngBackend};
use crate::spectral::Spectral;
use crate::tag::TagState;
use crate::tarot::Tarot;
use crate::voucher::{discounted, Voucher, Vouchers};
use balatro_types::Stake;
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use strum::IntoEnumIterator;

/// Everything shop generation reads off the `Game`. Bundled into one
/// struct because vouchers made the parameter list unreadable — every
/// field here was already being threaded through positionally.
pub(crate) struct ShopContext<'a> {
    pub planetarium: &'a Planetarium,
    /// Consumables the player already holds — excluded from the roll so
    /// the shop doesn't offer a duplicate.
    pub held_consumables: &'a [Consumable],
    pub held_jokers: &'a [Jokers],
    pub vouchers: &'a Vouchers,
    pub prob_mult: u32,
    pub ante: i32,
    /// The run's stake, which gates which stickers may spawn at all.
    pub stake: Stake,
    /// Which hand Telescope should guarantee a Planet card for.
    pub most_played: HandRank,
}

impl ShopContext<'_> {
    /// Card slots the shop fills, before packs: 2 by default, more with
    /// Overstock.
    pub(crate) fn card_slots(&self) -> usize {
        DEFAULT_SHOP_CARD_SLOTS + self.vouchers.shop_card_slots_bonus()
    }

    /// The slice of the context that pack contents depend on.
    /// Sticker rolls are per-ante in the real game (`etperpoll{ante}`), so
    /// this is only read where an ante is in hand.
    pub(crate) fn sticker_rolls(&self, rng: &mut impl Rng) -> (f64, f64) {
        (rng.gen::<f64>(), rng.gen::<f64>())
    }

    pub(crate) fn pack_inputs(&self) -> PackInputs<'_> {
        PackInputs {
            jokers: self.held_jokers,
            omen_globe: self.vouchers.has(Voucher::OmenGlobe),
        }
    }

    /// Edition probabilities are scaled by both the game-wide probability
    /// multiplier and Hone/Glow Up.
    pub(crate) fn edition_prob_mult(&self) -> u32 {
        self.prob_mult
            .saturating_mul(self.vouchers.edition_rate_mult())
    }
}

/// The player's holdings a pack draw has to respect. A struct for the same
/// reason as `ShopContext`: the list grows with every voucher that touches
/// pack contents, and a positional list of it stops being readable.
#[derive(Copy, Clone)]
pub(crate) struct PackInputs<'a> {
    pub jokers: &'a [Jokers],
    /// Omen Globe: lets a Spectral stand in for a Tarot in an Arcana pack.
    pub omen_globe: bool,
}

#[cfg(test)]
impl<'a> ShopContext<'a> {
    /// Bare context for tests: nothing held, no vouchers redeemed.
    pub(crate) fn for_test(
        planetarium: &'a Planetarium,
        vouchers: &'a Vouchers,
        ante: i32,
    ) -> Self {
        ShopContext {
            planetarium,
            held_consumables: &[],
            held_jokers: &[],
            vouchers,
            prob_mult: 1,
            ante,
            stake: Stake::White,
            most_played: HandRank::HighCard,
        }
    }
}

const DEFAULT_SHOP_CARD_SLOTS: usize = 2;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Default)]
pub struct Shop {
    pub jokers: Vec<Jokers>,
    pub consumables: Vec<Consumable>,
    /// Playing cards for sale. Always empty until Magic Trick is redeemed.
    #[cfg_attr(feature = "serde", serde(default))]
    pub cards: Vec<Card>,
    pub packs: Vec<Pack>,
    /// This ante's voucher offer, if it hasn't been bought yet. Unlike
    /// everything else in the shop it survives rerolls and persists
    /// across every shop visit of the ante.
    #[cfg_attr(feature = "serde", serde(default))]
    pub voucher: Option<Voucher>,
    /// A second voucher slot, granted by the Voucher Tag. Never a reroll
    /// target.
    #[cfg_attr(feature = "serde", serde(default))]
    pub voucher_tag_offer: Option<Voucher>,
    /// Coupon Tag: every Joker, consumable and pack here is free. Set when
    /// the shop is stocked and dropped on reroll, matching the real game
    /// making only the *initial* stock free.
    #[cfg_attr(feature = "serde", serde(default))]
    pub couponed: bool,
    /// Indices into `jokers` that a tag made free, for the Uncommon, Rare
    /// and edition tags.
    #[cfg_attr(feature = "serde", serde(default))]
    pub free_joker_slots: std::collections::BTreeSet<usize>,
}

impl Shop {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Shop {
    /// Prices a shop item, honouring the Coupon Tag. `Game` routes every
    /// purchase through here so a couponed shop is uniformly $0.
    pub(crate) fn price(&self, game_price: impl Fn(usize) -> usize, cost: usize) -> usize {
        if self.couponed {
            0
        } else {
            game_price(cost)
        }
    }

    pub(crate) fn refresh_cards(
        &mut self,
        ctx: &ShopContext,
        backend: &mut Backend,
        tags: &mut TagState,
    ) {
        self.jokers.clear();
        self.consumables.clear();
        self.cards.clear();
        self.couponed = tags.coupon;
        self.free_joker_slots.clear();
        tags.coupon = false;

        let mut excl_jokers: Vec<Jokers> = ctx.held_jokers.to_vec();
        let mut excl_tarots: Vec<Tarot> = ctx
            .held_consumables
            .iter()
            .filter_map(|c| {
                if let ConsumableKind::Tarot(t) = &c.kind {
                    Some(*t)
                } else {
                    None
                }
            })
            .collect();
        let mut excl_planets: Vec<Planets> = ctx
            .held_consumables
            .iter()
            .filter_map(|c| {
                if let ConsumableKind::Planet(p) = &c.kind {
                    Some(*p)
                } else {
                    None
                }
            })
            .collect();

        for _ in 0..ctx.card_slots() {
            match backend.gen_shop_item(ctx, &excl_jokers, &excl_tarots, &excl_planets) {
                GeneratedItem::Joker(joker) => {
                    let joker = self.tag_joker(joker, ctx, backend, tags);
                    excl_jokers.push(joker.clone());
                    self.jokers.push(joker);
                }
                GeneratedItem::Consumable(c) => {
                    exclude_held(&c, &mut excl_tarots, &mut excl_planets);
                    self.consumables.push(c);
                }
                GeneratedItem::PlayingCard(card) => self.cards.push(card),
            }
        }
    }

    /// Applies whichever tag is waiting on this shop Joker, returning the
    /// (possibly rewritten) Joker.
    ///
    /// Both hooks are per-Joker in the real game, and the Uncommon/Rare one
    /// has to fire *before* generation: it forces the rarity roll rather
    /// than filtering afterwards, so the real game never spends that draw.
    /// `gen_shop_item` has already spent it by the time we get here, which
    /// is a real-mode accuracy gap.
    fn tag_joker(
        &mut self,
        mut joker: Jokers,
        ctx: &ShopContext,
        backend: &mut Backend,
        tags: &mut TagState,
    ) -> Jokers {
        if let Some(rarity) = tags.pending_free_joker.take() {
            let joker = backend.gen_joker_of_rarity(ctx.ante, ctx.edition_prob_mult(), &[], rarity);
            let slot = self.jokers.len();
            self.free_joker_slots.insert(slot);
            return joker;
        }
        if let Some(edition) = tags.pending_edition {
            // Only the next *base* edition Joker, so an already-rolled
            // edition tag stays pending for a later slot.
            if joker.edition() == Edition::Base {
                joker.set_edition(edition);
                let slot = self.jokers.len();
                self.free_joker_slots.insert(slot);
                tags.pending_edition = None;
            }
        }
        joker
    }

    /// Fills any card slots the shop has gained since it was stocked,
    /// leaving what's already on the shelf alone. Redeeming Overstock
    /// mid-shop is the only thing that widens a live shop.
    pub(crate) fn top_up_cards(
        &mut self,
        ctx: &ShopContext,
        backend: &mut Backend,
        tags: &mut TagState,
    ) {
        let filled = self.jokers.len() + self.consumables.len() + self.cards.len();
        let mut excl_jokers: Vec<Jokers> = ctx.held_jokers.to_vec();
        excl_jokers.extend(self.jokers.clone());
        let mut excl_tarots: Vec<Tarot> = Vec::new();
        let mut excl_planets: Vec<Planets> = Vec::new();
        for c in ctx.held_consumables.iter().chain(self.consumables.iter()) {
            exclude_held(c, &mut excl_tarots, &mut excl_planets);
        }

        for _ in filled..ctx.card_slots() {
            match backend.gen_shop_item(ctx, &excl_jokers, &excl_tarots, &excl_planets) {
                GeneratedItem::Joker(joker) => {
                    let joker = self.tag_joker(joker, ctx, backend, tags);
                    excl_jokers.push(joker.clone());
                    self.jokers.push(joker);
                }
                GeneratedItem::Consumable(c) => {
                    exclude_held(&c, &mut excl_tarots, &mut excl_planets);
                    self.consumables.push(c);
                }
                GeneratedItem::PlayingCard(card) => self.cards.push(card),
            }
        }
    }

    pub(crate) fn refresh(
        &mut self,
        ctx: &ShopContext,
        backend: &mut Backend,
        tags: &mut TagState,
    ) {
        self.refresh_cards(ctx, backend, tags);

        let p1 = backend.gen_pack(ctx, None);
        let exclude = (&p1.category, &p1.size);
        let p2 = backend.gen_pack(ctx, Some(exclude));
        self.packs = vec![p1, p2];
        for pack in &mut self.packs {
            apply_telescope(pack, ctx);
        }
    }

    pub(crate) fn joker_from_index(&self, i: usize) -> Option<Jokers> {
        Some(self.jokers[i].clone())
    }

    pub(crate) fn consumable_from_index(&self, i: usize) -> Option<Consumable> {
        self.consumables.get(i).cloned()
    }

    pub(crate) fn pack_from_index(&self, i: usize) -> Option<Pack> {
        self.packs.get(i).cloned()
    }

    pub(crate) fn card_from_index(&self, i: usize) -> Option<Card> {
        self.cards.get(i).copied()
    }

    pub(crate) fn buy_joker(&mut self, joker: &Jokers) -> Result<Jokers, GameError> {
        let i = self
            .jokers
            .iter()
            .position(|j| j == joker)
            .ok_or(GameError::NoJokerMatch)?;
        Ok(self.jokers.remove(i))
    }

    pub(crate) fn buy_consumable(
        &mut self,
        consumable: &Consumable,
    ) -> Result<Consumable, GameError> {
        let i = self
            .consumables
            .iter()
            .position(|c| c == consumable)
            .ok_or(GameError::NoConsumableMatch)?;
        Ok(self.consumables.remove(i))
    }

    pub(crate) fn buy_pack(&mut self, pack: &Pack) -> Result<Pack, GameError> {
        let i = self
            .packs
            .iter()
            .position(|p| p == pack)
            .ok_or(GameError::InvalidAction)?;
        Ok(self.packs.remove(i))
    }

    pub(crate) fn buy_card(&mut self, card: &Card) -> Result<Card, GameError> {
        let i = self
            .cards
            .iter()
            .position(|c| c.id == card.id)
            .ok_or(GameError::NoCardMatch)?;
        Ok(self.cards.remove(i))
    }

    /// Takes the offered voucher off the shelf. Unlike the other buys it
    /// clears the slot rather than removing from a list — one voucher per
    /// ante, plus a second while a Voucher Tag is outstanding.
    pub(crate) fn buy_voucher(&mut self, voucher: &Voucher) -> Result<Voucher, GameError> {
        if self.voucher == Some(*voucher) {
            self.voucher = None;
            return Ok(*voucher);
        }
        if self.voucher_tag_offer == Some(*voucher) {
            self.voucher_tag_offer = None;
            return Ok(*voucher);
        }
        Err(GameError::NoVoucherMatch)
    }

    pub(crate) fn gen_moves_buy_joker(
        &self,
        balance: usize,
        vouchers: &Vouchers,
    ) -> Option<impl Iterator<Item = Action>> {
        if self.jokers.is_empty() {
            return None;
        }
        let discount = vouchers.discount_pct();
        let buys = self
            .jokers
            .clone()
            .into_iter()
            .filter(move |j| discounted(j.cost(), discount) <= balance)
            .map(Action::BuyJoker);
        Some(buys)
    }

    pub(crate) fn gen_moves_buy_consumable(
        &self,
        balance: usize,
        consumable_slots: usize,
        held: usize,
        vouchers: &Vouchers,
    ) -> Option<impl Iterator<Item = Action>> {
        if self.consumables.is_empty() {
            return None;
        }
        // A Negative consumable is slot-free, so it is still offerable when
        // the slots are full.
        if held >= consumable_slots && self.consumables.iter().all(|c| c.takes_a_slot()) {
            return None;
        }
        let discount = vouchers.discount_pct();
        let buys = self
            .consumables
            .clone()
            .into_iter()
            .filter(move |c| discounted(c.cost(), discount) <= balance)
            .map(Action::BuyConsumable);
        Some(buys)
    }

    pub(crate) fn gen_moves_buy_pack(
        &self,
        balance: usize,
        vouchers: &Vouchers,
    ) -> Option<impl Iterator<Item = Action>> {
        if self.packs.is_empty() {
            return None;
        }
        let discount = vouchers.discount_pct();
        let buys = self
            .packs
            .clone()
            .into_iter()
            .filter(move |p| discounted(p.cost(), discount) <= balance)
            .map(Action::BuyPack);
        Some(buys)
    }

    pub(crate) fn gen_moves_buy_card(
        &self,
        balance: usize,
        vouchers: &Vouchers,
    ) -> Option<impl Iterator<Item = Action>> {
        if self.cards.is_empty() {
            return None;
        }
        let discount = vouchers.discount_pct();
        let buys = self
            .cards
            .clone()
            .into_iter()
            .filter(move |c| discounted(c.shop_cost(), discount) <= balance)
            .map(Action::BuyPlayingCard);
        Some(buys)
    }

    pub(crate) fn gen_moves_buy_voucher(
        &self,
        balance: usize,
    ) -> Option<impl Iterator<Item = Action>> {
        // Vouchers are never discounted, so no `Vouchers` argument here.
        let affordable = [self.voucher, self.voucher_tag_offer]
            .into_iter()
            .flatten()
            .filter(|v| v.cost() <= balance)
            .map(Action::BuyVoucher)
            .collect::<Vec<_>>();
        if affordable.is_empty() {
            return None;
        }
        Some(affordable.into_iter())
    }
}

/// Records a held consumable's centre so the shop doesn't re-offer it.
///
/// A Negative copy deliberately does *not* exclude: holding a Negative
/// Hanged Man and then being offered a base Hanged Man is the entire point
/// of the Negativer, and of Perkeo minting one. The real game's shop
/// exclusion builds its list from held card centres, which a Negative copy
/// shares, so this is the one place the reading is a judgement call rather
/// than a transcription — the 1.0.0 dump doesn't contain the shop
/// generation function.
pub(crate) fn exclude_held(c: &Consumable, tarots: &mut Vec<Tarot>, planets: &mut Vec<Planets>) {
    if c.is_negative() {
        return;
    }
    match c.kind {
        ConsumableKind::Tarot(t) => tarots.push(t),
        ConsumableKind::Planet(p) => planets.push(p),
        ConsumableKind::Spectral(_) => {}
    }
}

/// Telescope guarantees the most played hand's Planet card in every
/// Celestial Pack. Applied after generation so it works the same for both
/// RNG backends.
fn apply_telescope(pack: &mut Pack, ctx: &ShopContext) {
    if !ctx.vouchers.telescope() || pack.category != PackCategory::Celestial {
        return;
    }
    let Some(wanted) = Planets::iter().find(|p| p.hand_rank() == ctx.most_played) else {
        return;
    };
    if pack
        .contents
        .iter()
        .any(|c| matches!(c, PackContent::Planet(p) if *p == wanted))
    {
        return;
    }
    if let Some(first) = pack.contents.first_mut() {
        *first = PackContent::Planet(wanted);
    }
}

pub(crate) fn gen_edition(prob_mult: u32, rng: &mut impl Rng, allow_negative: bool) -> Edition {
    if allow_negative && rng.gen_ratio(3u32.saturating_mul(prob_mult).min(1000), 1000) {
        return Edition::Negative;
    }
    if rng.gen_ratio(3u32.saturating_mul(prob_mult).min(1000), 1000) {
        return Edition::Polychrome;
    }
    if rng.gen_ratio(14u32.saturating_mul(prob_mult).min(1000), 1000) {
        return Edition::Holographic;
    }
    if rng.gen_ratio(20u32.saturating_mul(prob_mult).min(1000), 1000) {
        return Edition::Foil;
    }
    Edition::Base
}

/// A shop playing card (Magic Trick). Plain unless Illusion is also
/// redeemed, which is the only thing that lets it roll an enhancement,
/// edition, or seal.
pub(crate) fn gen_shop_playing_card(prob_mult: u32, illusion: bool, rng: &mut impl Rng) -> Card {
    if illusion {
        return gen_random_playing_card(prob_mult, rng, false, None);
    }
    let values: Vec<Value> = Value::iter().collect();
    let suits: Vec<Suit> = Suit::iter().collect();
    let v = values[rng.gen_range(0..values.len())];
    let s = suits[rng.gen_range(0..suits.len())];
    Card::new(v, s)
}

pub(crate) fn gen_random_playing_card(
    prob_mult: u32,
    rng: &mut impl Rng,
    force_enhance: bool,
    force_values: Option<&[Value]>,
) -> Card {
    let values: Vec<Value> = Value::iter()
        .filter(|v| force_values.is_none_or(|fv| fv.contains(v)))
        .collect();
    let suits: Vec<Suit> = Suit::iter().collect();
    let v = values[rng.gen_range(0..values.len())];
    let s = suits[rng.gen_range(0..suits.len())];
    let mut card = Card::new(v, s);

    if rng.gen_ratio(1, 5) || force_enhance {
        const ENHANCEMENTS: [Enhancement; 8] = [
            Enhancement::Bonus,
            Enhancement::Mult,
            Enhancement::Wild,
            Enhancement::Glass,
            Enhancement::Steel,
            Enhancement::Stone,
            Enhancement::Gold,
            Enhancement::Lucky,
        ];
        card.enhancement = Some(ENHANCEMENTS[rng.gen_range(0..ENHANCEMENTS.len())]);
    }

    card.edition = gen_edition(prob_mult, rng, false);

    if rng.gen_ratio(1, 10) {
        const SEALS: [Seal; 4] = [Seal::Gold, Seal::Red, Seal::Blue, Seal::Purple];
        card.seal = Some(SEALS[rng.gen_range(0..SEALS.len())]);
    }

    card
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub(crate) struct JokerGenerator {}

impl JokerGenerator {
    pub(crate) fn new() -> Self {
        JokerGenerator {}
    }

    // Randomly generate rarity of new joker.
    // 70% chance Common, 25% chance Uncommon, 5% chance Rare.
    // Legendary can only appear from Soul Spectral Card.
    fn gen_rarity(&self, rng: &mut impl Rng) -> Rarity {
        let choices = [Rarity::Common, Rarity::Uncommon, Rarity::Rare];
        let weights = [70u32, 25, 5];
        let dist = WeightedIndex::new(weights).unwrap();
        choices[dist.sample(rng)]
    }

    pub(crate) fn gen_joker(
        &self,
        prob_mult: u32,
        exclude: &[Jokers],
        rng: &mut impl Rng,
    ) -> Jokers {
        let rarity = self.gen_rarity(rng);
        self.gen_joker_of_rarity(prob_mult, exclude, rarity, rng)
    }

    pub(crate) fn gen_joker_of_rarity(
        &self,
        prob_mult: u32,
        exclude: &[Jokers],
        rarity: Rarity,
        rng: &mut impl Rng,
    ) -> Jokers {
        let all = jokers_by_rarity(rarity);
        let choices: Vec<_> = all
            .iter()
            .filter(|j| {
                !exclude
                    .iter()
                    .any(|e| std::mem::discriminant(e) == std::mem::discriminant(*j))
            })
            .cloned()
            .collect();
        let choices = if choices.is_empty() { all } else { choices };
        let i = rng.gen_range(0..choices.len());
        let mut joker = choices[i].clone();
        joker.set_edition(gen_edition(prob_mult, rng, true));
        joker.set_instance_id(crate::joker::mint_joker_id());
        crate::joker::roll_discard_selector(rng, &mut joker);
        joker
    }

    pub(crate) fn clone_joker(&self, j: Jokers, rng: &mut impl Rng) -> Jokers {
        let mut joker = j.to_owned();
        joker.set_instance_id(crate::joker::mint_joker_id());
        crate::joker::roll_discard_selector(rng, &mut joker);
        joker
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub(crate) struct ConsumableGenerator {}

impl ConsumableGenerator {
    pub(crate) fn new() -> Self {
        ConsumableGenerator {}
    }

    /// Generate a random planet, excluding secret planets unless their hand has been played,
    /// and excluding any already-picked planets.
    pub(crate) fn gen_planet(
        &self,
        planetarium: &Planetarium,
        exclude: &[Planets],
        rng: &mut impl Rng,
    ) -> Planets {
        let available: Vec<Planets> = Planets::iter()
            .filter(|p| {
                if exclude.contains(p) {
                    return false;
                }
                if p.is_secret() {
                    planetarium.level(p.hand_rank()).plays > 0
                } else {
                    true
                }
            })
            .collect();
        if available.is_empty() {
            // Every planet was excluded (or all the rest are secret and
            // undiscovered). Retry without the exclusion list rather than
            // recursing, which is one stack frame per retry.
            let all: Vec<Planets> = Planets::iter()
                .filter(|p| !p.is_secret() || planetarium.level(p.hand_rank()).plays > 0)
                .collect();
            if all.is_empty() {
                // Only reachable on a planetarium where every planet is
                // secret and none has been played. Fall back to the whole
                // set rather than panicking on an empty index.
                let every: Vec<Planets> = Planets::iter().collect();
                return every[rng.gen_range(0..every.len())];
            }
            return all[rng.gen_range(0..all.len())];
        }
        let i = rng.gen_range(0..available.len());
        available[i]
    }

    pub(crate) fn gen_planet_consumable(
        &self,
        planetarium: &Planetarium,
        exclude: &[Planets],
        rng: &mut impl Rng,
    ) -> Consumable {
        Consumable::planet(self.gen_planet(planetarium, exclude, rng))
    }

    fn gen_tarot(&self, exclude: &[Tarot], rng: &mut impl Rng) -> Tarot {
        let all: Vec<Tarot> = Tarot::iter().collect();
        let choices: Vec<_> = all.iter().filter(|t| !exclude.contains(t)).collect();
        let choices = if choices.is_empty() {
            all.iter().collect()
        } else {
            choices
        };
        let i = rng.gen_range(0..choices.len());
        *choices[i]
    }

    pub(crate) fn gen_tarot_consumable(&self, exclude: &[Tarot], rng: &mut impl Rng) -> Consumable {
        Consumable::tarot(self.gen_tarot(exclude, rng))
    }

    /// A spectral for pack generation. Rare variants never appear in packs,
    /// so they are out of the pool regardless of `exclude`.
    fn gen_spectral(&self, exclude: &[Spectral], rng: &mut impl Rng) -> Spectral {
        let all: Vec<Spectral> = Spectral::iter().filter(|s| !s.is_rare()).collect();
        let choices: Vec<_> = all.iter().filter(|s| !exclude.contains(s)).collect();
        // A Mega pack wants more cards than the pool holds, so falling back to
        // the full set duplicates rather than panicking. The real game gets
        // the same effect from releasing its locks as the pack fills.
        let choices = if choices.is_empty() {
            all.iter().collect()
        } else {
            choices
        };
        let i = rng.gen_range(0..choices.len());
        *choices[i]
    }

    pub(crate) fn gen_spectral_consumable(
        &self,
        exclude: &[Spectral],
        rng: &mut impl Rng,
    ) -> Consumable {
        Consumable::spectral(self.gen_spectral(exclude, rng))
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub(crate) struct PackGenerator {}

impl PackGenerator {
    // Weighted random pack selection per spec (real per-mille weights from
    // balatro-seed/src/pools.rs's PACKS table, scaled *100 so every category,
    // including Spectral's 0.07 Mega weight, is an exact integer):
    // Standard/Arcana/Celestial: Normal=4.0, Jumbo=2.0, Mega=0.5 -> 400,200,50
    // Buffoon: Normal=1.2, Jumbo=0.6, Mega=0.15 -> 120,60,15
    // Spectral: Normal=0.6, Jumbo=0.3, Mega=0.07 -> 60,30,7
    /// Skips the weighted category/size roll, for the pack tags which name
    /// both outright.
    pub(crate) fn gen_pack_of(
        &self,
        planetarium: &Planetarium,
        prob_mult: u32,
        category: PackCategory,
        size: PackSize,
        held: PackInputs<'_>,
        rng: &mut impl Rng,
    ) -> Pack {
        let count = balatro_seed::pack_card_count(category, size) as usize;
        let contents = self.gen_contents(&category, count, planetarium, prob_mult, held, rng);
        Pack {
            category,
            size,
            contents,
        }
    }

    pub(crate) fn gen_pack(
        &self,
        planetarium: &Planetarium,
        prob_mult: u32,
        exclude: Option<(&PackCategory, &PackSize)>,
        held: PackInputs<'_>,
        rng: &mut impl Rng,
    ) -> Pack {
        #[rustfmt::skip]
        let all_choices: &[(PackCategory, PackSize, u32)] = &[
            (PackCategory::Standard,  PackSize::Normal, 400),
            (PackCategory::Standard,  PackSize::Jumbo,  200),
            (PackCategory::Standard,  PackSize::Mega,    50),
            (PackCategory::Arcana,    PackSize::Normal, 400),
            (PackCategory::Arcana,    PackSize::Jumbo,  200),
            (PackCategory::Arcana,    PackSize::Mega,    50),
            (PackCategory::Celestial, PackSize::Normal, 400),
            (PackCategory::Celestial, PackSize::Jumbo,  200),
            (PackCategory::Celestial, PackSize::Mega,    50),
            (PackCategory::Buffoon,   PackSize::Normal, 120),
            (PackCategory::Buffoon,   PackSize::Jumbo,   60),
            (PackCategory::Buffoon,   PackSize::Mega,    15),
            (PackCategory::Spectral,  PackSize::Normal,  60),
            (PackCategory::Spectral,  PackSize::Jumbo,   30),
            (PackCategory::Spectral,  PackSize::Mega,     7),
        ];

        let choices: Vec<&(PackCategory, PackSize, u32)> = all_choices
            .iter()
            .filter(|(cat, sz, _)| exclude.is_none_or(|(ec, es)| cat != ec || sz != es))
            .collect();

        let weights: Vec<u32> = choices.iter().map(|(_, _, w)| *w).collect();
        let dist = WeightedIndex::new(&weights).unwrap();
        let idx = dist.sample(rng);
        let (category, size, _) = choices[idx];

        // Shared with `RealBackend` (`rng.rs`) so Fast/Real modes can never disagree
        // on pack content counts - Buffoon and Spectral share the smaller 2/4/4
        // shape, everything else is 3/5/5.
        let count = balatro_seed::pack_card_count(*category, *size) as usize;

        let contents = self.gen_contents(category, count, planetarium, prob_mult, held, rng);

        Pack {
            category: *category,
            size: *size,
            contents,
        }
    }

    fn gen_contents(
        &self,
        category: &PackCategory,
        count: usize,
        planetarium: &Planetarium,
        prob_mult: u32,
        held: PackInputs<'_>,
        rng: &mut impl Rng,
    ) -> Vec<PackContent> {
        let joker_gen = JokerGenerator {};
        let consumable_gen = ConsumableGenerator {};
        let held_jokers = held.jokers;
        let omen_globe = held.omen_globe;

        match category {
            // Omen Globe, transcribed from `next_arcana_pack`: each card is
            // a Spectral if a roll clears 0.8, otherwise a Tarot. The roll
            // is per card, not per pack, so a pack can come out mixed. It
            // is a raw roll on the game's own `omen_globe` node, so it is
            // deliberately not scaled by `prob_mult` — real mode gets the
            // exact node from balatro-seed.
            //
            // `next_arcana_pack` locks each card as it is drawn, so a pack
            // cannot repeat one. We were drawing with replacement instead.
            PackCategory::Arcana => {
                let mut tarots: Vec<Tarot> = Vec::new();
                let mut spectrals: Vec<Spectral> = Vec::new();
                (0..count)
                    .map(|_| {
                        let content = if omen_globe && rng.gen_ratio(1u32, 5u32) {
                            consumable_to_pack_content(
                                consumable_gen.gen_spectral_consumable(&spectrals, rng),
                            )
                        } else {
                            consumable_to_pack_content(
                                consumable_gen.gen_tarot_consumable(&tarots, rng),
                            )
                        };
                        match content {
                            PackContent::Tarot(t) => tarots.push(t),
                            PackContent::Spectral(s) => spectrals.push(s),
                            _ => unreachable!("an Arcana roll is a Tarot or a Spectral"),
                        }
                        content
                    })
                    .collect()
            }
            PackCategory::Celestial => {
                let mut exclude: Vec<Planets> = vec![];
                (0..count)
                    .map(|_| {
                        let planet = consumable_gen.gen_planet(planetarium, &exclude, rng);
                        exclude.push(planet);
                        PackContent::Planet(planet)
                    })
                    .collect()
            }
            PackCategory::Buffoon => {
                let mut seen: Vec<Jokers> = held_jokers.to_vec();
                (0..count)
                    .map(|_| {
                        let joker = joker_gen.gen_joker(prob_mult, &seen, rng);
                        seen.push(joker.clone());
                        PackContent::Joker(joker)
                    })
                    .collect()
            }
            PackCategory::Standard => (0..count)
                .map(|_| {
                    PackContent::PlayingCard(gen_random_playing_card(prob_mult, rng, false, None))
                })
                .collect(),
            // `next_spectral_pack` locks each card as it is drawn, so a pack
            // cannot repeat one.
            PackCategory::Spectral => {
                let mut seen: Vec<Spectral> = Vec::new();
                (0..count)
                    .map(|_| {
                        let c = consumable_gen.gen_spectral_consumable(&seen, rng);
                        let s = match c.kind {
                            ConsumableKind::Spectral(s) => s,
                            _ => unreachable!("gen_spectral_consumable returns a Spectral"),
                        };
                        seen.push(s);
                        PackContent::Spectral(s)
                    })
                    .collect()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::FastBackend;
    use rand_chacha::ChaCha8Rng;

    fn fast_backend() -> Backend {
        Backend::Fast(FastBackend::new(ChaCha8Rng::from_entropy()))
    }

    /// A Spectral pack cannot repeat a card. `next_spectral_pack` locks each
    /// card as it draws it, and the Real backend still does — this was a
    /// Fast/Real divergence, invisible in Real mode.
    #[test]
    fn test_spectral_packs_never_repeat_a_card() {
        let mut rng = ChaCha8Rng::seed_from_u64(9);
        let gen = PackGenerator {};
        let planetarium = Planetarium::new();
        let held = PackInputs {
            jokers: &[],
            omen_globe: false,
        };
        for size in [PackSize::Normal, PackSize::Jumbo, PackSize::Mega] {
            for _ in 0..200 {
                let contents = gen.gen_contents(
                    &PackCategory::Spectral,
                    balatro_seed::pack_card_count(PackCategory::Spectral, size) as usize,
                    &planetarium,
                    1,
                    held,
                    &mut rng,
                );
                let names: Vec<&str> = contents
                    .iter()
                    .map(|c| match c {
                        PackContent::Spectral(s) => s.name(),
                        _ => panic!("a Spectral pack holds Spectrals, got {c:?}"),
                    })
                    .collect();
                let unique: std::collections::HashSet<&&str> = names.iter().collect();
                assert_eq!(
                    unique.len(),
                    names.len(),
                    "{size:?} Spectral pack repeated a card: {names:?}"
                );
            }
        }
    }

    /// The same for Arcana, which is a Tarot roll with a Spectral substituted
    /// per card under Omen Globe. Tracked as two pools, so a pack can hold
    /// five of either kind without ever repeating one.
    #[test]
    fn test_arcana_packs_never_repeat_a_card() {
        let mut rng = ChaCha8Rng::seed_from_u64(11);
        let gen = PackGenerator {};
        let planetarium = Planetarium::new();
        for omen_globe in [false, true] {
            let held = PackInputs {
                jokers: &[],
                omen_globe,
            };
            for size in [PackSize::Normal, PackSize::Jumbo, PackSize::Mega] {
                for _ in 0..200 {
                    let contents = gen.gen_contents(
                        &PackCategory::Arcana,
                        balatro_seed::pack_card_count(PackCategory::Spectral, size) as usize,
                        &planetarium,
                        1,
                        held,
                        &mut rng,
                    );
                    let mut tarots: Vec<&str> = Vec::new();
                    let mut spectrals: Vec<&str> = Vec::new();
                    for c in &contents {
                        match c {
                            PackContent::Tarot(t) => tarots.push(t.name()),
                            PackContent::Spectral(s) => spectrals.push(s.name()),
                            _ => panic!("an Arcana pack holds Tarots or Spectrals, got {c:?}"),
                        }
                    }
                    for (kind, names) in [("Tarot", &tarots), ("Spectral", &spectrals)] {
                        let unique: std::collections::HashSet<&&str> = names.iter().collect();
                        assert_eq!(
                            unique.len(),
                            names.len(),
                            "{size:?} Arcana pack repeated a {kind} (omen_globe={omen_globe}): {names:?}"
                        );
                    }
                }
            }
        }
    }

    /// Standard packs are the deliberate exception. `next_standard_pack` does
    /// not lock, so a Standard pack really can contain two of the same card,
    /// and "fixing" that would make Fast disagree with Real. Asserted so a
    /// future blanket de-duplication of all five categories is caught.
    #[test]
    fn test_standard_packs_may_repeat_a_card() {
        let mut rng = ChaCha8Rng::seed_from_u64(13);
        let gen = PackGenerator {};
        let planetarium = Planetarium::new();
        let held = PackInputs {
            jokers: &[],
            omen_globe: false,
        };
        let mut saw_a_repeat = false;
        for _ in 0..400 {
            let contents = gen.gen_contents(
                &PackCategory::Standard,
                balatro_seed::pack_card_count(PackCategory::Standard, PackSize::Mega) as usize,
                &planetarium,
                1,
                held,
                &mut rng,
            );
            let mut keys: Vec<(Value, Suit)> = contents
                .iter()
                .map(|c| match c {
                    PackContent::PlayingCard(c) => (c.value, c.suit),
                    _ => panic!("a Standard pack holds playing cards, got {c:?}"),
                })
                .collect();
            let before = keys.len();
            keys.sort();
            keys.dedup();
            if keys.len() != before {
                saw_a_repeat = true;
                break;
            }
        }
        assert!(
            saw_a_repeat,
            "Standard packs are drawn with replacement in the real game too"
        );
    }

    #[test]
    fn test_shop_refresh() {
        let mut shop = Shop::new();
        let planetarium = Planetarium::new();
        assert_eq!(shop.jokers.len(), 0);
        assert_eq!(shop.packs.len(), 0);
        let vouchers = Vouchers::new();
        let ctx = ShopContext::for_test(&planetarium, &vouchers, 1);
        shop.refresh(&ctx, &mut fast_backend(), &mut TagState::default());
        assert_eq!(shop.jokers.len() + shop.consumables.len(), 2);
        assert_eq!(shop.packs.len(), 2);
    }

    #[test]
    fn test_shop_buy_joker() {
        let mut shop = Shop::new();
        let j1 = jokers_by_rarity(Rarity::Common)[0].clone();
        shop.jokers = vec![j1.clone()];
        assert_eq!(shop.joker_from_index(0).expect("first joker"), j1.clone());
        shop.buy_joker(&j1).expect("buy joker");
    }

    #[test]
    fn test_shop_buy_joker_disambiguates_duplicates_by_instance_id() {
        // Two copies of the same joker variant, distinguishable only by
        // instance_id (same as real shop generation would produce). Without
        // instance-id-sensitive equality, `buy_joker`'s `.position()` lookup
        // can't tell them apart and may remove the wrong one.
        let mut j1 = jokers_by_rarity(Rarity::Common)[0].clone();
        let mut j2 = j1.clone();
        j1.set_instance_id(1);
        j2.set_instance_id(2);

        let mut shop = Shop::new();
        shop.jokers = vec![j1.clone(), j2.clone()];

        let bought = shop.buy_joker(&j2).expect("buy joker");
        assert_eq!(bought.instance_id(), 2);
        assert_eq!(shop.jokers.len(), 1);
        assert_eq!(shop.jokers[0].instance_id(), 1);
    }

    #[test]
    fn test_shop_buy_consumable() {
        let mut shop = Shop::new();
        shop.consumables = vec![Consumable::planet(Planets::Mercury)];
        assert_eq!(shop.consumables.len(), 1);
        let c1 = shop.consumables[0];
        shop.buy_consumable(&c1).expect("buy consumable");
        assert_eq!(shop.consumables.len(), 0);
    }

    #[test]
    fn test_shop_buy_pack() {
        let mut shop = Shop::new();
        let planetarium = Planetarium::new();
        let vouchers = Vouchers::new();
        let ctx = ShopContext::for_test(&planetarium, &vouchers, 1);
        shop.refresh(&ctx, &mut fast_backend(), &mut TagState::default());
        assert_eq!(shop.packs.len(), 2);
        let p1 = shop.packs[0].clone();
        let bought = shop.buy_pack(&p1).expect("buy pack");
        assert_eq!(bought.category, p1.category);
        assert_eq!(shop.packs.len(), 1);
    }

    #[test]
    fn test_secret_planet_gating() {
        let planetarium = Planetarium::new();
        let gen = ConsumableGenerator::new();
        for _ in 0..500 {
            let c = gen.gen_planet_consumable(&planetarium, &[], &mut rand::thread_rng());
            let ConsumableKind::Planet(planet) = c.kind else {
                continue;
            };
            assert!(
                !planet.is_secret(),
                "secret planet generated before discovery"
            );
        }
    }

    #[test]
    fn test_gen_moves_buy_consumable_slots_full() {
        let mut shop = Shop::new();
        let planetarium = Planetarium::new();
        let vouchers = Vouchers::new();
        let ctx = ShopContext::for_test(&planetarium, &vouchers, 1);
        shop.refresh(&ctx, &mut fast_backend(), &mut TagState::default());
        // slots full (held == consumable_slots)
        let moves = shop.gen_moves_buy_consumable(100, 2, 2, &vouchers);
        assert!(moves.is_none());
    }

    #[test]
    fn test_gen_moves_buy_consumable_no_funds() {
        let mut shop = Shop::new();
        let planetarium = Planetarium::new();
        let vouchers = Vouchers::new();
        let ctx = ShopContext::for_test(&planetarium, &vouchers, 1);
        shop.refresh(&ctx, &mut fast_backend(), &mut TagState::default());
        // 0 money can't afford any planet ($3)
        let moves: Option<Vec<Action>> = shop
            .gen_moves_buy_consumable(0, 2, 0, &vouchers)
            .map(|i| i.collect());
        assert!(moves.is_none_or(|v| v.is_empty()));
    }

    #[test]
    fn test_pack_gen_produces_valid_packs() {
        let planetarium = Planetarium::new();
        let gen = PackGenerator {};
        for _ in 0..50 {
            let pack = gen.gen_pack(
                &planetarium,
                1,
                None,
                PackInputs {
                    jokers: &[],
                    omen_globe: false,
                },
                &mut rand::thread_rng(),
            );
            let expected_count = balatro_seed::pack_card_count(pack.category, pack.size) as usize;
            assert_eq!(pack.contents.len(), expected_count);
        }
    }

    #[test]
    fn test_pack_gen_can_produce_valid_spectral_packs() {
        let planetarium = Planetarium::new();
        let gen = PackGenerator {};
        let mut saw_spectral = false;
        for _ in 0..200 {
            let pack = gen.gen_pack(
                &planetarium,
                1,
                None,
                PackInputs {
                    jokers: &[],
                    omen_globe: false,
                },
                &mut rand::thread_rng(),
            );
            if pack.category != PackCategory::Spectral {
                continue;
            }
            saw_spectral = true;
            let expected_count = balatro_seed::pack_card_count(pack.category, pack.size) as usize;
            assert_eq!(pack.contents.len(), expected_count);
            for content in &pack.contents {
                let PackContent::Spectral(s) = content else {
                    panic!("Spectral pack produced non-Spectral content: {content:?}");
                };
                assert!(!s.is_rare(), "{s:?} should never appear in a normal draw");
            }
        }
        assert!(
            saw_spectral,
            "expected at least one Spectral pack in 200 draws"
        );
    }

    #[test]
    fn test_gen_joker_rarity_distribution_roughly_matches_weights() {
        use rand::SeedableRng;
        let mut rng = ChaCha8Rng::seed_from_u64(1234);
        let gen = JokerGenerator::new();
        let n = 2000;
        let mut common = 0;
        let mut uncommon = 0;
        let mut rare = 0;
        for _ in 0..n {
            match gen.gen_joker(1, &[], &mut rng).rarity() {
                Rarity::Common => common += 1,
                Rarity::Uncommon => uncommon += 1,
                Rarity::Rare => rare += 1,
                Rarity::Legendary => panic!("gen_joker should never roll Legendary"),
            }
        }
        // Generous bands around the intended 70/25/5 split - wide enough to
        // never flake, tight enough to catch the roll being broken/reverted
        // to Common-only.
        assert!(
            (1200..1900).contains(&common),
            "expected ~70% Common, got {common}/{n}"
        );
        assert!(
            (300..800).contains(&uncommon),
            "expected ~25% Uncommon, got {uncommon}/{n}"
        );
        assert!(
            (10..250).contains(&rare),
            "expected ~5% Rare, got {rare}/{n}"
        );
    }

    #[test]
    fn test_gen_edition_playing_cards_never_negative() {
        use rand::SeedableRng;
        let mut rng = ChaCha8Rng::seed_from_u64(42);
        // prob_mult saturates every gen_ratio numerator at 1000/1000 (always true);
        // with allow_negative=false this should fall through to Polychrome instead.
        for _ in 0..500 {
            assert_ne!(gen_edition(400, &mut rng, false), Edition::Negative);
        }
    }

    #[test]
    fn test_gen_edition_jokers_can_be_negative() {
        use rand::SeedableRng;
        let mut rng = ChaCha8Rng::seed_from_u64(42);
        assert_eq!(gen_edition(400, &mut rng, true), Edition::Negative);
    }
}

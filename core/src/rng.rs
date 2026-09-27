use crate::card::Card;
use crate::consumable::{Consumable, ConsumableKind};
use crate::deck::Deck;
use crate::joker::Jokers;
use crate::pack::{Pack, PackCategory, PackContent, PackSize};
use crate::planet::{Planetarium, Planets};
use crate::shop::{
    gen_random_playing_card, gen_shop_playing_card, ConsumableGenerator, JokerGenerator,
    PackGenerator, ShopContext,
};
use crate::tag::Tag;
use crate::tarot::Tarot;
use crate::voucher::{Voucher, Vouchers};
use balatro_seed::Instance;
use balatro_types::{BossBlind, Edition, HandRank, Rarity, Seal, Spectral, Suit, Value};
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use strum::IntoEnumIterator;

/// Which effect is rolling. Each maps to its own node in the real game's
/// per-node cache, so they are tracked separately rather than sharing one
/// stream — a variant with no [`balatro_seed::ProcNode`] has no confirmed
/// node ID and falls back to the fast stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EffectProc {
    /// Lucky Card: +20 Mult.
    LuckyMult,
    /// Lucky Card: +$20.
    LuckyMoney,
    /// The Wheel of Fortune tarot.
    WheelOfFortune,
    BusinessCard,
    ReservedParking,
    Bloodstone,
    // These are implemented in `balatro-seed` but their jokers aren't yet
    // in `core`, so nothing constructs them for now.
    SpaceJoker,
    EightBall,
    #[allow(dead_code)]
    /// The Wheel boss's per-played-card edition roll.
    WheelBoss,
    /// Hallucination's coin flip when a pack is opened.
    Hallucination,
    /// Misprint's flat Mult range.
    Misprint,
    /// The Crimson Heart's pick of which Joker to debuff. `core` knows this
    /// key but can't use it: the real game sorts the candidate pool by each
    /// Joker's `sort_id` (definition order) before rolling, and that
    /// ordering isn't recoverable from the 1.0.0 source dump.
    #[allow(dead_code)]
    CrimsonHeart,
    /// Gros Michel's end-of-round destruction roll.
    GrosMichelExtinct,
    /// Cavendish's, a distinct node at 1 in 1000.
    CavendishExtinct,
    /// Glass Card shattering after a scored hand. No node ID is recoverable
    /// from the 1.0.0 Lua dump — the playing-card centre definitions that
    /// hold the roll aren't in it.
    GlassCardBreak,
}

impl EffectProc {
    fn real_node(self) -> Option<balatro_seed::ProcNode> {
        use balatro_seed::ProcNode as P;
        match self {
            Self::LuckyMult => Some(P::LuckyMult),
            Self::LuckyMoney => Some(P::LuckyMoney),
            Self::WheelOfFortune => Some(P::WheelOfFortune),
            Self::BusinessCard => Some(P::BusinessCard),
            Self::ReservedParking => Some(P::ReservedParking),
            Self::Bloodstone => Some(P::Bloodstone),
            Self::SpaceJoker => Some(P::SpaceJoker),
            Self::EightBall => Some(P::EightBall),
            Self::GrosMichelExtinct => Some(P::GrosMichel),
            Self::CavendishExtinct => Some(P::Cavendish),
            Self::WheelBoss => Some(balatro_seed::ProcNode::WheelBoss),
            Self::Hallucination => None,
            Self::Misprint => Some(balatro_seed::ProcNode::Misprint),
            // See the variant's doc comment.
            Self::CrimsonHeart | Self::GlassCardBreak => None,
        }
    }
}

/// What a single shop-item generation call produced. The category split
/// has to happen inside the backend, since `Real` mode's category roll is
/// bundled into one `Instance::next_shop_item` call.
pub(crate) enum GeneratedItem {
    Joker(Jokers),
    Consumable(Consumable),
    /// Only ever produced with the Magic Trick voucher redeemed.
    PlayingCard(Card),
}

pub(crate) trait RngBackend {
    fn gen_shop_item(
        &mut self,
        ctx: &ShopContext,
        exclude_jokers: &[Jokers],
        exclude_tarots: &[Tarot],
        exclude_planets: &[Planets],
    ) -> GeneratedItem;

    fn gen_pack(&mut self, ctx: &ShopContext, exclude: Option<(&PackCategory, &PackSize)>) -> Pack;

    /// A pack of a forced category and size, skipping the weighted
    /// `shop_pack{ante}` roll. The pack tags open these directly rather
    /// than through the shop.
    fn gen_pack_of(
        &mut self,
        ctx: &ShopContext,
        category: PackCategory,
        size: PackSize,
    ) -> Pack;

    /// This ante's voucher offer, drawn from the vouchers still
    /// offerable (see `Vouchers::offerable`). `None` once all 32 are
    /// redeemed.
    fn gen_voucher(&mut self, ante: i32, vouchers: &Vouchers) -> Option<Voucher>;

    /// Owned-joker dedup hook: called on buy/sell so `Real` mode's lock
    /// table stays accurate. No-op for `Fast` mode.
    fn on_joker_bought(&mut self, joker: &Jokers);
    fn on_joker_sold(&mut self, joker: &Jokers);
    /// Redeemed-voucher hook: `Real` mode's draw rates read straight off
    /// its own active-voucher list, so it has to be told. No-op for
    /// `Fast` mode, which reads `ShopContext::vouchers` per call instead.
    fn on_voucher_bought(&mut self, voucher: &Voucher);
    /// Jokers::Showman's real effect. No call site yet — see `jokers.md`.
    #[allow(dead_code)]
    fn set_showman(&mut self, owned: bool);

    /// Once per run, before anything is drawn. `Real` mode seeds the real
    /// game's ante-1 and fresh-run lock table here, so it has to run before
    /// the first `on_ante_start`.
    fn on_run_start(&mut self) {}

    /// Every time the run enters a new ante, before that ante's draws.
    /// `Real` mode applies the ante-gated boss/tag unlocks.
    fn on_ante_start(&mut self, _ante: i32) {}

    /// A single freshly generated joker, outside the shop/pack context —
    /// currently only Judgement. Distinct from `gen_shop_item`'s joker arm
    /// since a shop slot's category (joker vs. consumable) is its own roll.
    fn gen_joker(&mut self, ante: i32, prob_mult: u32, exclude: &[Jokers]) -> Jokers;
    fn gen_joker_of_rarity(
        &mut self,
        _ante: i32,
        prob_mult: u32,
        exclude: &[Jokers],
        rarity: Rarity,
    ) -> Jokers;
    fn pick_random_joker(&mut self, available: Vec<Jokers>) -> Jokers;
    fn clone_joker(&mut self, j: Jokers) -> Jokers;
    fn shuffle_deck(&mut self, deck: &mut Deck);
    /// Shuffle the player's joker row. Only Amber Acorn wants this.
    fn shuffle_jokers(&mut self, jokers: &mut Vec<Jokers>);
    /// A `1 / denominator` effect proc, scaled by the run's `prob_mult`.
    fn prob_roll(&mut self, proc: EffectProc, numerator: u32, denominator: u32) -> bool;
    /// Returns `(small_blind_tag, big_blind_tag)` for a fresh ante.
    fn draw_ante_tags(&mut self, ante: i32) -> (Tag, Tag);
    /// Returns the Boss Blind for a fresh ante.
    fn draw_boss(&mut self, ante: i32) -> BossBlind;
    /// A single random Tarot consumable, respecting `exclude` — used by
    /// the purple-seal discard trigger and the Emperor/High Priestess
    /// tarot effects.
    fn roll_random_tarot(&mut self, exclude: &[Tarot]) -> Consumable;
    /// A single random Planet consumable, respecting `exclude` — used by
    /// the blue-seal round-end trigger and the High Priestess tarot effect.
    fn roll_random_planet(&mut self, planetarium: &Planetarium, exclude: &[Planets]) -> Consumable;
    /// A flat integer range, for Misprint's +0-23.
    fn roll_range(&mut self, proc: EffectProc, min: i32, max: i32) -> i32;
    /// A single random Spectral consumable — Séance, Ankh and friends.
    fn roll_random_spectral(&mut self, exclude: &[Spectral]) -> Consumable;
    /// A random hand rank, for the Orbital Tag. Royal Flush shares a
    /// Planetarium slot with Straight Flush, so only the 12 scoring ranks
    /// are rolled.
    fn roll_random_hand(&mut self) -> HandRank;
    /// Castle/MailInRebate's per-round reroll (`Game::clear_blind`). Mint-time
    /// rolling for freshly generated jokers already happens inside
    /// `gen_joker`/`gen_shop_item`/`gen_pack`, each already using the right
    /// stream per backend — this is only for rerolling an *existing* joker.
    fn roll_discard_selector(&mut self, j: &mut Jokers);
    fn roll_random_edition(&mut self) -> Edition;
    /// A random seal for Certificate, from the `stdsealtype{ante}` node.
    fn roll_random_seal(&mut self, ante: i32) -> Seal;
    fn roll_random_suit(&mut self) -> Suit;
    fn roll_random_value(&mut self) -> Value;
    fn pick_random_card(&mut self, available: Vec<Card>) -> Card;
    /// A uniform index into a collection, for the bosses that pick at
    /// random (The Hook's discards, The Crimson Heart's victim).
    fn random_index(&mut self, len: usize) -> usize;
    fn gen_random_card(
        &mut self,
        prob_mult: u32,
        force_enhance: bool,
        force_values: Option<&[Value]>,
    ) -> Card;
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub(crate) struct FastBackend {
    rng: ChaCha8Rng,
    joker_gen: JokerGenerator,
    consumable_gen: ConsumableGenerator,
    pack_gen: PackGenerator,
}

impl FastBackend {
    pub(crate) fn new(rng: ChaCha8Rng) -> Self {
        FastBackend {
            rng,
            joker_gen: JokerGenerator::new(),
            consumable_gen: ConsumableGenerator::new(),
            pack_gen: PackGenerator {},
        }
    }
}

impl RngBackend for FastBackend {
    fn gen_shop_item(
        &mut self,
        ctx: &ShopContext,
        exclude_jokers: &[Jokers],
        exclude_tarots: &[Tarot],
        exclude_planets: &[Planets],
    ) -> GeneratedItem {
        // Joker=20, Tarot/Planet=4 each before the Merchant/Tycoon
        // vouchers raise them; playing cards weigh 0 until Magic Trick.
        let weights = [
            20.0,
            ctx.vouchers.tarot_rate(),
            ctx.vouchers.planet_rate(),
            ctx.vouchers.playing_card_rate(),
        ];
        let dist = WeightedIndex::new(weights).expect("shop item weights are finite and nonzero");
        match dist.sample(&mut self.rng) {
            0 => GeneratedItem::Joker(self.joker_gen.gen_joker(
                ctx.edition_prob_mult(),
                exclude_jokers,
                &mut self.rng,
            )),
            1 => GeneratedItem::Consumable(
                self.consumable_gen
                    .gen_tarot_consumable(exclude_tarots, &mut self.rng),
            ),
            2 => GeneratedItem::Consumable(self.consumable_gen.gen_planet_consumable(
                ctx.planetarium,
                exclude_planets,
                &mut self.rng,
            )),
            _ => GeneratedItem::PlayingCard(gen_shop_playing_card(
                ctx.edition_prob_mult(),
                ctx.vouchers.shop_cards_are_modified(),
                &mut self.rng,
            )),
        }
    }

    fn gen_pack(&mut self, ctx: &ShopContext, exclude: Option<(&PackCategory, &PackSize)>) -> Pack {
        self.pack_gen.gen_pack(
            ctx.planetarium,
            ctx.edition_prob_mult(),
            exclude,
            ctx.pack_inputs(),
            &mut self.rng,
        )
    }

    fn gen_pack_of(
        &mut self,
        ctx: &ShopContext,
        category: PackCategory,
        size: PackSize,
    ) -> Pack {
        self.pack_gen.gen_pack_of(
            ctx.planetarium,
            ctx.edition_prob_mult(),
            category,
            size,
            ctx.pack_inputs(),
            &mut self.rng,
        )
    }

    fn gen_voucher(&mut self, _ante: i32, vouchers: &Vouchers) -> Option<Voucher> {
        let offerable = vouchers.offerable();
        if offerable.is_empty() {
            return None;
        }
        Some(offerable[self.rng.gen_range(0..offerable.len())])
    }

    fn on_joker_bought(&mut self, _joker: &Jokers) {}
    fn on_joker_sold(&mut self, _joker: &Jokers) {}
    fn on_voucher_bought(&mut self, _voucher: &Voucher) {}
    fn set_showman(&mut self, _owned: bool) {}

    fn gen_joker(&mut self, _ante: i32, prob_mult: u32, exclude: &[Jokers]) -> Jokers {
        self.joker_gen.gen_joker(prob_mult, exclude, &mut self.rng)
    }

    fn gen_joker_of_rarity(
        &mut self,
        _ante: i32,
        prob_mult: u32,
        exclude: &[Jokers],
        rarity: Rarity,
    ) -> Jokers {
        self.joker_gen
            .gen_joker_of_rarity(prob_mult, exclude, rarity, &mut self.rng)
    }

    fn pick_random_joker(&mut self, available: Vec<Jokers>) -> Jokers {
        let idx = self.rng.gen_range(0..available.len());
        available[idx].clone()
    }

    fn clone_joker(&mut self, j: Jokers) -> Jokers {
        self.joker_gen.clone_joker(j, &mut self.rng)
    }

    fn shuffle_deck(&mut self, deck: &mut Deck) {
        deck.shuffle(&mut self.rng);
    }

    fn shuffle_jokers(&mut self, jokers: &mut Vec<Jokers>) {
        jokers.shuffle(&mut self.rng);
    }

    fn prob_roll(&mut self, _proc: EffectProc, numerator: u32, denominator: u32) -> bool {
        self.rng.gen_ratio(numerator.min(denominator), denominator)
    }

    fn draw_ante_tags(&mut self, _ante: i32) -> (Tag, Tag) {
        let tags: Vec<Tag> = Tag::iter().collect();
        let small = *tags.choose(&mut self.rng).unwrap();
        let big = *tags.choose(&mut self.rng).unwrap();
        (small, big)
    }

    fn draw_boss(&mut self, ante: i32) -> BossBlind {
        let pool: Vec<BossBlind> = BossBlind::iter()
            .filter(|b| b.is_finisher() == (ante % 8 == 0))
            .filter(|b| ante >= b.min_ante())
            .collect();
        *pool.choose(&mut self.rng).unwrap()
    }

    fn roll_random_tarot(&mut self, exclude: &[Tarot]) -> Consumable {
        self.consumable_gen
            .gen_tarot_consumable(exclude, &mut self.rng)
    }

    fn roll_random_planet(&mut self, planetarium: &Planetarium, exclude: &[Planets]) -> Consumable {
        self.consumable_gen
            .gen_planet_consumable(planetarium, exclude, &mut self.rng)
    }

    fn roll_range(&mut self, _proc: EffectProc, min: i32, max: i32) -> i32 {
        self.rng.gen_range(min..=max)
    }

    fn roll_random_spectral(&mut self, exclude: &[Spectral]) -> Consumable {
        let all = Spectral::iter().filter(|s| !exclude.contains(s)).collect::<Vec<_>>();
        let pick = *all
            .choose(&mut self.rng)
            .expect("Spectral pool is never empty");
        Consumable::spectral(pick)
    }

    fn roll_random_hand(&mut self) -> HandRank {
        HandRank::iter()
            .filter(|r| *r != HandRank::RoyalFlush)
            .choose(&mut self.rng)
            .expect("HandRank has more than RoyalFlush")
    }

    fn roll_discard_selector(&mut self, j: &mut Jokers) {
        crate::joker::roll_discard_selector(&mut self.rng, j);
    }

    fn roll_random_edition(&mut self) -> Edition {
        const EDITIONS: [Edition; 3] = [Edition::Foil, Edition::Holographic, Edition::Polychrome];
        EDITIONS[self.rng.gen_range(0..EDITIONS.len())]
    }

    fn roll_random_seal(&mut self, _ante: i32) -> Seal {
        const SEALS: [Seal; 4] = [Seal::Red, Seal::Blue, Seal::Gold, Seal::Purple];
        SEALS[self.rng.gen_range(0..SEALS.len())]
    }

    fn roll_random_suit(&mut self) -> Suit {
        let suits: Vec<Suit> = Suit::iter().collect();
        suits[self.rng.gen_range(0..suits.len())]
    }

    fn roll_random_value(&mut self) -> Value {
        let values: Vec<Value> = Value::iter().collect();
        values[self.rng.gen_range(0..values.len())]
    }

    fn pick_random_card(&mut self, available: Vec<Card>) -> Card {
        available[self.rng.gen_range(0..available.len())]
    }

    fn random_index(&mut self, len: usize) -> usize {
        self.rng.gen_range(0..len)
    }

    fn gen_random_card(
        &mut self,
        prob_mult: u32,
        force_enhance: bool,
        force_values: Option<&[Value]>,
    ) -> Card {
        gen_random_playing_card(prob_mult, &mut self.rng, force_enhance, force_values)
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub(crate) struct RealBackend {
    instance: Instance,
    // `Instance` only replays real Balatro's actual seed algorithm, need a
    // separate dedicated fast rng for some other things atm — also used
    // for shop playing cards (Magic Trick): `balatro-seed`'s
    // `ShopItem::PlayingCard` is a marker that carries no card data, and
    // drawing one out of the `Instance` would advance nodes the real game
    // doesn't, desyncing every later draw.
    //
    // Every method that still calls into `fast` is a real-mode accuracy gap,
    // not a deliberate simplification. See `ARCHITECTURE.md`.
    extra_rng: ChaCha8Rng,
    // Need a fast backend for gen that real doesn't yet impl
    fast: FastBackend,
}

impl RealBackend {
    pub(crate) fn new(seed: &str, fast: FastBackend) -> Self {
        RealBackend {
            instance: Instance::new(seed),
            extra_rng: ChaCha8Rng::seed_from_u64(crate::seed_from_str(seed).wrapping_add(1)),
            fast,
        }
    }

    fn gen_pack_contents(
        &mut self,
        ante: i32,
        category: PackCategory,
        count: i32,
    ) -> Vec<PackContent> {
        match category {
            PackCategory::Arcana => self
                .instance
                .next_arcana_pack(count, ante)
                .into_iter()
                .map(consumable_to_pack_content)
                .collect(),
            PackCategory::Celestial => self
                .instance
                .next_celestial_pack(count, ante)
                .into_iter()
                .map(consumable_to_pack_content)
                .collect(),
            PackCategory::Spectral => self
                .instance
                .next_spectral_pack(count, ante)
                .into_iter()
                .map(consumable_to_pack_content)
                .collect(),
            PackCategory::Buffoon => self
                .instance
                .next_buffoon_pack(count, ante)
                .into_iter()
                .map(|j| seed_joker_with_id(j, &mut self.extra_rng))
                .map(PackContent::Joker)
                .collect(),
            PackCategory::Standard => self
                .instance
                .next_standard_pack(count, ante)
                .into_iter()
                .map(|c| PackContent::PlayingCard(seed_card_to_core_card(c)))
                .collect(),
        }
    }
}

/// `balatro_types::Card` has no `id`; `core::Card` adds one, so only
/// `value`/`suit` go through the id-assigning constructor.
fn seed_card_to_core_card(c: balatro_types::Card) -> Card {
    let mut card = Card::new(c.value, c.suit);
    card.edition = c.edition;
    card.enhancement = c.enhancement;
    card.seal = c.seal;
    card
}

/// Joker generation needs some engine only init at create time
fn seed_joker_with_id(mut j: Jokers, rng: &mut impl Rng) -> Jokers {
    j.set_instance_id(crate::joker::mint_joker_id());
    crate::joker::roll_discard_selector(rng, &mut j);
    j
}

/// `Consumable` may carry Soul/Black Hole (a Spectral) even from a
/// nominally Tarot/Planet draw.
fn consumable_to_pack_content(c: Consumable) -> PackContent {
    match c.kind {
        ConsumableKind::Tarot(t) => PackContent::Tarot(t),
        ConsumableKind::Planet(p) => PackContent::Planet(p),
        ConsumableKind::Spectral(s) => PackContent::Spectral(s),
    }
}

impl RngBackend for RealBackend {
    // `ctx.planetarium` is unused: `Fast` mode uses it to gate secret
    // planets behind discovery state, which isn't wired into `Real` mode
    // TODO. Voucher-driven rates come from the `Instance`'s own active
    // list (kept current by `on_voucher_bought`), not `ctx.vouchers`.
    fn gen_shop_item(
        &mut self,
        ctx: &ShopContext,
        _exclude_jokers: &[Jokers],
        _exclude_tarots: &[Tarot],
        _exclude_planets: &[Planets],
    ) -> GeneratedItem {
        match self.instance.next_shop_item(ctx.ante) {
            balatro_seed::ShopItem::Joker(j) => {
                GeneratedItem::Joker(seed_joker_with_id(j, &mut self.extra_rng))
            }
            balatro_seed::ShopItem::Consumable(c) => GeneratedItem::Consumable(c),
            balatro_seed::ShopItem::PlayingCard => {
                GeneratedItem::PlayingCard(gen_shop_playing_card(
                    ctx.edition_prob_mult(),
                    ctx.vouchers.shop_cards_are_modified(),
                    &mut self.extra_rng,
                ))
            }
        }
    }

    fn gen_pack(
        &mut self,
        ctx: &ShopContext,
        _exclude: Option<(&PackCategory, &PackSize)>,
    ) -> Pack {
        let (category, size) = self.instance.next_pack(ctx.ante);
        let count = balatro_seed::pack_card_count(category, size);
        let contents = self.gen_pack_contents(ctx.ante, category, count);
        Pack {
            category,
            size,
            contents,
        }
    }

    // Unlike `Fast` mode this draws from the full pool and then filters:
    // the draw has to consume the same node the real game would, whether
    // or not the result is still offerable. A base voucher already owned
    // is upgraded in place (`voucher_upgrade`), matching the real game.
    // Calls the same per-category draw the weighted `gen_pack` path ends up
    // calling, so a tag-opened pack is indistinguishable from a bought one
    // in what the `Instance` consumes.
    fn gen_pack_of(
        &mut self,
        ctx: &ShopContext,
        category: PackCategory,
        size: PackSize,
    ) -> Pack {
        let ante = ctx.ante;
        let count = balatro_seed::pack_card_count(category, size);
        let contents = self.gen_pack_contents(ante, category, count);
        Pack {
            category,
            size,
            contents,
        }
    }

    fn gen_voucher(&mut self, ante: i32, vouchers: &Vouchers) -> Option<Voucher> {
        let drawn = self.instance.next_voucher(ante);
        let offer = if vouchers.has(drawn) {
            balatro_seed::voucher_upgrade(drawn).filter(|u| vouchers.is_offerable(*u))
        } else {
            Some(drawn).filter(|v| vouchers.is_offerable(*v))
        };
        if let Some(v) = offer {
            self.instance.lock(&v);
        }
        offer
    }

    fn on_joker_bought(&mut self, joker: &Jokers) {
        self.instance.lock(joker);
    }

    fn on_joker_sold(&mut self, joker: &Jokers) {
        self.instance.unlock(joker);
    }

    fn on_voucher_bought(&mut self, voucher: &Voucher) {
        self.instance.activate_voucher(voucher);
    }

    fn set_showman(&mut self, owned: bool) {
        self.instance.params.showman = owned;
    }

    // The real game seeds its lock table before the first draw: everything
    // ante-gated below 2, plus the fresh-run list. `fresh_profile` is
    // false because `core` has no unlock-set to read it from.
    fn on_run_start(&mut self) {
        self.instance.init_locks(1, false, true);
    }

    // Ante 2+ unlock the bosses and tags that just became eligible. This is
    // what makes `next_boss`'s category filter respect `BossBlind::min_ante`
    // without a separate check.
    fn on_ante_start(&mut self, ante: i32) {
        self.instance.init_unlocks(ante, false);
    }

    // `Instance::next_joker(source, ante)` exists and isn't wired up yet,
    // real accuracy here is a follow-up. Reuses the same Fast-style
    // generator `FastBackend` uses, seeded off `extra_rng` instead.
    fn gen_joker(&mut self, ante: i32, prob_mult: u32, exclude: &[Jokers]) -> Jokers {
        self.fast.gen_joker(ante, prob_mult, exclude)
    }

    fn gen_joker_of_rarity(
        &mut self,
        ante: i32,
        prob_mult: u32,
        exclude: &[Jokers],
        rarity: Rarity,
    ) -> Jokers {
        self.fast
            .gen_joker_of_rarity(ante, prob_mult, exclude, rarity)
    }

    fn pick_random_joker(&mut self, available: Vec<Jokers>) -> Jokers {
        self.fast.pick_random_joker(available)
    }

    fn clone_joker(&mut self, j: Jokers) -> Jokers {
        self.fast.clone_joker(j)
    }

    // The real game seeds `CardArea:shuffle` from `pseudoseed('shuffle')`
    // and runs a Fisher-Yates, which is portable — but only once the
    // deck's pre-shuffle order matches, and the real game sorts by a
    // `sort_id` assigned in table-declaration order. That order isn't
    // recoverable from the 1.0.0 source dump, so a port here would shuffle
    // correctly and still draw the wrong cards.
    fn shuffle_deck(&mut self, deck: &mut Deck) {
        self.fast.shuffle_deck(deck);
    }

    // The real game uses the node 'aajk' here, but sorts the joker row by
    // each joker's declaration order first, which isn't recoverable from
    // the 1.0.0 source dump. A valid permutation, not a byte-exact one.
    fn shuffle_jokers(&mut self, jokers: &mut Vec<Jokers>) {
        self.fast.shuffle_jokers(jokers);
    }

    // Every effect proc that has a confirmed node ID replays the real
    // game's own per-node cache. Glass Card's roll is the one exception —
    // see `EffectProc::GlassCardBreak`.
    fn prob_roll(&mut self, proc: EffectProc, numerator: u32, denominator: u32) -> bool {
        match proc.real_node() {
            Some(node) => self.instance.proc_roll(node, 1, numerator, denominator),
            None => self.fast.prob_roll(proc, numerator, denominator),
        }
    }

    // Two independent draws from the `Tag{ante}` node, matching the real
    // game's per-ante small/big tag pair. The lock table (not an explicit
    // ante filter) is what keeps ante-1-only tags like Negative out of
    // ante 1 — see `Instance::init_locks`.
    fn draw_ante_tags(&mut self, ante: i32) -> (Tag, Tag) {
        let small = self.instance.next_tag(ante);
        let big = self.instance.next_tag(ante);
        (small, big)
    }

    // Correct category (regular vs. finisher), correct pool-cycling, and
    // correct minimum ante — the last because `on_run_start`/`on_ante_start`
    // pre-lock every not-yet-eligible boss name.
    fn draw_boss(&mut self, ante: i32) -> BossBlind {
        self.instance.next_boss(ante)
    }

    // `Instance::next_tarot(source, ante, soulable)` exists and isn't
    // wired up yet - real accuracy here is a follow-up.
    fn roll_random_tarot(&mut self, exclude: &[Tarot]) -> Consumable {
        self.fast.roll_random_tarot(exclude)
    }

    // `Instance::next_planet(source, ante, soulable)` exists and isn't
    // wired up yet - real accuracy here is a follow-up.
    fn roll_random_planet(&mut self, planetarium: &Planetarium, exclude: &[Planets]) -> Consumable {
        self.fast.roll_random_planet(planetarium, exclude)
    }

    // No node ID is recoverable for the Orbital Tag's pick, and the real
    // game may not even roll one — it shows the hand on the tag and the
    // player chooses whether to take it. Falls back to the fast stream.
    fn roll_range(&mut self, proc: EffectProc, min: i32, max: i32) -> i32 {
        match proc.real_node() {
            Some(node) => self.instance.proc_randint(node, min, max),
            None => self.fast.roll_range(proc, min, max),
        }
    }

    fn roll_random_spectral(&mut self, exclude: &[Spectral]) -> Consumable {
        // `Instance::next_spectral` exists and takes a source code the real
        // game uses for its own spectral sources; no consumer of ours has a
        // known one yet, so this stays on the fast stream.
        self.fast.roll_random_spectral(exclude)
    }

    fn roll_random_hand(&mut self) -> HandRank {
        self.fast.roll_random_hand()
    }

    fn roll_discard_selector(&mut self, j: &mut Jokers) {
        self.fast.roll_discard_selector(j);
    }

    fn roll_random_edition(&mut self) -> Edition {
        self.fast.roll_random_edition()
    }

    fn roll_random_seal(&mut self, ante: i32) -> Seal {
        self.instance.proc_roll_seal(ante)
    }

    fn roll_random_suit(&mut self) -> Suit {
        self.fast.roll_random_suit()
    }

    fn roll_random_value(&mut self) -> Value {
        self.fast.roll_random_value()
    }

    fn pick_random_card(&mut self, available: Vec<Card>) -> Card {
        self.fast.pick_random_card(available)
    }

    // Bare `math.random` in the real game, so the exact result depends on
    // call history rather than on a node.
    fn random_index(&mut self, len: usize) -> usize {
        self.fast.random_index(len)
    }

    fn gen_random_card(
        &mut self,
        prob_mult: u32,
        force_enhance: bool,
        force_values: Option<&[Value]>,
    ) -> Card {
        self.fast
            .gen_random_card(prob_mult, force_enhance, force_values)
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
// Both variants are inherently chunky — each owns an rng, and `Real` also
// owns the seed `Instance`'s node and lock tables. Exactly one exists per
// `Game`, so boxing would only trade a few hundred bytes for an
// indirection on every draw.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Backend {
    Fast(FastBackend),
    Real(RealBackend),
}

impl RngBackend for Backend {
    fn gen_shop_item(
        &mut self,
        ctx: &ShopContext,
        exclude_jokers: &[Jokers],
        exclude_tarots: &[Tarot],
        exclude_planets: &[Planets],
    ) -> GeneratedItem {
        match self {
            Backend::Fast(b) => {
                b.gen_shop_item(ctx, exclude_jokers, exclude_tarots, exclude_planets)
            }
            Backend::Real(b) => {
                b.gen_shop_item(ctx, exclude_jokers, exclude_tarots, exclude_planets)
            }
        }
    }

    fn gen_pack(&mut self, ctx: &ShopContext, exclude: Option<(&PackCategory, &PackSize)>) -> Pack {
        match self {
            Backend::Fast(b) => b.gen_pack(ctx, exclude),
            Backend::Real(b) => b.gen_pack(ctx, exclude),
        }
    }

    fn gen_pack_of(
        &mut self,
        ctx: &ShopContext,
        category: PackCategory,
        size: PackSize,
    ) -> Pack {
        match self {
            Backend::Fast(b) => b.gen_pack_of(ctx, category, size),
            Backend::Real(b) => b.gen_pack_of(ctx, category, size),
        }
    }

    fn gen_voucher(&mut self, ante: i32, vouchers: &Vouchers) -> Option<Voucher> {
        match self {
            Backend::Fast(b) => b.gen_voucher(ante, vouchers),
            Backend::Real(b) => b.gen_voucher(ante, vouchers),
        }
    }

    fn on_joker_bought(&mut self, joker: &Jokers) {
        match self {
            Backend::Fast(b) => b.on_joker_bought(joker),
            Backend::Real(b) => b.on_joker_bought(joker),
        }
    }

    fn on_joker_sold(&mut self, joker: &Jokers) {
        match self {
            Backend::Fast(b) => b.on_joker_sold(joker),
            Backend::Real(b) => b.on_joker_sold(joker),
        }
    }

    fn on_voucher_bought(&mut self, voucher: &Voucher) {
        match self {
            Backend::Fast(b) => b.on_voucher_bought(voucher),
            Backend::Real(b) => b.on_voucher_bought(voucher),
        }
    }

    fn set_showman(&mut self, owned: bool) {
        match self {
            Backend::Fast(b) => b.set_showman(owned),
            Backend::Real(b) => b.set_showman(owned),
        }
    }

    fn on_run_start(&mut self) {
        match self {
            Backend::Fast(b) => b.on_run_start(),
            Backend::Real(b) => b.on_run_start(),
        }
    }

    fn on_ante_start(&mut self, ante: i32) {
        match self {
            Backend::Fast(b) => b.on_ante_start(ante),
            Backend::Real(b) => b.on_ante_start(ante),
        }
    }

    fn gen_joker(&mut self, ante: i32, prob_mult: u32, exclude: &[Jokers]) -> Jokers {
        match self {
            Backend::Fast(b) => b.gen_joker(ante, prob_mult, exclude),
            Backend::Real(b) => b.gen_joker(ante, prob_mult, exclude),
        }
    }

    fn gen_joker_of_rarity(
        &mut self,
        ante: i32,
        prob_mult: u32,
        exclude: &[Jokers],
        rarity: Rarity,
    ) -> Jokers {
        match self {
            Backend::Fast(b) => b.gen_joker_of_rarity(ante, prob_mult, exclude, rarity),
            Backend::Real(b) => b.gen_joker_of_rarity(ante, prob_mult, exclude, rarity),
        }
    }

    fn pick_random_joker(&mut self, available: Vec<Jokers>) -> Jokers {
        match self {
            Backend::Fast(b) => b.pick_random_joker(available),
            Backend::Real(b) => b.pick_random_joker(available),
        }
    }

    fn clone_joker(&mut self, j: Jokers) -> Jokers {
        match self {
            Backend::Fast(b) => b.clone_joker(j),
            Backend::Real(b) => b.clone_joker(j),
        }
    }

    fn shuffle_deck(&mut self, deck: &mut Deck) {
        match self {
            Backend::Fast(b) => b.shuffle_deck(deck),
            Backend::Real(b) => b.shuffle_deck(deck),
        }
    }

    fn shuffle_jokers(&mut self, jokers: &mut Vec<Jokers>) {
        match self {
            Backend::Fast(b) => b.shuffle_jokers(jokers),
            Backend::Real(b) => b.shuffle_jokers(jokers),
        }
    }

    fn prob_roll(&mut self, proc: EffectProc, numerator: u32, denominator: u32) -> bool {
        match self {
            Backend::Fast(b) => b.prob_roll(proc, numerator, denominator),
            Backend::Real(b) => b.prob_roll(proc, numerator, denominator),
        }
    }

    fn draw_ante_tags(&mut self, ante: i32) -> (Tag, Tag) {
        match self {
            Backend::Fast(b) => b.draw_ante_tags(ante),
            Backend::Real(b) => b.draw_ante_tags(ante),
        }
    }

    fn draw_boss(&mut self, ante: i32) -> BossBlind {
        match self {
            Backend::Fast(b) => b.draw_boss(ante),
            Backend::Real(b) => b.draw_boss(ante),
        }
    }

    fn roll_random_tarot(&mut self, exclude: &[Tarot]) -> Consumable {
        match self {
            Backend::Fast(b) => b.roll_random_tarot(exclude),
            Backend::Real(b) => b.roll_random_tarot(exclude),
        }
    }

    fn roll_random_planet(&mut self, planetarium: &Planetarium, exclude: &[Planets]) -> Consumable {
        match self {
            Backend::Fast(b) => b.roll_random_planet(planetarium, exclude),
            Backend::Real(b) => b.roll_random_planet(planetarium, exclude),
        }
    }

    fn roll_range(&mut self, proc: EffectProc, min: i32, max: i32) -> i32 {
        match self {
            Backend::Fast(b) => b.roll_range(proc, min, max),
            Backend::Real(b) => b.roll_range(proc, min, max),
        }
    }

    fn roll_random_spectral(&mut self, exclude: &[Spectral]) -> Consumable {
        match self {
            Backend::Fast(b) => b.roll_random_spectral(exclude),
            Backend::Real(b) => b.roll_random_spectral(exclude),
        }
    }

    fn roll_random_hand(&mut self) -> HandRank {
        match self {
            Backend::Fast(b) => b.roll_random_hand(),
            Backend::Real(b) => b.roll_random_hand(),
        }
    }

    fn roll_discard_selector(&mut self, j: &mut Jokers) {
        match self {
            Backend::Fast(b) => b.roll_discard_selector(j),
            Backend::Real(b) => b.roll_discard_selector(j),
        }
    }
    fn roll_random_seal(&mut self, ante: i32) -> Seal {
        match self {
            Backend::Fast(b) => b.roll_random_seal(ante),
            Backend::Real(b) => b.roll_random_seal(ante),
        }
    }

    fn roll_random_edition(&mut self) -> Edition {
        match self {
            Backend::Fast(b) => b.roll_random_edition(),
            Backend::Real(b) => b.roll_random_edition(),
        }
    }

    fn roll_random_suit(&mut self) -> Suit {
        match self {
            Backend::Fast(b) => b.roll_random_suit(),
            Backend::Real(b) => b.roll_random_suit(),
        }
    }

    fn roll_random_value(&mut self) -> Value {
        match self {
            Backend::Fast(b) => b.roll_random_value(),
            Backend::Real(b) => b.roll_random_value(),
        }
    }

    fn pick_random_card(&mut self, available: Vec<Card>) -> Card {
        match self {
            Backend::Fast(b) => b.pick_random_card(available),
            Backend::Real(b) => b.pick_random_card(available),
        }
    }

    fn random_index(&mut self, len: usize) -> usize {
        match self {
            Backend::Fast(b) => b.random_index(len),
            Backend::Real(b) => b.random_index(len),
        }
    }

    fn gen_random_card(
        &mut self,
        prob_mult: u32,
        force_enhance: bool,
        force_values: Option<&[Value]>,
    ) -> Card {
        match self {
            Backend::Fast(b) => b.gen_random_card(prob_mult, force_enhance, force_values),
            Backend::Real(b) => b.gen_random_card(prob_mult, force_enhance, force_values),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::joker::JokerEffects;

    #[test]
    fn fast_backend_draw_ante_tags_draws_two_valid_tags() {
        let mut backend = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let (small, big) = backend.draw_ante_tags(1);
        assert!(Tag::iter().any(|t| t == small));
        assert!(Tag::iter().any(|t| t == big));
    }

    #[test]
    fn fast_backend_draw_boss_matches_category() {
        let mut backend = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        for ante in [1, 7, 8, 9, 16] {
            let boss = backend.draw_boss(ante);
            assert_eq!(
                boss.is_finisher(),
                ante % 8 == 0,
                "ante {ante} drew {boss:?}"
            );
        }
    }

    #[test]
    fn fast_backend_draw_boss_respects_min_ante() {
        let mut backend = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        for _ in 0..200 {
            let boss = backend.draw_boss(1);
            assert!(boss.min_ante() <= 1, "ante 1 drew {boss:?} (min_ante > 1)");
        }
    }

    #[test]
    fn fast_backend_prob_roll_respects_ratio_edges() {
        let mut backend = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        // numerator 0 -> never; numerator == denominator -> always,
        // regardless of what's drawn.
        for _ in 0..20 {
            assert!(!backend.prob_roll(EffectProc::LuckyMult, 0, 10));
        }
        for _ in 0..20 {
            assert!(backend.prob_roll(EffectProc::LuckyMult, 10, 10));
        }
        // numerator > denominator gets clamped to denominator (always true),
        // not a `gen_ratio` panic.
        assert!(backend.prob_roll(EffectProc::LuckyMult, 50, 10));
    }

    #[test]
    fn fast_backend_shuffle_deck_reorders_without_losing_cards() {
        let mut backend = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut deck = crate::deck::Deck::default();
        let before = deck.cards();
        backend.shuffle_deck(&mut deck);
        let after = deck.cards();

        assert_eq!(before.len(), after.len());
        assert_ne!(before, after, "shuffle with a fixed seed should reorder");
        let mut sorted_before = before.clone();
        let mut sorted_after = after.clone();
        sorted_before.sort();
        sorted_after.sort();
        assert_eq!(
            sorted_before, sorted_after,
            "shuffle must permute, not lose or duplicate cards"
        );
    }

    // `RealBackend`'s new methods all fall back to `extra_rng` (no real
    // `Instance` equivalent wired up yet, see each method's doc comment) -
    // this is a smoke test that the fallback wiring is complete and doesn't
    // panic, not a claim of real-Balatro accuracy.
    #[test]
    fn real_backend_fallback_methods_all_work() {
        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut backend = RealBackend::new("TESTSEED", fast);

        let mut deck = crate::deck::Deck::default();
        let before = deck.cards();
        backend.shuffle_deck(&mut deck);
        assert_ne!(before, deck.cards());

        // GlassCardBreak is the one proc with no real node, so it exercises
        // the fast-stream fallback.
        assert!(!backend.prob_roll(EffectProc::GlassCardBreak, 0, 10));
        assert!(backend.prob_roll(EffectProc::GlassCardBreak, 10, 10));

        let (small, big) = backend.draw_ante_tags(1);
        assert!(Tag::iter().any(|t| t == small));
        assert!(Tag::iter().any(|t| t == big));

        let planetarium = Planetarium::new();
        let _ = backend.roll_random_planet(&planetarium, &[]);
        let _ = backend.roll_random_tarot(&[]);

        let joker = backend.gen_joker(1, 1, &[]);
        assert!(joker.is_implemented());
    }

    // `explore -- TEST` reports `Tags: Investment Tag, Speed Tag` for
    // ante 1. Two independent `next_tag` draws off the `Tag{ante}` node.
    #[test]
    fn real_backend_draw_ante_tags_matches_explore() {
        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut backend = RealBackend::new("TEST", fast);
        backend.on_run_start();
        backend.on_ante_start(1);
        assert_eq!(
            backend.draw_ante_tags(1),
            (Tag::Investment, Tag::Speed),
            "core's real-mode tags must match `explore -- TEST` ante 1"
        );
    }

    // `Instance::init_locks` is what keeps ante-1-illegal tags (Negative,
    // Standard, Meteor, Buffoon, Handy, Garbage, Ethereal, Top-up, Orbital)
    // out of ante 1. It only works if `on_run_start` runs before the draw.
    #[test]
    fn real_backend_ante_one_never_draws_a_late_unlock_tag() {
        let ante_one_illegal = [
            Tag::Negative,
            Tag::Standard,
            Tag::Meteor,
            Tag::Buffoon,
            Tag::Handy,
            Tag::Garbage,
            Tag::Ethereal,
            Tag::TopUp,
            Tag::Orbital,
        ];
        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut backend = RealBackend::new("TEST", fast);
        backend.on_run_start();

        for ante in 1..=40 {
            backend.on_ante_start(ante);
            let (small, big) = backend.draw_ante_tags(ante);
            if ante == 1 {
                assert!(
                    !ante_one_illegal.contains(&small) && !ante_one_illegal.contains(&big),
                    "ante 1 drew {small:?}/{big:?}"
                );
            }
        }
    }

    // Each effect proc must read its own node, so two procs rolling the same
    // odds from one `RealBackend` are independent draws — and both
    // reproducible across instances. If any of them silently fell through
    // to the fast stream this would drift between constructions.
    #[test]
    fn real_backend_proc_rolls_are_per_effect_and_deterministic() {
        fn run() -> Vec<bool> {
            let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
            let mut backend = RealBackend::new("TESTSEED", fast);
            (0..40)
                .flat_map(|_| {
                    [
                        backend.prob_roll(EffectProc::LuckyMult, 1, 5),
                        backend.prob_roll(EffectProc::LuckyMoney, 1, 15),
                        backend.prob_roll(EffectProc::BusinessCard, 1, 2),
                        backend.prob_roll(EffectProc::Bloodstone, 1, 2),
                    ]
                })
                .collect()
        }
        assert_eq!(run(), run());
    }

    // The real node IDs are per-effect, so Lucky Mult and Lucky Money must
    // not agree roll-for-roll even though both are 1-in-n on the same cards.
    #[test]
    fn real_backend_lucky_mult_and_money_are_separate_streams() {
        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut backend = RealBackend::new("TESTSEED", fast);
        let mut identical = 0;
        for _ in 0..200 {
            let mult = backend.prob_roll(EffectProc::LuckyMult, 1, 5);
            let money = backend.prob_roll(EffectProc::LuckyMoney, 1, 15);
            if mult == money {
                identical += 1;
            }
        }
        // Independent Bernoulli draws agree ~2/3 of the time; identical 200
        // times over would mean they share a node (or are a constant).
        assert!(
            identical < 200,
            "lucky_mult and lucky_money never diverged — they share a node"
        );
    }

    // `prob_mult` is applied by `Game::prob_roll` before it reaches the
    // backend, so a boosted roll should hit more often than a 1-in-5.
    #[test]
    fn real_backend_proc_roll_respects_its_odds() {
        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut backend = RealBackend::new("TESTSEED", fast);
        let hits = (0..2000)
            .filter(|_| backend.prob_roll(EffectProc::LuckyMult, 1, 5))
            .count();
        // 1/5 of 2000 is 400; allow a wide band so this can't flake.
        assert!((300..500).contains(&hits), "1-in-5 gave {hits}/2000");

        let never = (0..200)
            .filter(|_| backend.prob_roll(EffectProc::LuckyMult, 0, 5))
            .count();
        assert_eq!(never, 0);
    }

    #[test]
    fn real_backend_draw_boss_matches_category() {
        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut backend = RealBackend::new("TESTSEED", fast);
        for ante in [1, 7, 8, 9, 16] {
            let boss = backend.draw_boss(ante);
            assert_eq!(
                boss.is_finisher(),
                ante % 8 == 0,
                "ante {ante} drew {boss:?}"
            );
        }
    }

    #[test]
    fn real_backend_set_showman_propagates() {
        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut backend = RealBackend::new("TESTSEED", fast);
        assert!(!backend.instance.params.showman);
        backend.set_showman(true);
        assert!(backend.instance.params.showman);
        backend.set_showman(false);
        assert!(!backend.instance.params.showman);
    }

    // Structural check on gen_pack's typed plumbing across many draws.
    #[test]
    fn real_backend_gen_pack_contents_match_category_and_count() {
        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut backend = RealBackend::new("TESTSEED", fast);
        let planetarium = Planetarium::new();
        let vouchers = Vouchers::new();
        let mut seen_categories: std::collections::HashSet<PackCategory> =
            std::collections::HashSet::new();

        for ante in 1..=300 {
            let ctx = ShopContext::for_test(&planetarium, &vouchers, ante);
            let pack = backend.gen_pack(&ctx, None);
            seen_categories.insert(pack.category);

            let expected_count = balatro_seed::pack_card_count(pack.category, pack.size);
            assert_eq!(
                pack.contents.len() as i32,
                expected_count,
                "ante {ante}: {:?}/{:?} should hold {expected_count} cards",
                pack.category,
                pack.size
            );

            for content in &pack.contents {
                let matches = match pack.category {
                    PackCategory::Buffoon => matches!(content, PackContent::Joker(_)),
                    PackCategory::Standard => matches!(content, PackContent::PlayingCard(_)),
                    PackCategory::Spectral => matches!(content, PackContent::Spectral(_)),
                    PackCategory::Arcana => {
                        matches!(content, PackContent::Tarot(_) | PackContent::Spectral(_))
                    }
                    PackCategory::Celestial => {
                        matches!(content, PackContent::Planet(_) | PackContent::Spectral(_))
                    }
                };
                assert!(
                    matches,
                    "ante {ante}: {:?} pack produced unexpected content {content:?}",
                    pack.category
                );
            }
        }

        assert_eq!(
            seen_categories.len(),
            5,
            "expected all 5 pack categories across 300 draws, saw {seen_categories:?}"
        );
    }

    // regression: could offer a tier-2 upgrade before its base was ever redeemed.
    #[test]
    fn real_backend_gen_voucher_never_offers_an_ungated_upgrade() {
        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut backend = RealBackend::new("TESTSEED", fast);
        let vouchers = Vouchers::new();
        let mut saw_any = false;

        for ante in 1..=300 {
            if let Some(v) = backend.gen_voucher(ante, &vouchers) {
                saw_any = true;
                assert!(
                    vouchers.is_offerable(v),
                    "ante {ante}: offered {v:?} with its base not owned"
                );
            }
        }

        assert!(saw_any, "no voucher drawn across 300 antes");
    }

    // `seed_joker_with_id` is `RealBackend`'s equivalent mint chokepoint to
    // `JokerGenerator::gen_joker` (Fast mode) - Castle/MailInRebate must
    // come out with a selector already set via `RealBackend`'s own
    // `extra_rng`, not left `None`. Calls it directly with constructed
    // jokers rather than sampling real-seed shop/pack generation and hoping
    // to land on these two specific jokers among real Balatro's full
    // 150-joker pool, which isn't reliably hit in any bounded sample.
    #[test]
    fn real_backend_seed_joker_with_id_rolls_discard_selector() {
        use crate::joker::{Castle, MailInRebate};

        let fast = FastBackend::new(ChaCha8Rng::seed_from_u64(1));
        let mut backend = RealBackend::new("TESTSEED", fast);
        let castle = seed_joker_with_id(Jokers::Castle(Castle::default()), &mut backend.extra_rng);
        assert!(
            castle.state().selector.is_some(),
            "Castle minted with no selector"
        );
        let mail = seed_joker_with_id(
            Jokers::MailInRebate(MailInRebate::default()),
            &mut backend.extra_rng,
        );
        assert!(
            mail.state().selector.is_some(),
            "MailInRebate minted with no selector"
        );
    }
}

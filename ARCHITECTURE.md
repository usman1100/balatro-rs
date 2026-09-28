# Architecture

## Crate graph

```
                        balatro-types
                       (pure vocabulary)
                             |
        +---------+-----------+-----------+-----------+----------+
        |         |           |           |           |          |
  balatro-jkr   balatro-seed   core (balatro-rs)          |          |
        |                       ^     ^                  |          |
  balatro-profile              |     |                  |          |
        |                       |     +---> cli  -------+          |
        |                       |     +---> tui                     |
        |                       |     +---> pylatro                 |
        +-----------------------+                                  |
                                        balatro-types  <------------+
```

Dependencies are acyclic and shallow:

- `balatro-types` — pure data. Every enum, id, name, cost, rarity, description.
  No behavior, no file-format knowledge, no dependencies except `strum`
  (+ optional `serde` / `pyo3`). This is the crate everything else agrees on.
- `balatro-jkr` — codec for the `.jkr` container: raw-DEFLATE-compressed Lua
  table literals. Deliberately vocabulary-free; it knows tables, strings,
  numbers, bools, nil and nothing about Balatro.
- `balatro-seed` — byte-accurate port of the real seeded-RNG scheme.
  Depends on `balatro-types` only. Ships the `explore` binary.
- `balatro-profile` — reads real `meta.jkr` / `profile.jkr` / `save.jkr` into
  typed structs, bridging `balatro-jkr` to `balatro-types`. Import-only.
  Ships the `profile` binary.
- `core` (`balatro-rs`) — the rules engine. All gameplay behavior lives here.
  Depends on `balatro-types` and `balatro-seed`.
- `cli`, `tui`, `pylatro` — three frontends over `core`. None of them contain
  game rules.

## Why `balatro-seed` and `core` are separate

They are two different problems and are deliberately not fully merged:

- `core` needs *an* RNG to drive a simulation. It uses a plain
  `rand_chacha::ChaCha8Rng` seeded from a `u64`. That is fast, reproducible,
  and completely sufficient for RL. This is `RngMode::Fast`.
- Reproducing *what a specific real Balatro seed actually generates* is a much
  harder problem. The real game hashes the seed string, then re-seeds a fresh
  xoshiro256\*\* `LuaRandom` for **every individual decision**, threading a
  mutable per-node cache so that rerolling a shop advances that node's stream
  and nothing else. That is `balatro-seed`.

`core` reaches for `balatro-seed` through the `RngBackend` trait
(`core/src/rng.rs`), selected by `Config::rng_mode`. `RngMode::Real` is only
partially complete — see the coverage table below.

## The `RngBackend` seam

```rust
trait RngBackend {
    fn gen_shop_item(&mut self, ...) -> GeneratedItem;
    fn gen_pack(&mut self, ...) -> Pack;
    fn gen_voucher(&mut self, ante: Ante) -> Option<Voucher>;
    fn draw_boss(&mut self, ante: Ante) -> Option<BossBlind>;
    fn shuffle_deck(&mut self, deck: &mut Deck);
    fn prob_roll(&mut self, proc: EffectProc, numerator: u32, denominator: u32) -> bool;
    fn draw_ante_tags(&mut self, ante: i32) -> (Tag, Tag);
    fn on_run_start(&mut self);
    fn on_ante_start(&mut self, ante: i32);
    fn on_joker_bought(&mut self, ...);   // lock / unlock tracking
    // ...
}
```

`Backend` is either `Fast(FastBackend)` or `Real(RealBackend)`.
`RealBackend` owns a `balatro_seed::Instance` plus a nested `FastBackend` it
falls back to for calls that are not yet ported. Every such fallback is a
real-mode accuracy gap, not a deliberate simplification.

### `RngMode::Real` coverage

The real game splits its randomness in two. Node-seeded draws reseed a fresh
generator from a hash of a decision-specific node ID, so they are individually
reproducible. Bare `math.random` calls with no reseed instead consume whatever
stream the last reseed left behind, so they depend on exact call history and
cannot be ported at all.

Ported, and verified against `explore` output:

| Draw | Node |
| --- | --- |
| shop items, packs, pack contents | `cdt{ante}`, `shop_pack{ante}`, … |
| vouchers | `Voucher{ante}` |
| tags | `Tag{ante}`, gated by the ante-1 lock table |
| bosses | `boss`, gated by the same lock table |
| joker / tarot / planet / spectral generation | `rarity`, `edi`, `Joker1..4`, `Tarot…` |
| effect procs | one node per effect — see `balatro_seed::ProcNode` |

`RngMode::Real` is still incomplete:

- **deck shuffle.** The shuffle itself is portable (`pseudoshuffle(cards,
  pseudoseed('shuffle'))`, a plain Fisher-Yates) but the real game sorts the
  deck by a `sort_id` assigned in table-declaration order first. That order
  isn't recoverable from the 1.0.0 source dump, so a port would shuffle
  correctly and still deal the wrong cards.

`sort_id` is the recurring blocker: `pseudoshuffle` and
`pseudorandom_element` both sort their input by it, and it is a global counter
bumped in source-declaration order, so recovering it means recovering the exact
declaration order of every card and joker in the game.
- **Glass Card's shatter roll.** The only effect proc without a node ID, for
  the same reason: the playing-card centre definitions aren't in the dump.
- **The Crimson Heart's victim pick.** Its node (`crimson_heart`) is known, but
  the real game sorts the candidate pool by each Joker's `sort_id` — definition
  order — before rolling, and that ordering isn't recoverable.
- **Amber Acorn's joker shuffle.** Node `aajk` is known; the pre-shuffle order
  isn't. The permutation is valid, just not the exact one.
- **Joker, tarot and planet draws triggered by gameplay** — Judgement, the
  Top-up tag, the blue seal's planet — where the real game's source code for
  the node ID isn't available.
- **Sticker and shop-playing-card generation**, which `balatro-seed` doesn't
  model at all.

`balatro_seed::NODE_SOURCE_VERSION` records that the `ProcNode` keys were
transcribed from a 1.0.0 source dump while `InstParams::version` targets
1.0.1f. Nothing in that set has been validated against real game output.

## Game state and the action model

`Game` (`core/src/game.rs`) is the single source of truth for a run: config,
shop, planetarium, deck, hand, jokers, consumables, tags, vouchers, blind,
ante, money, and the run-history counters some jokers key off.

A run is a state machine over `Stage` (`core/src/stage.rs`):

```
PreBlind -> Blind(Small) -> Blind(Big) -> Blind(Boss) -> PostBlind
                                                          |
   End(Win/Lose) <-- ante exhausted                      v
                                                     Shop
                                                          |
   Tag / PackOpen / TarotHand / SpectralHand  <-----------+
```

Two independent APIs drive it, and both end at the same `handle_action`:

1. `gen_actions() -> impl Iterator<Item = Action>` — variable-length, ergonomic,
   for humans and scripted agents.
2. `gen_action_space() -> ActionSpace` — a fixed 109-slot binary mask
   (`core/src/space.rs`) for RL environments that need a stable action-space
   shape across states.

Both must be kept in sync. `Action` has 25 variants (`core/src/action.rs`) and
each one needs a `gen_actions_*` method in `generator.rs` and an
`unmask_action_space_*` counterpart. `core/src/space.rs` documents the exact
slot layout; changing it is a breaking change for any trained policy.

## The effect system

Jokers do not implement a trait directly. `balatro-types` generates a `Jokers`
enum via the `make_jokers!` macro, and `core/src/joker.rs` attaches behavior with
a single `match self` that returns `Vec<Effects>` (`core/src/effect.rs`):

| Effect | Fired where |
| --- | --- |
| `OnDiscard` | `game.rs:497` |
| `OnScore` | inline in the scoring loop, deliberately not cached |
| `OnSell` | `game.rs:1013` |
| `OnModifyHand` | `game.rs:814` |
| `OnRoundEnd` | `game.rs:325` |
| `TriggerCountPlayed` | `game.rs:614` |
| `TriggerCountHeld` | `game.rs:630` |
| `RuleFlag` | read by `is_face_card` (Pareidolia) |

`EffectRegistry` caches the rest; `OnScore` is recomputed inline because the
edition bonus must sandwich each joker's own contribution.

`OnPlay` and `OnHandRank` used to be in this table, marked as never fired. They
were removed rather than implemented — see [Effects](#effects) below for why
every joker that wanted them is already covered.

`is_implemented()` is an explicit whitelist, and it is the only thing keeping a
joker whose effects are missing out of shops and packs. A joker can therefore
be fully data-complete and still be unplayable.

The whitelist is now complete: all 150 jokers are in it. It is kept as a
whitelist rather than derived because a joker's behaviour can live in a
hand-rolled hook — `Game::joker_first_hand`, `joker_pack_opened`,
`joker_blind_start`, `shop_end_effects` — where `effects()` returns nothing at
all, and there is no way to see that from the enum.

Two tests keep that honest. `test_joker_implementation_count_is_tracked` pins
the count against a constant, so landing a joker without updating the number
fails. `test_every_registered_effect_has_a_firing_site` walks all 150 and
fails if one registers an effect the game never reads, which is the other way
a joker can silently do nothing.

## Scoring pipeline

`calc_score_inner` (`core/src/game.rs`) runs in a fixed order, and the order
matters:

1. Hand level from the planetarium (boss deleveling happens first).
2. Observatory voucher: held matching planets.
3. Each played card, in play order, retriggered N times where N accounts for
   Red Seal plus every `TriggerCountPlayed` joker. Cards can have their effects
   cancelled by a boss debuff.
4. Stone kickers — even outside the scoring subset.
5. Held-in-hand cards, retriggered by `TriggerCountHeld`.
6. `OnModifyHand` hooks.
7. Jokers in slot order, each wrapped in its own edition bonus.
8. `score = chips * mult`, then chips and mult reset to the configured base.

`core/src/score.rs` is not where the math lives; it is the `ScoreTrace` that
records every step, used by the `calc` binary to explain a score.

## Decks and stakes

`Config` carries a `deck: DeckVariant` and a `stake: Stake`, defaulting to Red
Deck and White Stake, so an existing caller is unaffected. `core/src/deck_variant.rs`
holds the rules; `Game` reads them through four seams — the starting card set,
the per-round resources, the slot counts, and `calc_reward`.

**The base values moved.** The real game's `get_starting_params` starts from 3
discards, and the Red Deck's `discards = 1` is what makes an ordinary run have
4. `Config::discards` had been 4 all along, meaning it already included the
deck modifier. Applying the deck on top of that gave 5, so `DEFAULT_DISCARDS`
is now 3 and every deck's own delta is counted once. This is the kind of thing
that looks fine until a joker reads the number: Green Joker and Trading Card
both compared against `config.discards` to work out how much of the round had
been used, and both were silently off by one. They now use
`discards_per_round()`.

**Two limits are derived, not stored.** `plays_per_round` and
`discards_per_round` read the deck and the stake, so a deck modifier reaches
every round without anything having to remember to apply it. `Game::new` seeds
both from the same functions, because the pre-blind state would otherwise
start a round short of the deck's own grant.

**Ante bases are per-scaling tables, not a formula.** Green and Purple stakes
and the Plasma Deck each raise `modifiers.scaling`, and the real game holds a
separate eight-entry table for each of the three levels rather than
multiplying one. `deck_variant::ante_base` transcribes all three from
`get_blind_amount`; `effective_scaling` takes the higher of the deck's and the
stake's, since both write the same field.

**Stickers are what the upper stakes do.** The Black, Orange and Gold stakes
enable eternal, perishable and rental stickers, which `core/src/sticker.rs`
implements. Two details are easy to get wrong:

The eternal/perishable roll is a *single* roll with three outcomes — `> 0.7`
Eternal, `(0.4, 0.7]` Perishable, otherwise neither — so the two are mutually
exclusive by construction and each is 30% with 40% clean. Rental is a
separate roll and does stack with either. The per-joker compat flags matter
too: `set_eternal` and `set_perishable` each refuse when the joker is not
compatible for that sticker, and each refuses when the other is already set, so
an incompatible joker ends up with nothing rather than the wrong sticker.

An expired Perishable is **debuffed, not removed**. The joker keeps its slot
and its sell value and still counts for Gift Card, Swashbuckler and
Temperance; it just stops contributing. That is why nothing drops it at
expiry and `is_joker_debuffed` reads the tally instead.

A Rental's charge is applied in `calc_reward`, before the payout, so it can
reduce the interest earned on the round — the wiki is explicit about that
and it is why the charge is not in the round-end tick.

**What the deck and stake still do not change.** The Erratic Deck randomises
each card's rank and suit individually; the implementation reshuffles the
existing values across the 52 cards instead, which is indistinguishable once
the deck is shuffled but is not the real per-card draw. It is noted in
`CLAUDE.md` rather than left to be discovered.

## Coverage

Behaviour implemented in `core`:

- hand identification and scoring, all 13 ranks
- all 15 deck variants and all 8 stakes
- the three in-run stickers (eternal, perishable, rental)
- play / discard / reorder / sort
- blind and ante progression, win and loss
- money, interest, rewards
- shop: jokers, consumables, playing cards, packs, vouchers, rerolls
- all 22 tarots, all 18 spectrals, all 12 planets
- all 8 enhancements, all 5 editions, all 4 seals
- all 24 tags
- all 32 vouchers
- all 28 boss blind modifiers
- all 150 jokers
- effect procs in real RNG mode (one node per effect)

Boss blind modifiers split three ways. Most are stateless: a debuff is a pure
function of the card, so `Game::is_card_debuffed` is the whole story. A handful
need to remember something between hands or across an ante, and that lives in
`BossState` (`core/src/boss.rs`) — The Eye's last rank, The Mouth's lock, The
Crimson Heart's victim, Pillar's per-ante played set. The rest is `Game::begin_round`
(boss setup when a round opens) and `Game::boss_on_play` (what fires after a hand
scores). Three bosses disable a blind outright — Chicot held, Luchador sold, any
Joker sold under Verdant Leaf — and the real game routes all three through one
`Blind:disable()`, so `Game::boss_disabled` is a single flag rather than one per
cause.

Tags all fire from `Game::apply_tag` at cashout, which is when the real game
pops them. Most of them shape the shop that cashout is about to stock, so they
run after the stage becomes `Shop` and before `Shop::refresh`; effects aimed
at a later blind or round are parked in `TagState` (`core/src/tag.rs`) and
consumed by the shop / boss / round hooks.

Not implemented:

- deck variants (the type exists in `balatro-types`, `core` never reads it)
- stakes (same)
- sticker behavior (eternal / perishable / rental are data only)
- partial hands and face-down cards (Four Fingers, Shortcut, Smeared Joker,
  and the Fish / House / Mark / Cerulean Bell bosses depend on these)
- the remainder of `RngMode::Real` (deck shuffle, Glass Card, gameplay-
  triggered draws) — see the coverage table above

## Testing

- `cargo test` — the whole workspace, around 820 tests. Most live beside the
  code in `#[cfg(test)]` modules; `core/tests/` holds the end-to-end ones and
  `balatro-seed/tests/golden.rs` the seed fixtures.
- `BALATRO_SOAK_GAMES=2000 cargo test -p balatro-rs --test test` — the soak.
  Every run is seeded, so a failure replays; the two move APIs each play
  complete runs and assert that every action they offer executes.
- `cargo run -p balatro-seed --bin explore -- SEED --ante N` prints a seed's
  expected contents in the same text format as the TheSoul/Immolate reference
  site, so output can be diffed directly against the real game. Its output is
  pinned under `balatro-seed/tests/golden/`; see `TEST_PLAN.md` for what
  those fixtures do and do not prove.
- `TEST_PLAN.md` is the audit that drove the current test suite: what was
  unverified, what was wrong, and what is deliberately still open.
- `cargo bench -p balatro-rs` — plays a whole game on random moves.
- CI runs clippy with `-D warnings` and `cargo fmt --check`, so neither can
  rot, plus the soak at 2000 and a real Python build.

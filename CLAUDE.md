# balatro-rs

Game engine and move generator for Balatro, written in Rust with Python bindings. The goal is full compatibility with the real game: same rules, same scoring, and for a given seed the same generated shops, packs, vouchers, bosses and tags.

## Workspace

Cargo workspace with eight members — see `ARCHITECTURE.md` for the full crate graph and design rationale:

- `core/` — main game engine library (`balatro-rs` crate), where almost all logic lives
- `cli/` — interactive terminal game (`balatro-cli`), depends on core
- `tui/` — interactive terminal UI (ratatui-based), depends on core
- `pylatro/` — PyO3 Python bindings, depends on core
- `balatro-types/` — neutral vocabulary (`Card`, `Jokers`, `Tarot`, etc.), no behavior, no file-format knowledge
- `balatro-jkr/` — codec for Balatro's `.jkr` save-file format (Lua table literals)
- `balatro-profile/` — reads real save/profile files into typed data via `balatro-jkr` + `balatro-types`; read-only, ships a `profile` CLI binary
- `balatro-seed/` — byte-accurate port of Balatro's actual seed/RNG algorithm (pseudohash + per-node LuaRandom reseeding, not the simplified `rand_chacha` stream `core` uses); depends on `balatro-types` only, ships an `explore` CLI binary

## Build & Test

```bash
# run all tests (all workspace members)
cargo test

# core library only
cargo test -p balatro-rs

# run benchmarks
cargo bench -p balatro-rs

# build Python bindings (requires maturin)
cd pylatro && maturin develop

# explore a seed's full expected contents (byte-accurate vs. real Balatro),
# same text format as the TheSoul/Immolate reference site for direct diffing
cargo run -p balatro-seed --bin explore -- SEED [--ante N]
```

Core has optional features; default is `["serde"]`. To build with the Python bindings:

```bash
cargo build -p balatro-rs --features python
```

## What `core`'s game engine does and doesn't implement

Implemented: poker hand scoring, playing/discarding/reordering cards, blind progression (small/big/boss), ante progression (1-8), money/interest, all 15 deck variants and all 8 stakes, buying/selling jokers, tarots, planets, spectrals, card enhancements/editions/seals in effect, all 24 tags, all 28 boss blind modifiers, all 32 vouchers, and the three
in-run stickers. All 150 jokers have real behavior.

Not implemented: the Erratic Deck's per-card randomization, which is a
deterministic reshuffle rather than the real per-card draw. Stickers (Eternal,
Perishable, Rental) are implemented end to end in `core/src/sticker.rs`, so the
Black/Orange/Gold stakes now have a visible effect. `DeckVariant` and `Stake` are plumbed through `Config` and applied; see
`core/src/deck_variant.rs`. `Effects::OnPlay` and `Effects::OnHandRank` were
removed: no joker produced either and nothing fired them.
`joker.rs::test_every_registered_effect_has_a_firing_site` now keeps the remaining
variants honest.

Partial hands, face-down cards and the four partial-hand jokers (Splash, Four Fingers, Shortcut, Smeared Joker) are all implemented. Every joker now has real behaviour; Perkeo works because `Consumable` carries an `Edition`.

A Negative consumable is a distinct card from its base version, so equality includes the edition and the shop will still offer a base copy of something you hold a Negative of. It costs $5 more, sells for more, and takes up no consumable slot — the real game raises the consumable card limit when the edition is applied, which nets out to the card being free to hold.

`Game::money` is an `i64` and can go negative, because Credit Card buys a debt allowance. `Game::bankrupt_at` is the floor — 0 by default, -$20 per Credit Card — and every affordability check goes through `spendable_balance()`, which is `money - bankrupt_at` clamped at zero. Only a purchase can open the allowance; a joker that charges money (Tooth) is clamped to the floor instead.

`core` can drive generation through `balatro-seed` via `RngMode::Real`, but the port is partial. Real: shop items, packs, pack contents, vouchers, tags, bosses, joker/tarot/planet/spectral draws, and effect procs. Still on the simplified `rand_chacha` stream: deck shuffle, Glass Card's shatter roll, gameplay-triggered joker/tarot/planet draws, and shop-playing-card generation. Sticker rolls use the
`etperpoll`/`ssjr` thresholds transcribed from the real game but read from the
`rand_chacha` stream, since `balatro-seed` has no nodes for them. So a real-mode run is not yet byte-accurate end to end. `balatro_seed::NODE_SOURCE_VERSION` records that the effect-proc node IDs were transcribed from a 1.0.0 source dump and are unvalidated.

## Two public APIs

`Game` exposes two ways to get and execute moves:

1. `gen_actions() -> impl Iterator<Item = Action>` — variable-length, ergonomic
2. `gen_action_space() -> ActionSpace` — fixed-size binary mask vector for RL environments

Both call `handle_action(action)` to execute moves.

## Dev

Keep comments very concise, not verbose, clear code is better than commented code. Self documenting. If does need comment, keep it 1 sentence, 2 max.

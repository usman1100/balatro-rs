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

Implemented: poker hand scoring, playing/discarding/reordering cards, blind progression (small/big/boss), ante progression (1-8), money/interest, buying/selling jokers, tarots, planets, spectrals, card enhancements/editions/seals in effect, all 24 tags, all 28 boss blind modifiers, 29/32 vouchers. 149/150 jokers have real behavior.

Not implemented: alternative decks/stakes (`core` never reads `DeckVariant` or `Stake`), sticker behavior (eternal/perishable/rental are data only), the remaining 2 jokers. Three vouchers (Omen Globe, Director's Cut, Retcon) are inert. `Effects::OnPlay` and `Effects::OnHandRank` are collected but never fired.

**Perkeo** is the one outstanding joker, blocked on a type rather than on logic: it needs an `Edition` on `Consumable`, which the byte-accurate draw pools in `balatro-seed` depend on the shape of. Partial hands, face-down cards and the four partial-hand jokers (Splash, Four Fingers, Shortcut, Smeared Joker) are all implemented.

`Game::money` is an `i64` and can go negative, because Credit Card buys a debt allowance. `Game::bankrupt_at` is the floor — 0 by default, -$20 per Credit Card — and every affordability check goes through `spendable_balance()`, which is `money - bankrupt_at` clamped at zero. Only a purchase can open the allowance; a joker that charges money (Tooth) is clamped to the floor instead.

`core` can drive generation through `balatro-seed` via `RngMode::Real`, but the port is partial. Real: shop items, packs, pack contents, vouchers, tags, bosses, joker/tarot/planet/spectral draws, and effect procs. Still on the simplified `rand_chacha` stream: deck shuffle, Glass Card's shatter roll, gameplay-triggered joker/tarot/planet draws, sticker and shop-playing-card generation. So a real-mode run is not yet byte-accurate end to end. `balatro_seed::NODE_SOURCE_VERSION` records that the effect-proc node IDs were transcribed from a 1.0.0 source dump and are unvalidated.

## Two public APIs

`Game` exposes two ways to get and execute moves:

1. `gen_actions() -> impl Iterator<Item = Action>` — variable-length, ergonomic
2. `gen_action_space() -> ActionSpace` — fixed-size binary mask vector for RL environments

Both call `handle_action(action)` to execute moves.

## Dev

Keep comments very concise, not verbose, clear code is better than commented code. Self documenting. If does need comment, keep it 1 sentence, 2 max.

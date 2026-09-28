# Test plan

Everything in here is a gap that was found by reading, not by guessing. Each
item names the risk, the change, and how we know it is fixed. Ordered by
risk, not by effort.

Status legend: `TODO` / `DOING` / `DONE` / `WONTFIX` (with a reason).

## Tier 0 — defects found by reading, fix before anything else

| # | Risk | Where | Status |
|---|---|---|---|
| 0.1 | `to_action` has no arm for `reroll_boss`; the mask advertises an action that always fails | `space.rs:464` | DONE |
| 0.2 | The Fish used run totals for a round-scoped rule. Now a sticky `prepped` flag, transcribed from `bl_fish` | `game.rs:1333`, `boss.rs:27` | DONE |
| 0.3 | `Tag::Economy` could push `money` below `debt_floor()`; clamped and pinned in both directions | `game.rs:1875` | DONE |
| 0.4 | `GameError::InvalidMoveCard` unreachable; `NoCardMatch` already covers it | `error.rs` | DONE |
| 0.5 | `Config` had no `serde(default)`, so one missing key lost the run. Every field now defaults to the real value | `config.rs:56` | DONE |
| 0.6 | `calc` scores with default hand rules, so it disagrees with the engine when Four Fingers / Shortcut / Smeared are held | `bin/calc.rs:48` | TODO |
| 0.7 | `money_max` and `discarded_max` are never enforced; `chips * mult` is an unchecked `usize` multiply | `game.rs:1698` | TODO |
| 0.8 | `Jokers::eq` ignores `state.counter` and `sell_bonus`, so a save round trip can silently lose accumulator state while the natural assertion passes | `joker.rs:113` | TODO |
| 0.9 | Comment in `boss.rs` claimed `played_this_ante` was rebuilt on load; it is serialized | `boss.rs:34` | DONE |

## Tier 1 — the invariants that matter most

### 1.1 The two move APIs must agree
`gen_actions()` and `gen_action_space()` are the crate's two public contracts
and nothing checks they agree beyond one consumable test. `DeselectCard` has
no action-space bucket at all, which is a structural asymmetry that needs a
documented decision, not a test.

- Conversion: every unmasked index resolves. DONE (`space.rs`)
- Execution: every generated action executes on a fresh copy. DONE (`space.rs`)
- Both verified to fail when the `to_action` arm is removed.
- Set equality: `gen_actions()` == indices the space unmask, per variant.
- `DeselectCard`: either add a bucket or document why not.
- Amounts compared, not just membership.

### 1.2 Save / load
No test round-trips a *played* game. Everything round-trips a fresh one, so
an open pack, a populated `boss_state`, mutated deck cards, `planetarium`
levels and `action_history` are all unverified. Also: the RNG is **not**
round-tripped (`backend` is `serde(default)`), so a reloaded `RngMode::Real`
run is not reproducible. Decide whether that is acceptable, then test it.

- Fully-played run round trip.
- `to_json -> from_json -> to_json` stability.
- `Jokers::eq` fixed first, or the round trip cannot detect losses (0.8).
- A save missing every optional `Config` key.

### 1.3 Known-answer tests for `balatro-seed`
"byte-accurate" currently rests on manual diffing. There is not one golden
test in the crate; `node_id_strings_match_immolate` and
`proc_node_strings_match_the_game` are circular — they compare the table
against a copy of itself.

- Pin `explore -- TEST --ante N` output to fixture files (note `.gitignore`
  excludes `*.json`, so fixtures need another extension or a scoped rule).
- Pin `pseudohash` for known seeds against the Lua.
- Pin a few `ProcNode` draws.
- Note the three latent panics: `randweightedchoice` sentinel assumption, the
  empty-pool index, and `voucher_upgrade`'s hand-maintained pairing.

## Tier 2 — coverage that is thin rather than wrong

- 8 of 12 hand `level_step` values unpinned; all 150 joker costs unpinned;
  most tarot/planet/spectral/tag/voucher costs unpinned. One test per
  table, mirroring `deck_variant.rs:289`.
- `BossBlind::Fish` is the only boss with no test reference — and it holds
  0.2.
- Lucky enhancement, Wild, Negative-as-a-played-card, Blue/Purple seals during
  scoring.
- 8 of 32 vouchers have no test reference, three of them with live effects.
- 55 of 150 jokers have no test reference. Four Fingers / Smeared / Shortcut
  are only tested by constructing `HandRules` by hand, never by owning the
  joker, so `hand_rules()` wiring is unverified.

## Tier 3 — tests that lie

| Test | Problem |
|---|---|
| `balatro-jkr/tests/roundtrip.rs:42` | iterates an empty fixture list and passes vacuously; sibling test has a non-empty guard, this one does not |
| `balatro-profile` real-data tests (5) | `else { return }` on a missing fixture, so all five are no-ops today |
| `core/tests/test.rs:65,127` | use `thread_rng()`, unseeded, and assert almost nothing |
| `core/tests/deck_stake.rs:257,305` | `if handle_action(...).is_err() { break }` silently ends the run; assertions are trivially true |
| `core/src/lib.rs:507` | `.expect("valid index to action")` on a random index — this is the flake that 0.1 would have caused |
| `pylatro` | 0 tests; CI's `cargo test -p pylatro` asserts nothing (cdylib) |
| `balatro-types/tests/python_smoke.rs` | `#![cfg(feature = "python")]`, so `cargo test` reports "0 passed" and the 5 tests never run |

## Tier 4 — error paths

11 of 18 `GameError` variants, all 6 `PlayHandError` and all 3
`ActionSpaceError` are never exercised. `hand.rs:208 UnknownHand` is the only
hand-evaluation failure path and `play_is_legal` swallows it with `let Ok`.

One test per variant. The Eye / Mouth / Psychic errors are currently only
tested by asserting the action is *absent*, which is a different code path
from the one that produces the error.

## Tier 5 — arithmetic

- `game.rs:1698` `chips * mult` unchecked (0.7).
- `game.rs:1854` Economy unclamped (0.3).
- `space.rs:110,134,142` `len() - 1` panics if `available_max == 0`; untested.
- `instance.rs:140,108` and `draws.rs:22` index-out-of-bounds on an empty
  pool or an unpaired voucher.
- `game.rs:1712` Big Blind's 1.5x via `f32`; the test restates the
  implementation rather than pinning a value.

## Tier 6 — process

CI runs `cargo build`, `cargo test`, `cargo test -p pylatro`. It does not run
clippy, fmt, or the `#[ignore]`d soak tests.

- Un-`#[ignore]` the soaks, seed them, and make them assert.
- Add clippy and fmt to CI.
- `pylatro` needs a real test, or the CI line removed because it is a no-op.

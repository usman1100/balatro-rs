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
| 0.6 | `calc` scored with default hand rules, disagreeing with the engine under Four Fingers / Shortcut / Smeared | `bin/calc.rs:44` | DONE |
| 0.7 | `chips * mult` was an unchecked `usize` multiply, fed by Bull from the balance. Now saturating. `money_max`/`discarded_max` are not real caps and are documented as unenforced | `game.rs:1716` | DONE |
| 0.8 | `Jokers::eq`/`Hash` ignored `state.counter` and `sell_bonus`, so a round trip could lose them and `assert_eq!` would still pass. Now every persisted field, with the `f32` counter hashed by bits | `joker.rs:113` | DONE |
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
- Set equality: DONE (`test_the_two_move_apis_offer_the_same_actions`) — walks 25
  seeds and 6 turns each, comparing the two action sets. Verified to fail
  when a bucket is unmasked in one place but not the other.
- `DeselectCard`: DONE — it had no bucket at all, so the two APIs were
  structurally incapable of agreeing. It now has one, `to_action` resolves it
  to the card in that hand position, and the space is 135 long (was 111). The
  bucket is appended so **no existing index moves**, only the length grows.
- Amounts are not compared, only membership. Left open deliberately: a
  difference there is a formatting question, not a contract question.

### 1.2 Save / load
No test round-trips a *played* game. Everything round-trips a fresh one, so
an open pack, a populated `boss_state`, mutated deck cards, `planetarium`
levels and `action_history` are all unverified. Also: the RNG is **not**
round-tripped (`backend` is `serde(default)`), so a reloaded `RngMode::Real`
run is not reproducible. Decide whether that is acceptable, then test it.

- Fully-played run round trip: DONE — five blinds deep, then compares deck,
  available, discarded, jokers, consumables, planetarium, history and stage.
- `to_json -> from_json -> to_json` stability: DONE.
- `Jokers::eq` fixed first: DONE (0.8), so the round trip can now detect it.
- A save missing every optional `Config` key: DONE (0.5).
- Known remaining gap: the RNG backend is `serde(default)`, so a reloaded run
  continues on a *different* stream and a `RngMode::Real` run is not
  reproducible across a save. Not yet decided whether that is acceptable.

### 1.3 Known-answer tests for `balatro-seed`
"byte-accurate" currently rests on manual diffing. There is not one golden
test in the crate; `node_id_strings_match_immolate` and
`proc_node_strings_match_the_game` are circular — they compare the table
against a copy of itself.

- Golden fixtures: DONE. The rendering moved from the `explore` binary into
  `balatro-seed/src/render.rs` so it is callable from a test, and four seeds
  are pinned under `tests/golden/` (`.txt`, since `.gitignore` excludes
  `*.json`).
- `pseudohash` known-answer: DONE. Eight inputs checked against values
  computed by reimplementing the Lua in Python, not read back out of the
  Rust — the first non-circular check in the crate.
- Fixtures alone only pin *stability*, not correctness. `VERIFIED` in the
  golden test is the short list a human has diffed against the reference
  site; it is `TEST` only, and the README says the same. Adding a seed to the
  fixture list does not make it verified.
- Still open: the `node_id_strings_match_immolate` and
  `proc_node_strings_match_the_game` tests are circular (they compare the
  tables against copies of themselves), and the three latent panics —
  `randweightedchoice`'s sentinel assumption, the empty-pool index, and
  `voucher_upgrade`'s hand-maintained pairing — are unfixed.

## Tier 2 — coverage that is thin rather than wrong

- DONE: all 150 joker costs pinned, table-driven by save-file id, verified
  against the game's own centre table with zero mismatches, and verified to
  fail when one is changed. Plus a range check.
- DONE: all 12 scoring hands' level-1 base and per-level step pinned. The
  base values come from the wiki's own "Base Scoring" column and agree with
  the game; the steps are the hand table's `l_chips`/`l_mult`.
- Consumable prices pinned (uniform per type, plus the Negative bump).
- Still unpinned: all 24 tag values, 32 voucher costs (one derivable), 15
  deck starting contents (three covered), 6 pack costs (three covered),
  and the `balatro-seed` pool contents and weights beyond the name check.
- `BossBlind::Fish` now has four tests (0.2).
- DONE: the four partial-hand jokers are now tested by *owning* them rather
  than by hand-constructing `HandRules`, and each is checked to lapse when
  sold. That was the actual untested part — the hand evaluator was covered,
  the registry-to-`hand_rules()` wiring was not.
- DONE: both Lucky rolls (+20 Mult at 1 in 5, $20 at 1 in 15) over 400 seeds.
- DONE: a debuffed card contributes nothing — chip value, enhancement,
  edition and seal all checked against an undebuffed baseline, and against a
  boss that does not debuff. The code was right; nothing checked it.
- Wild cards, and Blue/Purple seals *during* scoring, are still uncovered.
- DONE: Nacho Tong, Recyclomancy and Palette, the three vouchers with live
  effects that had no test, including that they stack with the vouchers
  sharing the same sum.
- Still uncovered: the four Merchant/Tycoon rate vouchers, Tarot Merchant,
  Planet Merchant and Illusion.
- 55 of 150 jokers have no test reference. Four Fingers / Smeared / Shortcut
  are only tested by constructing `HandRules` by hand, never by owning the
  joker, so `hand_rules()` wiring is unverified.

## Tier 3 — tests that lie

| Test | Problem |
|---|---|
| `balatro-jkr/tests/roundtrip.rs:42` | DONE — comment now records why it differs from its sibling: the real files are private, so a non-empty guard would fail CI every run. |
| `balatro-profile` real-data tests (5) | DONE — all five skips now print a note naming the missing directory, instead of passing as if they had run. |
| `core/tests/test.rs` soaks | DONE — seeded, asserting, un-`#[ignore]`d, and the index is drawn from the unmasked entries rather than by rejection sampling. Count via `BALATRO_SOAK_GAMES`, default 200. Verified clean at 4000. |
| `core/tests/deck_stake.rs:257,305` | `if handle_action(...).is_err() { break }` silently ends the run; assertions are trivially true — TODO |
| `pylatro` | 0 tests; CI's `cargo test -p pylatro` asserts nothing (cdylib) — TODO, or drop the line |
| `balatro-types/tests/python_smoke.rs` | DONE — the file-level `#![cfg]` is gone, each test is gated individually, and a test that runs *without* the feature prints a note. 5 pass with `--features python`, 1 skip-note without. |

## Tier 4 — error paths

DONE. Every one of the 18 `GameError`, 6 `PlayHandError` and 3
`ActionSpaceError` variants is now constructed by a test, through the public
API wherever that is possible.

Two needed a note. `TooManyCards` is not reachable through `Action::Play`,
because `select_card` refuses past five first, so it is driven through the
`SelectHand` entry the engine itself calls. `InvalidActionSpace` is only
constructed by `Game::handle_action_index`, which is the entry point an RL
caller actually uses, so it is tested there.

The Eye, Mouth and Psychic were *tested* before, but only by asserting the
action is absent from `gen_actions`, which goes through `play_is_legal` and
discards the error. The branch that produces the error had never run. The
Eye also turned out to key off `last_played` and the Mouth off `locked_rank`
rather than the `first_hand_dealt` I first assumed.

`UnknownHand` is still only reachable by hand-constructing a `SelectHand` the
evaluator cannot classify; it is checked, but there is no gameplay path.

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

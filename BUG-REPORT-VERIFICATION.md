# Verification of BALATRO-RS-BUG-REPORT.md

Each claim was checked against the source rather than taken at face value.
Verdict: **5 valid engine bugs, 3 invalid, 4 presentation.**

One of the three invalid was originally written up here as valid, on a
misreading of the wiki; see item 3.

The report itself is unusually careful — it separates "verified correct" from
"not covered", and it was right about the scoring engine, which matters, since
that is the part everything else is downstream of.

---

## Valid, engine-level

### All five fixed

1. The Hook card leak — b76a761.
2. Starting money — 2e1c4c1.
4. Duplicate Arcana and Spectral pack cards — ec2df77.
10. Shop prices and unaffordable stock — 2ae6f8a.
7. Held tags invisible — 5ec62b4.

### 1. Starting money is $0 — VALID, and it is a transcription error

`DEFAULT_MONEY_START = 0` in `core/src/config.rs:12`. The real game's
`get_starting_params` (`misc_functions.lua:1870`) has `dollars = 4`.

Not just an economy nit: $5 is the interest threshold, so a $0 start means ante
1 can never earn interest, and the report's own observation that a voucher was
never bought in 14 games follows directly from it. The tester's conclusion
("you can never earn interest in Ante 1") is right.

### 2. The Hook card leak — VALID, and it is a real corruption

`boss_hook_discard` (`core/src/game.rs:1185`) pushes to `self.discarded`
**without removing from `self.available`**:

```rust
let card = pool.remove(idx);
self.discarded.push(card);
```

The real game calls `discard_cards_from_highlighted`
(`blind.lua:481`), which removes the card from the hand. Ours removes it from a
*local copy* of the pool and pushes a duplicate into the discard pile, so the
card exists in both `available` and `discarded`. Two cards per hand, forever.

The tester's reproduction is exactly right, including the observable
consequence: duplicate playing cards in hand, and a deck that outgrows 52. This
breaks seeded reproducibility, which is the project's stated goal.

### 3. Celestial pack planets are destroyed — INVALID, our behaviour is correct

**Corrected on review.** I first wrote this up as valid, citing
`game-docs/Planet_Cards.md:38`. That was a misreading and the citation does
not support the claim.

The line says a planet "in the shop, or in your consumable slots, cannot also
be in that shop's pack" — and its stated use is *generation*:

> This can be used to drammatically increase the odds of finding the planet
> card in a pack you're looking for.

That is a rule about which planets a pack may contain, and we implement it:
`exclude_held` (`core/src/shop.rs:462`) drops every planet you are holding
from the draw pool.

What happens when you then *select* one is a different question, and the docs
answer it directly. `Booster_Packs.md:16` says Celestial packs contain Planet
Cards "**for immediate use**", and `:22` says "usually only one consumable
option can be used, and the rest disappear after the player has used that
card". The real game's pack definition carries `choose = 1`, so you pick one
and it fires.

That is what `pick_pack_card` (`core/src/game.rs:2437`) does: it levels the
hand and discards the card. Both halves of the rule are correct. The report's
premise — that a pack planet "can never enter your consumable slots" — is
true of the real game too.

The mistake worth recording: I treated a rule about *generation* as if it were
a rule about *consumption*, because both sentences mention consumable slots.
Checking the sentence's own example would have caught it.

### 4. Duplicate cards in packs — VALID for `Fast` mode only, but only half of it

`gen_contents` draws with replacement in every category. The real backend
does not: `next_spectral_pack` and friends call `self.lock(&item)` on each
draw, and `randchoice` resamples anything already locked, so a Real-mode pack
cannot repeat a card.

The CLI uses `RngMode::default()`, which is `Fast`, so the report's
observation stands. Worth stating precisely though: this is a Fast/Real
divergence, not a defect in the byte-accurate path.

### 7. Tags are invisible and unusable — VALID, with a correction

Valid: there is no `tags` line in the `Display` impl
(`core/src/game.rs:3016-3070`), so the state dump never shows a held tag.

Correction to the report: tags are not "granted but unusable" — they are
**applied automatically** at cashout (`core/src/game.rs:1963-1965`), before
the shop is stocked. There is no `UseTag` action, so there is nothing to
offer in a menu even if the dump showed them. The tester concluded skipping a
blind is a "pure loss", which is wrong: skipping a blind is worth a tag's
effect, applied silently. Orbital, Coupon, D6 and Speed all do work; the
report's "can never be triggered" is incorrect.

So this is two separate issues: invisibility (valid) and the absence of player
choice (a design decision, not a bug).

### 10. The shop hides what you cannot afford — VALID, and worse than reported

`gen_actions_buy_joker` passes `self.spendable_balance()` to
`gen_moves_buy_joker`, which filters on affordability
(`core/src/generator.rs:201`). Unaffordable stock is therefore absent from the
action list rather than listed-and-disabled. The tester's own check confirms
the filter works — at $3 only the two $3 consumables appeared.

Worse than reported, in fact: the state dump listed no shop stock *at
all* — only owned jokers, consumables and vouchers. So an unaffordable
item was unobservable everywhere, not just absent from the menu.

Fixed by adding the whole priced shelf to the dump rather than by
exposing unaffordable purchases as actions. `gen_actions` is contractually
legal-only and two tests hold that every action it offers executes;
`buy_joker` already rejects with `InvalidBalance`. Handing callers actions
that routinely fail is a bad contract for an RL environment, where a
policy picks from what it is offered. The real game shows the whole shelf
and greys out what you cannot buy.

---

## Invalid

### 5. Card modifiers are never displayed — INVALID

Both the hand line and the deck line go through the same `card_display`
(`core/src/game.rs:3011` and `3040`), and `card_display` appends
`[enhancement]`, `[edition]` and `[seal]`
(`core/src/card.rs`). A hand line reads:

```
hand: 5♠[Gold] 7♥[Foil] K♣[Red]
```

The report's own evidence undercuts it — the deck line it contrasts against
showed `6♥[Gold]`, which is the *same* function. The tester was reading a hand
that happened to contain no modified cards.

Pinned by `test_hand_display_shows_card_modifiers`, added here.

The underlying point about the *TUI* has merit though: the CLI dump shows no
debuff indicator, and there is no boss-name line (see below), so "I could not
tell a Wild from a Stone" is a real ergonomics complaint even though the
mechanism is there.

### 9. The phantom hand — INVALID, this is the real mechanic

`core/src/game.rs:1938` deals a hand when an Arcana or Spectral pack opens.
That matches the real game. `game-docs/Booster_Packs.md:22`:

> For Tarot and Spectral packs, a hand of playing cards is also dealt from the
> player's deck to apply the effects on.

And `Booster_Packs.md:59` documents the softlock edge case that only exists
*because* a hand is dealt — you can hit zero cards and have no legal target.
The tester's "in real Balatro your hand is empty in the shop" is true, and also
incomplete: it is empty in the shop, and a hand is dealt when the pack opens.

The deck swinging 44↔52 is the hand being dealt and returned.

---

## Presentation, not bugs

### 6, 8, 11, 12

These are all real limitations and worth fixing, but they are not engine
defects, and the report's own severity labels conflate the two.

- **6. Boss blinds unnamed.** True: `blind: {:?}` and `stage: Blind(Boss)`.
  No boss name, no effect description. The effects *are* implemented — the
  report confirms this by observing Manacle's hand reduction and the
  Needle/Wall multipliers. Fix is a `Display` change.
- **8. No play feedback.** True: `handle_action(action).expect(...)`
  (`cli/src/main.rs:90`) prints nothing. This is the most legitimate of the
  CLI complaints — the tester twice concluded scoring was broken before
  checking the score. Fix is a few lines against `calc_score_traced`, which
  already exists.
- **11. Not scriptable.** True: only `--load`. Worth noting this is now
  partly obsolete — the last three commits added `BALATRO_SOAK_GAMES` and
  the golden fixtures, and the deck/stake work made all 15 decks and 8 stakes
  reachable through `Config`. The CLI still cannot reach them.
- **12. Debug formatting.** `stage: Blind(Boss)`, `blind: Some(Small)` and
  `consumables: [planet] Uranus` are all true. `Some(Small)` is a genuine
  leak. `[planet]` is a deliberate category label so tarots and planets are
  distinguishable in a flat list, which is useful rather than accidental. The
  leading `|` the tester saw is their own parser, as they note.

---

## Recommended order, corrected

The report's order is good. Two changes:

1. **The Hook leak above the starting money.** The report ranks money first
   because it is a one-liner. It ranks second for a reason: it permanently
   corrupts deck contents and therefore reproducibility, which is the
   project's primary goal. A wrong economy number is a bug; a deck that
   grows past 52 invalidates every seeded-run claim.
2. **Treat 5, 6, 8, 9, 10, 12 as one workstream** — CLI and state-dump
   ergonomics. They are one class of problem (the interface does not show
   what the engine knows), they do not touch the engine, and the engine is
   correct in all of them. Notably, bugs 5 and 9 were misdiagnoses *because*
   of 6, 8 and 12: with no feedback, no boss name, and a state dump that
   looks like a plain rank-and-suit list, a correct engine looks broken.

## What the report got right that is worth keeping

The scoring verification — 100+ hands across all 13 ranks, with the formula
spelled out per hand — is the most valuable part, because it establishes that
the layer everything else sits on is correct. The ante table, the economy, the
planet-card effects and the joker formulas were each checked against the
docs and found right.

The honesty about the "not covered" section is also what made this review
possible: it is explicit that vouchers were never exercised *because* of bug 1,
and that The Hook's joker was confounded with bug 2. Both turned out to be
exactly right, and both are cases where a less careful report would have
claimed a passing result.

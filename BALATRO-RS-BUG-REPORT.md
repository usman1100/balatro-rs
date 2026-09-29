# balatro-rs CLI — Game Tester Bug Report

**Tester:** black-box testing via `target/debug/balatro-cli` (and `balatro-edit` for
controlled setups). No source code was read; all expectations come from `./game-docs`
and balatrowiki.org.

**Harness note:** `balatro-cli` requires a real TTY on **both** stdin and stdout.
With a pipe it emits nothing until the process exits, so all testing was driven
through a pty.

---

## Confirmed bugs

### 1. [Critical] Starting money is $0, should be $4

Every run begins with `money_start: 0`.

```
$ balatro-cli
money: 0
```

Real Balatro starts a standard run at **$4** (Red Deck). Consequences:

- You can never earn interest in Ante 1 (interest needs $5).
- You cannot afford anything until after the Small Blind, so the whole early
  economy is shifted one blind behind.
- Vouchers ($10) become effectively unreachable until Ante 2–3. In 14 automated
  games my player **never once** managed to buy a voucher, purely because of this.

---

### 2. [Critical] Card-count leak at The Hook — deck grows without bound

At the boss blind that discards 2 cards from hand after each played hand, the total
number of cards in the game **increases by 2 every hand**, and never comes back down.

```
leak hunt, 12 games:
gi=5 step=3  Blind(Boss) hand 8->8 deck 44->39 disc 0->7  TOTAL 52->54
gi=5 step=4  Blind(Boss) hand 8->8 deck 39->34 disc 7->14 TOTAL 54->56
gi=5 step=5  Blind(Boss) hand 8->8 deck 34->29 disc 14->21 TOTAL 56->58
```

The discard pile gains 2 cards that were never removed from the deck, while the hand
is refilled as if they weren't. Reproduced in 3 separate games.

**Observable consequence:** the deck outgrows 52 cards, and duplicate playing cards
appear in hand. I caught this in a live game:

```
hand: 7♠ 8♦ K♦ J♠ J♠ Q♥ 2♠ Q♥     <-- two J♠ and two Q♥
discard pile: 5, deck: 47          <-- 60 cards in the game
```

A 52-card Red Deck must never contain two J♠. Over a long run against this boss the
deck integrity is permanently broken, which also breaks seeded-run reproducibility
(the stated goal of the project).

---

### 3. [High] Planet cards taken from Celestial Booster Packs are destroyed

Buy a Celestial Pack, take a Planet Card, and it is silently lost — it never enters
your consumable slots.

```
BEFORE: money=4 cons=(none)
pack menu: [PickPackCard: Saturn, PickPackCard: Uranus, PickPackCard: Mercury, SkipPack]
picked 'PickPackCard: Saturn'
AFTER:  stage=Shop cons='(none)' menu=['Show game state', 'NextRound']
UseConsumable present? []
```

Reproduced 3 independent times, with Saturn / Mercury / Pluto. Note that the
**same planet cards bought directly from the shop work correctly** — I verified
Mercury, Neptune and Saturn levelling their hands (1P L1→L2, SF L1→L2, ST L1→L2).
So the bug is specific to the pack code path, not the consumable system.

Affects the whole Celestial pack line: Mega and Jumbo Celestial packs too.

---

### 4. [High] Duplicate cards generated inside booster packs

Packs draw their contents with replacement, so the same card can appear twice.
Real Balatro packs always contain distinct cards.

```
PACK ['J♦', '5♥', '5♥']                      <-- Standard Pack, two 5♥
PACK ['The Moon', 'The Moon', 'The Star']    <-- Spectral Pack, two The Moon
PACK ['Mercury', 'Earth', 'Pluto']           <-- distinct, for contrast
```

---

### 5. [High] Enhancements and editions on cards in hand are never displayed

The `hand:` line always renders a bare rank+suit. The same card rendered in the
deck summary shows its modifier:

```
hand: 5♠ 5♦ 7♠ K♠ 4♠ 5♠ 3♠ 9♠        <-- no modifier shown
deck: 52 (1 modified: 6♥[Gold])      <-- modifier shown here
```

Real Balatro labels every card in hand (Bonus Card, Lucky Card, Wild, Steel, Foil,
Holographic, Polychrome, and so on). This is not cosmetic:

- You cannot see whether a card is a Wild, a Stone, a Glass, or Gold.
- You cannot see whether an enemy boss blind has debuffed it (debuffed cards get a
  red cross in the real game).
- I could not verify a single enhancement's scoring effect, because a Lucky card
  (+20 chips) in hand is indistinguishable from a plain one. Several apparent joker
  mismatches during testing turned out to be exactly this.

---

### 6. [High] Boss blinds are unnamed and their effects are completely invisible

The blind-select menu only ever offers a generic label:

```
$ select blind
[0] Show game state
[1] SelectBlind: Boss Blind
```

and the state prints a raw enum:

```
stage: Blind(Boss)
blind: Some(Boss)
```

There is no boss name, no description, and no debuff indicator anywhere in the CLI.
You cannot tell you are facing The Plant (debuffs face cards), The Needle (1 hand,
1× base), The Wall (4× base), The Manacle (−1 hand size), The Arm (de-levels your
hand), and so on.

The effects themselves are largely implemented — I confirmed a boss reducing hand
size to 7 (The Manacle), debuffs correctly zeroing a card's chips while still letting
it form the hand, and the Needle/Wall score multipliers. But the player is blind to
all of it, so the game is not playable as a faithful simulation.

---

### 7. [High] Tags are granted but completely invisible and unusable

Skipping a Small or Big Blind offers a tag, but:

- the state display has **no `tags` field at all** (confirmed: the only keys the
  state ever prints are `ante, blind, consumables, deck, deck variant, discard pile,
  discards remaining, hand, hands remaining, jokers, money, planetarium, round,
  score, stage, stake, vouchers`);
- no action in any menu ever mentions a tag.

All 24 tags are unreachable through the CLI. This also means the Orbital, Coupon,
D6 and Speed tags can never be triggered, and skipping a blind is a pure loss.

---

### 8. [High] No feedback whatsoever when a hand is played

Playing a hand prints **nothing** — no hand name, no chips, no mult, no points
scored, no level change. The only way to find out what happened is to issue
`Show game state` afterwards and diff the score yourself.

```
$ (select A♠ A♥, then Play:)
$                       <-- no output at all
```

This is the single biggest obstacle to verifying the simulator. It cost me real time:
I twice concluded the scoring engine was broken before realising I simply had not
looked at the score.

---

### 9. [Medium] A phantom hand is materialised whenever a pack is opened in the shop

Buying any pack in the shop (where your hand is empty) suddenly deals 8 cards out of
the deck and offers them as legal targets for card-based tarots and spectrals:

```
[shop before buy]      stage=Shop      hand=''              deck=52
[after buy, pack open] stage=PackOpen  hand='Q♣ 5♦ 2♣ J♠ 4♥ K♦ 9♥ 7♠'  deck=44
[after ApplyTarot]     stage=Shop      hand=''              deck=52 (1 modified: Q♣[Gold])
```

The Devil did apply correctly to Q♣, so the effect itself works — but the targeting UI
is operating on 8 cards that are not really in your hand, and the deck count swings
44↔52 around a pack purchase. In real Balatro your hand is empty in the shop, so
card-targeting tarot/spectral effects simply have nothing to act on.

---

### 10. [Medium] The shop never shows prices, and hides anything you can't afford

Items are listed by name only:

```
[2] BuyJoker: Jolly Joker [Common] +8 Mult if played hand contains a Pair
[3] BuyConsumable: The High Priestess
[4] BuyPack: Standard Pack
[5] Reroll
```

No cost, no sell value, and no "can't afford" marker. Unaffordable items are dropped
from the menu entirely, so you cannot see the rest of the stock or plan around it.
Verified that the filter itself works: at $3 the menu showed only the two $3
consumables, and at $4 a $4 Celestial Pack appeared while the $10 voucher and $5
reroll did not.

---

### 11. [Medium] `balatro-cli` cannot be scripted

The only options are `--load <FILE>` and `--help`. There is no `--deck`, `--stake`,
`--seed`, or non-interactive mode. Combined with the TTY requirement, this makes
automated testing and headless RL-environment use impractical.

Related: the run is always Red Deck / White Stake. There is no way to reach the other
14 decks or 7 stakes through the CLI at all.

---

### 12. [Low] Raw Rust debug formatting leaks into the player-facing UI

```
deck variant: Red Deck
stake: White Stake
stage: Blind(Boss)
blind: Some(Small)
consumables: [planet] Uranus
jokers: | Scholar
```

`Some(Small)` is a leaked `Option` debug print, `[planet]` is a leaked enum, and the
leading `|` on multi-item joker lists is an artefact of my parser joining the
continuation line — the underlying format is an unlabelled list.

---

## Verified correct

These I checked and found right, so they're not in the list above.

**Scoring engine** — the most important result. I verified the documented formula
across **100+ played hands** with no jokers, consumables or vouchers, covering all
13 poker hands, level scaling via planet cards, "only the hand's relevant cards
score", face cards = 10 chips, Ace = 11. Zero mismatches. Examples:

| Hand | Cards | Expected | Got |
|---|---|---|---|
| Pair L1 | 7♣ 7♥ | (10+7+7)×2 = 48 | 48 |
| Two Pair L1 | 9♠ 3♥ 3♦ 9♦ + 5♦ | (20+32)×2 = 104 | 104 |
| High Card L1 | K♥ | (5+10)×1 = 15 | 15 |
| Full House L1 | 9♣ 9♥ 9♦ A♥ A♦ | (40+10+10+10+11+11)×4 = 368 | 368 |

Boss debuffs are also handled per spec — a debuffed card still forms the hand but
contributes no chips, which is exactly what the docs specify.

**Ante score table** — verified all 8 antes exactly against the docs, for all three
blind types, using `balatro-edit set-ante`:

```
ante | small(1x) | big(1.5x) | boss(2x)  |  docs expect
  1      300 OK                        |  300 450 600
  2      800 OK                        |  800 1200 1600
  3     2000 OK                        |  2000 3000 4000
  4     5000 OK                        |  5000 7500 10000
  5    11000 OK                        |  11000 16500 22000
  6    20000 OK                        |  20000 30000 40000
  7    35000 OK                        |  35000 52500 70000
  8    50000 OK                        |  50000 75000 100000
```

**Economy** — blind rewards and interest are correct in every round I checked:
small $3 / big $4 / boss $5, +$1 per unused hand, +$1 interest per $5 held capped at
$5. E.g. a boss beaten with 0 hands left while holding $48 cashed out for exactly
$10. Red Deck's +1 discard over a base of 3 giving 4 is also correct.

**Planet cards bought from the shop** — Mercury, Neptune and Saturn all incremented
the right hand level, and the new level then scored correctly.

**Jokers** — I numerically verified Scholar (+20 chips/+4 mult per Ace), Wrathful
Joker (+3 mult per scored spade), Even Steven (+4 mult per even), Cavendish
(unconditional ×3 mult, matching its documented effect), Pareidolia, Zany Joker,
Droll Joker, Sly Joker and Crazy Joker all match their documented formulas exactly.

**Packs** — Standard, Buffoon, Celestial, Arcana, Spectral and their Jumbo/Mega
variants all open, contents are displayed, the cost is deducted correctly
($4 Normal pack: money 7→3), and card effects work: Justice applied a Glass card,
The Devil applied Gold, The Empress applied a Mult enhancement, The Hermit doubled
money ($2→$4), Temperance paid out, and card-targeting tarots work via `ApplyTarot`.

**Sorting** — matches the documented order exactly: by suit (Spade, Heart, Club,
Diamond, descending rank within suit) and by rank (A, KQJ, 10–2 with suit as
tiebreaker).

**Round mechanics** — 4 hands, hand size 8, refill to hand size after play and
discard, discards decrement correctly, game-over is detected and reported.

**Shop transactions** — prices are deducted correctly (bought a $4 joker, money
6→2) and mid-round use/sell of consumables and jokers is offered.

---

## Not covered

I ran out of practical time on these; they are open questions, not known-good:

- Most of the 150 jokers (I verified ~9 numerically). In particular **The Hook's own
  joker effect is confounded with the boss-blind card leak in bug 2** — I could not
  separate them.
- The other ~25 boss blind effects, individually.
- Voucher effects — I never got to buy one (see bug 1).
- Tarot and spectral card effects beyond the 6 I exercised.
- Decks other than Red, and stakes other than White (unreachable via the CLI).
- The Ante 8 win condition, and whether the run can actually be completed.
- Erratic Deck, Glass Card shatter accounting, stickers, editions on jokers.

## Suggested order

1. **Starting money $4** — one-line, restores the entire early economy.
2. **The Hook card leak** — corrupts the deck and therefore breaks the project's core
   reproducibility goal.
3. **Celestial pack planet loss** — silently deletes items.
4. **Play feedback + boss blind naming + card modifiers in hand** — without these the
   CLI is not playable and not testable, regardless of engine correctness.
5. **Tags exposure** — a whole content category is currently dead.
6. Duplicate pack contents, phantom shop hand, shop prices, `--deck`/`--stake`/`--seed`
   flags, and the debug-formatting leak.

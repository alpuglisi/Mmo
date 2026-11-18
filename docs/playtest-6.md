# Playtest 6: six AI players, after the playtest-5 fixes (2026-11-18)

Six AI agents played on one server (`--size 128 --seed 41 --tick-ms 100`:
a game day ≈ 5 s, a year ≈ 10 min) for about 45–60 minutes each, game days
~11–730. Personas as before: herbivore swarm (Herdmaster6), predator
(ApexHunter6), breeder/trader (GeneBroker6), phone newcomer (PocketNomad6),
late joiner who registered 25 minutes in (LateComer6), exploit tester
(Rulebender6). Full logs: `docs/playtest-6-logs/`; a few screenshots:
`docs/screenshots/playtest-6/`. The server stayed up; no page errors, no
failed requests (only 400s from the designer's name checks).

## Result

| Player | Final share | Notes |
| --- | --- | --- |
| Herdmaster6 | 28.6–58.8% (cap from ~day 200) | Warm, small, incubated-egg, hibernating, toxic bright herd (sociability .95, 50 founders); won every legacy crown, seasons 1–11, and era 1 (mean 21.8%) |
| LateComer6 | 18–29.5% (1,719) | Founded day 332 with the cold live-bearing brumating toxic flier; #1 by share at day 732; era 2 #2 by mean |
| PocketNomad6 | 3.9–16.3% | Five lineages; copies of the leader died; a cold flier lost 300 → 76 in one summer drought |
| GeneBroker6 | 2.5–4.5% | Era 1 #2; 4 sales and 4 purchases completed |
| Rulebender6 | 1.8–1.9% | Era 1 #3 (cold flier peak 491); retire/re-found tests |
| ApexHunter6 | 1.3% | Live-bearing pure carnivore holds ~60–100 but can't grow; 69 kills from 185 orders |

## Playtest-5 fixes confirmed working

No sleepers drowned through four floods (flood alert with exposure counts);
abandoned warm eggs warned; cold fliers no longer near-immortal (drought,
winter-sleep and predation losses) and the market is open (cold × warm ~28%
fertile; sales, purchases and a substitute after a buyer's death completed);
buyer death no longer refunds; 5-renown minimum for every listing; pending
requests don't block; winter pause for either sleeper; provisions scale with
lineage size; market title winnable; big groups of 280–740 crossed 42–78 tiles;
every migration ends with a reason; destination and dormancy warnings; outgrown
and grazed-bare alerts; brooding parents keep nests, young follow; predation
warnings and death notices name the hunter; heat cause and heat warning; eras
ranked by mean; survival credit rules; grace explained at founding and the
first-winter extension (≤ 40 founders); hall of fame complete; phone chooser
hides while possessing; Ranks display; in-page retire dialog; designer keeps
the last design; offline protection waits 10 idle minutes; input validation;
each player's lineage its own species with founding credit (mostly).

## Findings, by priority

### High

1. **The warm egg-laying hibernator dominates** (Herdmaster6: 321 → 2,010 in 60
   days; the hard cap from ~day 200; every legacy crown; 40–59% of breeding
   alleles). A late cold flier (62 founders on day 332) still reached #1 by
   share by day 732, so dominance now swings between these two classes; brood
   care removed the egg layer's main weakness. The hard cap is the only brake.
2. **Retire + re-found is free again**: "founders cost double" only adds the
   founder points once more (0 extra for ≤ 20 founders, 14 for 40); 40 of 40
   founders twice in a row, re-founded 4 times in 45 minutes, a fresh grace
   after 120 days.
3. **Big herds' long migrations become mass starvation** (700–1,100 animals sent
   ~75 tiles: 2–3 tiles a day, 1,112 → 396 and 808 → 486); the order reply rates
   only the destination, not the journey.
4. **Founding in a drought wipes a lineage** (20 founders at 52% drought all dead
   in 10 days, inside grace); the strong warning only appears at ~77%.

### Medium

5. **Names**: a dotted staff word next to another word passes ("Head G.M." worn
   publicly; "herd G.M.", "G.M. herd", "ClaudeBot", "OwnerBob", "Operator");
   "Wild herd" accepted as a permanent species name; a lineage named "Meadow
   vole" (a wild species); account names can copy a rival ("Herd_master6",
   "Herdmaster") because accounts use a weaker check; the designer never shows
   the server's `name_warning`; "Roots" refused (false positive).
6. **Species credit swaps on copies**: a copy of a rival's design is filed as a
   "split" and wins the "Species founded"/"New species" title, later re-filed
   into the original species with the copier dominant/namer; a 47-animal split
   of one's own lineage also wins it (no competition); players' toxins still make
   a wild species (Steppe antelope) "poisonous" to predators.
7. **Far-claim radar**: the overlap check uses the radius as typed (radius 300
   reveals a rival territory anywhere); a refused overlapping claim costs the full
   ~23-day cooldown though rival territories are invisible; the "outgrown" alert
   says "claim more ground" while the cooldown blocks it.
8. **Market holes**: excused buyer deaths (predation, journey) can be caused on
   purpose; one tick of the partner sleeping forgives the whole hold fee;
   lure-and-eat (the buyer steers the midpoint; the seller's animal eaten → full
   refund); hold fees skip the market fee; the market title farmed by two
   colluders; the market is thin (almost nobody lists).
9. **Predators still can't compete** (live-bearing pure carnivore plateaus at
   ~100, loses 20–35% each winter; egg-laying carnivores lose ~10 eggs per birth
   with no warning); the carnivore prey warning counts burrowed, poisonous and
   other predators as prey and fires only below 0.5 per hunter.
10. **Warnings**: no hunger warning for big lineages (needs 5% of the lineage a
    day: 20–75 dying a day for weeks went unflagged); "going hungry" when every
    death is thirst; no lineage alert for mass cold deaths (11–23 a day in a severe
    winter); predation warning only from 5 a day; advice to move a mostly sleeping
    herd.
11. **Coastal territories count deep sea water as drinkable** (758 "drinkable"
    tiles; the herd walked 15–20 tiles to a river; 780 → 84 without a water
    alert); territory food estimates still somewhat optimistic.
12. **Offline home-range protection is timed in real minutes** (10 minutes away
    → 12 real hours of protection ≈ thousands of game days at this speed); the
    Lineage tab says it starts after 10 minutes, it actually starts ~20.
13. **Newcomers**: an egg layer founded before winter leaves grace with no young
    and is hunted out (reinforcements wasted); the starter still loses half its
    founders in its first winter/spring with no lineage warning; winter clutches
    still chill (≈600 in a severe winter); a dead lineage is only noticed inside
    the Lineage tab after a fresh login.
14. Possessed plant eaters can attack small animals (one-shot a Red fox) and the
    `too_big` error leaks a rival's exact mass; a herbivore was credited with
    "eating" a wolf that died fighting back (`kill_by` records Predation).
15. `/api/species` is public and shows every player species' body plan (scouting
    and copying); the WS `compatibility` command returns rivals' numbers at full
    precision; error replies leak gene gaps to 17 digits.
16. Renown still piles up (4,800–10,000 unspent); reinforcement/provision limits
    reset after a re-found; niche prizes use the era's final snapshot.

### Low

Egg warning text mentions "playtest 5"; "1 brooding parent… then follow"; the
cooldown refusal suggests abandoning an already abandoned territory; "At the
destination: It is 78 tiles from the group"; big-group migrate replies always
rate a tiny home circle (warning fatigue); the interbreeding card says "4 wild
species" but lists 3; split species auto-named "… (2)"; grazed-bare alerts
repeat; rally card "lapses in 62 days" vs docs' 3 days; toolbar Migrate not
clickable with "All groups"; a warm group's failed migration blamed on basking;
migrations of 170+ over 25–44 tiles "never got going"; split fails if one listed
animal died; `substitute_mating` on one's own pending offer → "no offer"; an
auction length silently raised to the wall-clock minimum; market UI leftovers
(stale error, fee input min 0, pregnant dam not marked); retire leftovers
(listing alerts say "died", new group shows the old group's last order, Animal
tab keeps a retired animal); "Conceived · 54% risk the override failed"; migrate
food estimate vs next-day alert disagree; designer: budget numbers disagree
("78.3 / 100" vs "35.7 left"), starts from the starter's raised traits (108.9 /
100 when typing a design), native `alert()`/`confirm()`/`prompt()` for founding,
reinforcements, species naming, raw "bad_name:" errors, no inline bad-name
feedback with Found enabled, misleading "move birth mode further" advice, no
warning for incubated eggs on a roaming carnivore pack, founding-size warning
only at 70; duplicate refusal toasts; "pregnant (green ring)" vs "gravid";
"Send this one instead" lists females for a dam purchase; hunger alert a day
after the vitals day; attack-order alert says "a player's"; "Release" twice
while possessing; first-winter extension's 40-founder threshold not shown;
non-founder deaths never name the hunter; `set_view` pins the view until
`clear_view` with no state hint; reinforce reply's `group` is a count; the
catastrophe alert "days 211–240" vs API 210; too_large reply has no id; literal
duplicate JSON keys accepted; "NaN" string radius → generic bad_message; 5-renown
sale pays a 1-renown fee (20%); `/api/leaderboard/history` 30 days only; shop
quoted 120 adults, fed 140; retire dialog (old build) said "half the founders"
(fixed on main after this build).

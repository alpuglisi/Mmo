# Playtest 11: six AI players, after the playtest-10 fixes (2026-11-19)

Six AI agents played on one server (`--size 128 --seed 61 --tick-ms 100`:
a game day ≈ 5 s, a year ≈ 10 min) for about 50–65 minutes each, game days
~4–760, on main at c7efb51 (all of round 11: market shield only for accepted
agreements, attack refused as `protected`, lineage-offer pick limit and
midpoint meeting, walking buyers keep their fee, change-only mating updates,
restore pricing by animals in the circle with no census and a held quote,
grace preview treating a living lineage as ending now, credited-only
survivors, merge/retire group alerts, nest raids, share brake on litters only,
refuge ramp, reinforcements clear of rival territory, founding together,
claim/rally way warnings, alert cool-downs and the sleeping fold, banners
under the toolbar, Release reachable, landing toast, income taper, server and
client Lows, a full trade integration test, hardened e2e). Personas as before.
Full logs: `docs/playtest-11-logs/`; screenshots: `docs/screenshots/playtest-11/`.
The server stayed up; no page errors or failed requests.

## Result

| Player | Final share | Notes |
| --- | --- | --- |
| PocketNomad11 | 23.2% (819; peak 1,903) | Starter stalled (~40); a re-found of the warm egg burrowing design with 10 reinforcements; accepted 7 trades (5 completed) |
| Herdmaster11 | 11.5% at the end (peak 30.8%, the hard cap three times) | **Cold live-bearing toxic flier won both eras** (13.0% and 20.5% means) and seasons 3–10; crashed to 344 to ApexHunter11's pack after the wild collapsed in a drought |
| LateComer11 | 20.9% at era 2's end, #1 among players days 721–749 | A copy of the flier founded on day 362; era 2 #4 by mean (6.4%); retired at 1,192 to test grace |
| Rulebender11 | 9.0–16% | Warm live toxic hibernator, peak 1,087; era 1 #2; 2 trades |
| ApexHunter11 | 3.6% (peak 250) | Pack back to playtest-9 size; fed on PocketNomad11's egg nests (group 43 → 102) without stopping it (651 → 2,017) |
| GeneBroker11 | 0.3% | **7 cross-player trades completed** (6 buys, 1 sale; 3 conceived) — the first completed trades since playtest 8 |

## Playtest-10 fixes confirmed working

Trades complete (7 for one player, 5 accepted by another, 2 for a third); only
accepted-agreement walkers are protected (pending offers and listings eaten,
Market text says so, attack on a walker refused `protected`); mating update
positions coarse; restore preview without a census, quote held, value hint,
nothing in winter; grace preview correct while a lineage lives and after
retiring, continued grace never extended; Survivor feed lists only credited
survivors; merge/retire no false "died out"; share brake shrinks litters only
(no extra hunger); a non-egg design (the cold flier) won both eras; nest raids
let a pack feed on egg herds; pack back to 250; reinforcements land clear of
rival territory and say so; far claim/rally warn of hunters and rival
territory; drought banner clear of tools (desktop and Pixel 7); Release
reachable on phones; toasts above the possession bar; landing toast; designer
opens after retiring; crowding header shows the binding cap; listings show age
vs lifespan; one-letter deletions/substitutions of player names refused; income
taper correct; niche prizes ≥ 25%.

## Findings, by priority

### High

1. **Nest raids ignore grace, offline protection and the market shield**
   (`scent_clutch` in decide.rs, `raid_clutch` in apply.rs skip only own clutches
   and given-up species; no `grace_spares`): a graced newcomer's eggs were raided
   by a graced pack (8/8/21 eggs on days 45–55).
2. **Share runs away by attrition**: when droughts and packs shrink the wild
   (3,472 → 1,015; antelope ~755 → 108 in year 5 with no catastrophe), a leader's
   share rises even while its own herd shrinks (25% → 31% with litters ×0.10);
   the per-lineage brake can't stop it; two leaders held ~55%.

### Medium

3. **An accepted agreement with a sleeping animal is an open-ended shield**: the
   deadline waits for the whole sleep (seller: no cap; buyer: 30 days) while both
   animals stay protected (53 days for one free request); and every half-day
   deadline move sends a mating update (58 for one offer, ~2.4 s apart) — toast
   spam; the `complete_mating` too-far path has no de-duplication at all.
4. **The sleeping fold's 3-day cool-down never holds** (`warned.retain` in
   `warning_alerts` drops the `"dormant"` key daily): the line came 15 of 18 days,
   three copies within 10 s; the fold drops the hunter name and counts; "being
   hunted"/"eggs failing" still alternate every 5–15 s for < 1% daily losses;
   territory "no longer feeds"/"recovered" still swing every 4–10 days.
5. **Foundings on a crowded map land inside a rival herd** ("0 tiles away" with 148
   rival animals in view; "2 tiles", "5 tiles"): the 8-tile rule is only hard for
   150 of 200 picks; the avoid-last-home rule kept a re-found off the ground the
   player's own retired herd had just emptied.
6. **Carnivore's own grace starves it**: grace is two-sided, so a graced pack can't
   hunt rival animals whose grace has ended (24 → 16 beside ~170 unprotected
   animals) and no alert mentions grace or "End my grace now".
7. **Near-copy names**: adjacent letter swaps pass ("Herdmatser" bought and worn,
   "Rulebedner", "LateCmoer"); blends ("Herd Nomad", "Pocket Master"); species names
   near-copy rival lineages ("Skyglass Swarn", "Skyglass Swarq" beside "Skyglass
   Swarm": `name_species` checks only `rival_name`); "Thornbak"; name checks before
   funds give a free oracle.
8. **Leaks**: restore price vs the shop's base price (and price − own count above
   200) is an exact count of rival animals within 12 tiles (on the playtest build;
   main already has review's sight filter on the count — verify); way warnings give
   exact rival hunter counts and rally+cancel has no cooldown (free radar);
   `players_online` in `/api/assign` (anonymous) and `/api/worlds` (logged in);
   listed animals visible map-wide, and a pending request's `apart` narrows the
   32-tile region; anonymous `/api/metrics` apply/save timings.
9. **Income taper dodged with escrow** (park renown in a priced offer, cancel later).
10. **Market walks**: 4 of 6 lineage offers met at the seller's animal (any non-buyer
    carnivore near a candidate point, even a wild fox, slides it all the way);
    incoming lineage offers show no distance (no `apart` without a named partner);
    the buyer's animal walks up to 48 tiles with no thirst/route warning and no
    protection after mating (one died of thirst, one pregnant female eaten); the
    lineage-offer picker lists animals the server refuses; no player ever listed a
    stud or dam (supply side empty).
11. **Re-found halving cancelled for free**: request 40 founders, get 20 for the
    points of 20 — the penalty only bites above 40.
12. **Heat**: a furred design died of heat in its first summer in forest by water
    (the advice it was given); no designer heat warning for the landing spot.
13. **A split triggers a false "Group B has fallen from 751 to 311 adults"** (peak not
    reset on split).
14. **restore_pasture holds the price, not the food** (paid for 2,015, got 875 in a
    rising drought); one quote per player (preview A, preview B, buy A → charged B's).
15. **Rally to check warnings costs the territory** (rally then cancel_rally drops
    the claim); migrate warns of no hunters at all.
16. **Toxin deaths never trigger a decline alert** (−28% to toxin, one "going hungry").
17. **Pack scatter** (whole map without a rally); own-pack cannibalism unexplained;
    reinforcements can land in the middle of a rival herd that holds no territory.

### Low

Landing toast/line shown 2–3 times; World tab "extinct day 131" for a retirement on
134; "Going hungry" names "the soft cap" when the world-share cap binds; crowding
explainer says "20% of the world's animals"; grace preview calls the living lineage
"your earlier lineage"; meeting text "E of yours" for a zero-length direction
(atan2(0,0)); Lineage tab tap opens a neighbouring animal after a re-sort; "dying off
slowly … 25%" right after "crashing 42%"; float noise (rally radius, reinforce `at`,
`abandon_territory` x, `/api/species` peak_adults, preview distance); restore price
button vs dialog; winter restore preview quotes a price for 0 food with no reason;
stale shop restore note ("largest lineage grazing there"); "(it had 1 adults)";
"within about 1 minutes"; substitute alert mentions a hold fee for an unpriced
request; "yours has 423"; "detected on day 26Name it…"; "Sale: 11 … less a 1 renown
fee" (net shown as gross); "a mating slot is forfeit" without saying whose;
`unknown_offer` after close; `fertility 0.1` vs `fertile_share 0.8` (UI 20%);
`fertile_with` counts own lineage; auction `days: 20` silently ~62; taper ledger
wording and "1000"; achievements feed "your lineage" for every player; designer "It is
winter now" in spring; feed's mating line without a day; re-found box 79.4 vs dock
95.5; "· 0 alive" summary after retiring; stale login day count; leftover died-out
banner text in the DOM; share halves during a boom (young don't count) unexplained;
unpaired "has recovered"; era-1 tie 0.7% vs 0.7% with different ranks; early sex
ratio skew; Log out clipped on a 390 px phone; picker below the fold; View group toast
over the tip card; 3–4 line toasts cover a third of the phone map; big sprites hide
the Food layer; starter design a dead end; renown still piles up (1,567–2,900);
retired lineage in `/api/species`; nested re-found rule in the feed; compatibility
reply has no cost; migrate has no cooldown; `next_refound_fee` 100 before founding.

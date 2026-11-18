# Playtest 3: six AI players, after the playtest-2 fixes (2026-11-18)

Six AI agents played on one server (`--size 128 --seed 23 --tick-ms 100`:
a game day ≈ 5 s, a year ≈ 10 min) for about 55–65 minutes each, game days
~15–780. Personas: herbivore swarm (Herdmaster3), predator hunting players
(ApexHunter3), breeder/trader (GeneBroker3), phone newcomer on an emulated
iPhone 13 (PocketNomad3), late joiner who registered 25 minutes in
(LateComer3), and an exploit tester (Rulebender3). Full logs:
`docs/playtest-3-logs/`; a few screenshots: `docs/screenshots/playtest-3/`.

The container restarted (not gracefully) at about day 566; the server was
restarted from its save. No game data was lost: lineages, territories,
cooldowns, grace, renown, offers, agreements, escrow, shop counters and season
history all came back. Everyone had to log in again. No console errors or
failed requests were seen apart from the expected 401 for stale tokens.

## Result

| Player | Final share | Notes |
| --- | --- | --- |
| PocketNomad3 | 22–31% (2,100 alive, the hard cap) | Third lineage: cold-blooded, brumating, 4 kg toxic flier with honest bright colours and 4 eyes; flat through winters; won seasons 10–11 |
| Herdmaster3 | 8.5–11% (peak 1,918 alive, 20.6%) | Hibernating, burrowing egg-layer with 4 territories; won era 1 and 6 season crowns; food, water and winters capped it, not the soft cap |
| Rulebender3 | 2.9–4.4% | First lineage killed by one territory claim (84% in 25 days); re-founded to 1,500 |
| LateComer3 | 1.7–3.8% | Joined day 321 with the playtest-2 winner's design + 5 eyes; crippled by a winter founding on the north snowfield |
| GeneBroker3 | 0.8–1.3% | Five lineages; 0 of 21 cross-player matings completed |
| ApexHunter3 | extinct | Eight predator species, seven collapsed in 30–130 days; never out of grace |

The playtest-2 dominant strategy (egg-laying + toxic-but-drab + warm
hibernating burrower) no longer dominates: the dormancy toll costs ~25–30% a
winter. A new candidate replaced it (finding 8).

## Playtest-2 fixes confirmed working

Soft-cap text and numbers (builder, Lineage tab); drought alerts and banner,
also on phones; territory and rally water checks (`water_share`,
`drinkable_tiles`, `mostly_water`); `migrate`/group commands on missing groups
(`no_such_group`); migration timeout alert; offers survive season changes and
restarts, with real-time lifetimes and expiry warnings; grace visible (days and
real time, inspector pill, Help topic) and two-sided; reinforcements capped at
2 per season counting retired lineages, price doubling; `/api/leaderboard/
history?genes=true` needs login and rivals' genes are coarse; spectate hides
graced lineages; `preview_mating` refuses out-of-sight partners; interbreeding
card honest (sampled pairs, drift risk) and no longer calls player species
"wild"; body size costs points so prey is huntable; attack results, single
refusals, attack beats patrol, attack-alert limit; possessed animals drink and
get "parched" alerts; block / auto-decline UI; searchable adult picker with
badges; hint cards no longer cover the possession bar; minimum fee and 5-minute
auctions; `mating_prefs` rejects bad players; ledger summary includes
royalties and shop spending; malformed messages keep their id; COMMAND_NAMES
complete; founding ramp explained (after founding); catastrophe banners with
countdown; "N fewer organisms" fixed; no stale snow or chunk seams seen.

Partly fixed: migrations (median arrival works for compact herds, not big
ones); hibernators on lake ice (2–4 drown per melt); set_view (reach ~24 tiles,
still lists hundreds of rival animals); preview_mating (still reveals existence
and sex); silent wild dilution (Genes tab shows it, no alert).

## Findings, by priority

### High

1. **Grace restarts on re-founding** (reported by 5 of 6). `grace_cooldown` in
   `crates/sim/src/world/commands.rs` chains foundings only within
   2 × `attack_grace` game days (~40 days ≈ 3 real minutes here) and ignores
   the 30-minute real-time floor, so any later re-founding gives a fresh
   30-minute immunity. Help text says it doesn't start over.
2. **Predators never get to play.** Each founding gives 30 real minutes of
   two-way grace; predator species collapse in 5–10 minutes; re-founding
   restarts the clock. Combined with 5 of 6 lineages graced at any time,
   attack orders are nearly unusable. Predator energetics (a kill feeding the
   hunter, sustainable hunting) are not in this build.
3. **Animals die of thirst beside water.** (a) Hunger outranks thirst in action
   selection, so starving herbivores keep grazing/scavenging at thirst 90–100
   (15 thirst deaths in a day in a territory with 399 drinkable tiles).
   (b) Steep banks (slope > 0.6) block animals with climbing < 0.4 from
   drinking; the founding search counts such a shore as "near water".
4. **Fatal founding placement.** Founders placed on a dry corner after a
   drought (64% dead in 13 days), on a desert tile (8/8 dead), three times on
   the same unreachable lake peninsula (~95% dead), and on the north snowfield
   in winter (55 → 25), which cripples late joiners founding in winter.
5. **Five eyes buys full reproductive isolation for 3 points.** Each eye step
   adds 0.1 to the body-plan gap (`crates/sim/src/morph.rs`), so 2 → 5 eyes is
   sterile with every 2-eyed animal, and gives 360° vision.
6. **Cross-player matings never complete** (0 of 21). An accepted agreement
   only checks distance; nothing brings the pair together and neither side is
   told where the partner is. Promised females keep mating naturally and fail
   as "already pregnant". The market title is unwinnable as a result.
7. **Big groups' migrations "arrive" at once.** The arrival radius is the rally
   radius (up to 24 tiles) and the settle circle 1.5×; a 486-strong group
   "arrived" with its median 24–29 tiles out; long migrations of big groups
   fizzle. Herds walk home when the settle ends, with no alert.
8. **Balance: cold-blooded brumating toxic flier dominates** (2,100 alive, flat
   through winters). The dormancy toll applies only to warm hibernators;
   brumation costs 6 points/unit vs hibernation's 12; it is sterile with
   everyone (eyes, egg-laying) and survives catastrophes.
9. **Free dam-lock.** Buying a listed female holds her from breeding until the
   30-minute deadline; cancelling before then refunds in full.
10. **Territory claims can wipe out a lineage.** The claim checks water only at
    claim time: no food/capacity check, no re-check as droughts deepen or the
    herd grazes it bare, no alert (84% of a lineage in 25 days; 88–159 thirst
    deaths a day elsewhere). A 7-tile-water claim was accepted for 250 animals;
    a lake claim is accepted for swimmers (`standable_tile` counts swimmable
    tiles); a claim centred in a lake 85 tiles from the herd was accepted.

### Medium

11. **A kill doesn't save a starving hunter** (killed a fox, starved 4 days
    later).
12. **Possession outlives the session**: a possessed animal stays possessed
    after the browser closes and starves with the wrong death alert.
13. **Retire + re-found is a free founder re-roll** (fresh, fed founders at a
    new spot, plus fresh grace).
14. **Reinforcement cap resets at the season boundary** (+40 adults in ~2
    minutes); price resets every ranked season.
15. **Paid cosmetics don't work**: nameplates are never shown in the client;
    no UI for `name_species`; the founding species can't be named
    (`not_yours`). **Titles/species names are unfiltered**: RTL override,
    zero-width characters, markup and impersonation ("<b>Server Admin</b>").
16. **Leaks**: a failed pending offer reveals "already pregnant";
    `request_mating` has no sight check (existence, owner, maturity);
    `preview_mating` checks `not_found`/`same_sex` before sight;
    `/api/species` and `/api/design/compatibility` give exact body plans;
    `/api/leaderboard/history?limit=huge` is unbounded without login.
17. **Restart resets `request_log` and `attack_alerts`**; sessions are dropped
    on a non-graceful restart; matings history and vitals history restart.
18. **Real-time durations at fast ticks**: grace ≈ 370 game days, purchase
    deadline longer than a lifetime, request window ≈ 750 days. (Partly by
    design; the grace length should be capped in game time too.)
19. **No lineage-level warnings**: hunger/thirst crash (86 starving a day with
    "litters full size"), near-hard-cap status (only flagged at ≥ cap), dry or
    grazed-out territory, obedient groups starving inside a territory.
20. **Builder gaps**: no upkeep shown for muscle, size and fangs; drought
    warning only at ≥ 60 founders and none for a 94% drought; no catastrophe
    warning; wings on heavy bodies do nothing silently; Hibernation text says
    "very cheap" and omits the toll and the 6-day breeding lag.
21. **Dormancy**: toll ~1%/day (2–3× the documented rate); dormant animals die
    of thirst and can't follow migrations (migrate accepted with 104/120
    estivating); hibernators wake together on crowded pastures and starve;
    winter founders' eggs vanish without notice.
22. **Starting design interbreeds with the Steppe antelope** (100% fertile) and
    is absorbed into it; an egg-laying player lineage was filed under the wild
    species (no founding credit or naming).
23. **Help "Toxins and colours" recommends toxic-but-drab** as a strategy.
24. **The UI can't choose your animal when accepting a lineage offer** (server
    picks the nearest: 110 tiles away). Market compatibility list wording wrong
    ("best pair", "(none: no compatibility)", misleading sterile reason).
25. **Retire transfer abuse** (from code): animals with ≥ 25% of a rival's
    alleles move to the rival even when 75% are the retiree's, then earn the
    retiree royalties.
26. **Phone UI**: expanded notices cover the open Tools menu (Migrate
    untappable); the "Here:" herd chooser covers the map for 8 s and steals
    long-presses; Follow clamps the camera at the map edge so the herd sits
    under the toolbar.
27. **Migrate accepts a target in the middle of a lake** (route across water).
28. **API players without a socket count as offline** ("While you were
    away…" while giving orders).
29. **Burrowed voles** are unattackable but not shown as burrowed; summer
    drownings during hunts.

### Low

Grace end not announced; `own_grace` in game days only; grace help says
instinct hunts still happen (they don't); claim cooldown in ticks; split group
inherits the claim cooldown; Split enabled at the 4-group limit; "Eggs: no
clutch yet" after hatching; "Largest lineage" is a peak, unlabelled; "Only
100% of your genes are yours" at 99.7%; megadrought sends two alerts;
"Survivor" after a 70% loss; claim radius 0.5 silently raised to 3; rally
tooltip says "a few days" (≈ 62); hibernation toll reported as "exposure";
"cold or heat" doesn't say which; budget labels the large-body cost "cold
blood / small body"; partner shown as "#0"; `traded_share` −0.0;
"partner_unavailable" without reason; accepting reports `too_far` as an error;
failed offer shows its old `expires_tick`; retire dialog/toast counts differ;
"Your last lineage died out" after a retirement; "1 animals"; retire-transfer
alert says "mostly of your genes"; reinforcement clones arrive old; no
confirmation for reinforcements and disabled reason only in a tooltip; titles
counted differently in record, hall of fame and seasons; ties in largest
lineage; preview reason empty for an eye-count gap; builder card disagrees with
server for 5 eyes; drought % shown under full snow; egg-layer listed "F preg";
duplicate possession messages and "parched" alert; renown pill does nothing;
tour mentions a Migrate button not on the phone toolbar; Mates "Incoming:
None" right after load; Follow-mode taps pick neighbours; toolbar Clear
doesn't cancel a territory and Abandon gives no toast; year/day numbering
mismatches between top bar, API, vitals and alerts; parched herbivores
"scavenge"; auction error text omits the 5-minute floor; self-block silently
ok; `mating_prefs` drops bad players without saying so; COMMAND_NAMES lists 5
names twice; royalty doc comments contradict; no "server restarted, log in
again" message for API clients; no Help on founding ramp or winter provision;
pregnant dam can be listed as not busy; promised dam can get a fixed listing.

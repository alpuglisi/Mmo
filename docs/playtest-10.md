# Playtest 10: six AI players, after the playtest-9 fixes (2026-11-19)

Six AI agents played on one server (`--size 128 --seed 59 --tick-ms 100`:
a game day ≈ 5 s, a year ≈ 10 min) for about 52–65 minutes each, game days
~12–802, on main at d057fc7 (all of round 10: wild refuge, world-share
crowding, wild litters a tenth, reinforcements by season peak, grace from the
lineage's end, no natural cross-player mating, rankings need living animals,
graced squats kept, founding placement by rival animals and water,
reinforcement placement, grace-end warning, survival judged at the end, journey
counts hunger and dry stretches, dwindling-group alerts, pinned views
re-checked, name containment for every kind, leak fixes, market shield and
fault rules, restore preview/group/season cap, client toasts and Lows, server
Lows). Personas as before. Full logs: `docs/playtest-10-logs/`; screenshots:
`docs/screenshots/playtest-10/`. The server stayed up; no page errors or failed
requests.

## Result

| Player | Final share | Notes |
| --- | --- | --- |
| PocketNomad10 | 14–21% (734–1,530) | Starter retired; a 10-founder graceless re-found of the warm egg design (with burrowing) grew 10 → 734 → 1,530; #1 at the end |
| GeneBroker10 | 9.3% (300) | Four re-foundings to test fees; era 1 #2; no trade completed |
| Rulebender10 | 5.2–8.5% (196–566) | Era 1 #3; a re-found in an empty corner reached #1 among players on day 620 |
| LateComer10 | 13.8% peak (750); era 2 #2 (6.4% mean) | Warm egg herd founded on era 2's first day; retired at 385 days to test grace |
| Herdmaster10 | 24.9% peak (1,372) → extinct | Won era 1 (7.1% mean) and seasons 2–8; world-share brake held it at 24.9%, then winter, flood, pack and drought took it to extinction by day ~555 |
| ApexHunter10 | 1.2–3.7% (peak 112) | Pure carnivore pack, smaller than playtest 9's (240–261); carnivore niche prize |

**No strategy ran away**: era 2 means were 7.1 / 6.4 / 6.0 / 4.7 / 3.9 / 2.1%
with the wild at ~70%; a late joiner on era 2's first day placed #2 (not an
automatic win, not hopeless).

## Playtest-9 fixes confirmed working

World-share crowding (litters ×0.25 past a fifth of the world) and the wild
refuge (wild 3,000–4,400, lows 1,700–1,950); wild interbreeding a tenth (77% →
7%); reinforcement limit by season peak; no fresh grace after re-founding a
lineage of any age (designer, reply, alert agree after retiring); no natural
cross-player gene flow (lineages stayed 100% own alleles beside 90%-fertile
herds); founding placement (15–44 tiles from rivals, water 1–5 steps; no pack
within 25 tiles); grace-end warning names nearby hunters; survival judged at the
end with keep-half/hit/same-lineage in `/api/catastrophes`, tallies and World
line; niche prizes need 25%; pinned view narrows/drops as animals leave; default
view on the largest group; name containment and homoglyphs refused, probe answers
`already_founded` first; anonymous metrics hidden; journey counts thirst and dry
stretches, bed-down lookahead over the whole trip, bands instead of counts;
dwindling-group alerts; carnivore advice; poison wording; restore preview, group,
unit, nothing in midwinter; owed debt in ledger/shop; retire dialog quotes the
fee; listing dormancy; listing-closed alerts name the killer; seller sleep pauses
the deadline; 0% offers refused; desktop toasts clear of tools; folded notices;
new lineage starts clean; designer re-checks only on change; animal-name confirm.

## Findings, by priority

### High

1. **The market shield is free, renewable immunity.** Any open offer shields its
   animals from other players' predators; `offer_to_lineage` with no price costs
   nothing, uses no mating slot, and is refunded on expiry — 6–12 animals per player
   immune, renewable every 10 real minutes; `request_mating` shields the rival's
   animal too; an attack order on a shielded animal is accepted with no warning and
   never lands; shielded animals still get "being hunted" alerts. Stud listings
   (the paid, visible part of the market) are *not* shielded.

### Medium

2. **The market is still thin**: no trade completed in 55 minutes (23 attempts, 2
   accepted offers); offering to a whole lineage lets the seller pick an animal 107
   tiles away and the buyer forfeits when its unsteered animal can't arrive; Accept
   gives no distance warning; mating updates every ~2.5 s (toast spam on the other
   player's phone, and both animals' exact positions every update).
3. **restore_pasture**: the free preview is an exact census of rival animals within
   12 tiles of any group (`also_feeds` with counts); one stray rival animal prices
   the restore by its owner's whole lineage (4× a small player's price); preview ≠
   purchase seconds later; drought previews offer 9 food for 1,554 renown with no
   hint; it still restored food in midwinter and in an 88–95% drought for one player.
4. **Near-copy names**: one-letter deletions pass in nameplates and animal names
   ("LateComr10" bought and worn, "Herdmastr10", "GeneBrokr"); rival lineage names
   pass in titles/animal names ("Ironfang"); "Nomad Hunter" from two players' names;
   checks only against players who exist at naming time.
5. **Grace preview wrong while a lineage is alive**: designer / compatibility
   `grace_note` promises "Newcomer grace: 60 days" for a lineage older than 375 days
   (after retiring: none); a small re-found recomputes the winter extension from the
   old founding day and brings back a run-out grace (12.2 days on day 137).
6. **Public achievements feed lists every living lineage as "Survivor"**
   (`/api/achievements` `recent` filtered by `real`, not `survival_credited`), next
   to "1 of 5 survived".
7. **Predators weaker again**: pack peaked at 112 (playtest 9: 261); a fresh world's
   wild starts low so the refuge leaves an early pack little legal prey; toxic herds
   can't feed a pack; packs still scatter over the map (`pack_cohesion` off).
8. **Steep boom-bust and narrow design space**: four of six players ran near-identical
   warm egg herds (90–100% mutually fertile); the era-1 winner went 1,372 → extinct
   within ~130 days; a graceless 10-founder re-found reached #1 (734 → 1,530); an
   isolated corner re-found reached #1 from 31 founders.
9. **Reinforcements land inside a rival territory** when the pack lives there (no hint
   that the rule was relaxed), and 2 tiles from a territory's edge; 2 bought in one
   spring after winter despite the season-peak rule (one report).
10. **Merging a group gives a false "Group X has died out"** (group_peak not cleared on
    merge); retiring also sends "Group C has died out".
11. **Drought banner covers Pin view and the Food layer button** on desktop (1280×900)
    while telling players to use the Food layer; toast stack covers Release at
    1280×800; on phones the half-open sheet hides Release after Possess.
12. **Alert floods**: territory alerts swing outgrown / grazed bare / no longer feeds /
    recovered / starving; winter hungry/hunted/crashing alerts every ~5 s while 70% of
    the herd sleeps; "Your eggs are failing" for by-design chill (and tells a litter-4
    lineage to lay fewer).
13. **Leaks** (logged in): `/api/metrics` players_online, connection and login counts;
    migrate bands without a cooldown (8/s maps the crowds); listings still 16-tile
    cells in one report; `apply_ms.n` public.
14. **Split founding** lands in two clusters 110 tiles apart and the landing text gives
    their midpoint (6 tiles from a rival herd); a territory claim 28 tiles away is a
    free long move with no predator warning; rally into a rival territory has no
    warning.

### Low

"about about N days"; "about 1 real minutes"; float noise in listings (body_size,
age_days) and migrate replies (dry_stretch_tiles, thirsty_share, route points);
`mostly_water` water_share; "sterile … drift risk high" (and "drift risk" for players
at all); "5 renown … forfeit (5 after the market fee)"; nested re-found rule text;
retired lineage shown as "extinct day 161" (retired 165) and the extinct banner says
"died out"; designer doesn't open after retiring; "Founded 20 of 40" modal not a
dialog; severe winter announced early in the designer; crowding header "653 / 1,500"
while the world-share cap binds; restore dialog "group 0" vs "main group" / "Group A";
`fertile_with: 2` with every row 0; survey price vs quote; an offer made before
connecting never announced; retiring voids an offer as "died before the mating";
`/api/species` lists a retired lineage; "near 16, 80" for a 16-tile cell; listings
don't show age vs lifespan; founding reply's drought provision line meaningless;
landing text not shown in the client itself; grace "ends on day N" drifts; pinned view
never widens again; group peak includes young (yearly dwindling alerts); species tree
draws separate foundings as splits; renown piles up (1,700–4,000); animal sprites large
when zoomed out; empty Group A wording unchanged.

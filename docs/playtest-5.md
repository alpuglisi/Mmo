# Playtest 5: six AI players, after the playtest-4 fixes (2026-11-18)

Six AI agents played on one server (`--size 128 --seed 37 --tick-ms 100`:
a game day ≈ 5 s, a year ≈ 10 min) for about 50–60 minutes each, game days
~12–730. Personas as before: herbivore swarm (Herdmaster5), predator
(ApexHunter5), breeder/trader (GeneBroker5), phone newcomer on an emulated
iPhone 13 (PocketNomad5), late joiner who registered 25 minutes in
(LateComer5), exploit tester (Rulebender5). Full logs:
`docs/playtest-5-logs/`; a few screenshots: `docs/screenshots/playtest-5/`.
The server stayed up; no page errors, console errors or failed requests.

## Result

| Player | Final share | Notes |
| --- | --- | --- |
| PocketNomad5 | 22.6–24.6% (2,090, cap) | Starter hunted to 9 by player wolves; re-founded a cold brumating toxic bright flier with 10 founders → cap in ~25 wall minutes |
| Rulebender5 | 16–23.5% (2,100, cap) | Two warm walkers collapsed in a drought; a half-size (22) cold flier re-found reached the cap in ~16 minutes; won seasons 10–11 |
| LateComer5 | 16–17.2% (1,600) | LateComer4's exact cold flier founded day 313; 2nd in era 1 with 47 days played |
| GeneBroker5 | 4.4–5.6% | Ended on a cold flier too; 0 completed market sales all game |
| ApexHunter5 | 1.3–1.6% | Pure carnivore pack held 60–90 without orders; omnivore pack peaked at 223; 63 attack kills |
| Herdmaster5 | 0.7–1.9% | Warm egg-laying hibernator (the playtest-4 co-leader); won era 1 (3.2%); drowned, hunted, lost eggs |

## Playtest-4 fixes confirmed working

Big-group drift-back fixed (home circle ≤ 12; centres 2.6–6.2 tiles from target
for 250–400; 1,460 still 15 out); every migration ends with a reason (alert
or group line); realistic territory food and standing-days warnings; cooldown
waived for bare/dry; far claims become migrations; overlap refused; orders say
what they replace; drought-aware water warnings; lineage warnings; player
predator packs self-sustain while small (45 → 93 without orders; toxin deaths
4 vs 36); learned toxic avoidance ~62–70%; founding placement (no water, ice,
corners or edges seen); sleepers no longer die of thirst; possessed animals
hibernate, alert spam gone, second possession says it released the first;
attack reach 12; grace cap and end alert; half founders after retire or
extinction for ~30 real minutes, shown in the designer, API and an alert;
spectate hides player herds; retire transfer gives no legacy; rendezvous on dry
land, no lure, pauses while both sleep; dam minimum 5, hold ≤ 30 days, cancel
forfeit; provisions with limits; reinforcements beside the herd; nameplate
filter for "Game Master", "GM", "Mod", "Owner", "wild"; phone bottom edge,
chooser width, second notice folded; winter advice by thermoregulation;
server-measured fertility in the designer; "Why are my animals dying?" help;
day numbering consistent in the UI; near-cap alert once.

## Findings, by priority

### High

1. **Cold-blooded brumating toxic fliers dominate as a class.** Four of six
   players converged on them; three sit at the hard cap, holding 56–72% of
   player alleles; every warm player lineage collapsed below 2%. Winter rest
   works (no winter births, ~10% toll), but cold blood is fully sterile with
   warm lineages, toxins plus bright colours deter predators, flight and
   brumation give near-zero mortality (10 founders → cap in ~25 minutes; 3
   deaths, all old age, over 180 days; none through a severe winter or a
   52–63% drought).
2. **The breeding market is walled off**: cold blood is sterile with every warm
   lineage and most players are cold; 0 completed cross-player sales in ~50
   minutes (all 4 purchases failed when the buyer's animal died); the market
   title stayed "Nobody yet" for 11 seasons.
3. **Warm-blooded + abandoned eggs never hatch** (252 eggs chilled over 45
   summer days, 0 births): the designer offers it with no warning; the summary
   says "by design".
4. **Sleeping hibernators drown when water rises** (25 of 115 in one day at a
   thaw/flood; waves of 16 and 12 earlier); no flood alert exists.

### Medium

5. **Names**: "wild" passes lineage and species names (the largest species on
   the server, ~1,600 adults, is named "wild"; names can't be changed after);
   "G.M." passes titles because reserved words are checked per word after
   splitting on punctuation ("g", "m"); also "G M", "G-M", "GMs", "M.o.d",
   "GameMod", "TheOwner", "SupportTeam", "Root", "Anthropic", rivals' names
   minus the digit.
6. **Market exploits**: the buyer's own animal dying refunds the escrow in full
   (hold a dam up to 30 days then release free); stud-lock for 0–1 renown (the
   5 minimum is dams only); a pending price-0 request marks a listed dam busy
   (free 10-minute block, from code); the winter pause only when both sleep.
7. **Species credit**: later copies of a design are filed into the first
   player's species and the namer/dominant player flips to whoever has more
   animals; small player lineages still filed into wild species (and their
   toxins then mark the wild species poisonous for predators); the wild Red fox
   lineage filed inside the Meadow vole species.
8. **Migrations**: long (60–70 tile) migrations of big groups still silently do
   nothing ("ended: dormant"/"stalled" only in the group line; outcome alerts
   pushed out by daily summaries); far-claim migrations start ~12 tiles off the
   real median and give up after ~7 days; far claims and rallies skip the
   dormant-share and route-water warnings; migrate has no destination water
   warning (1,004 fliers sent to a dry spot, 25–83 thirst deaths a day); the
   home circle a migration leaves is never checked for food/water; far claims
   are a free radar of rival territories (overlap refusal names the owner, no
   cooldown).
9. **Territories aren't re-checked as the herd grows** (claimed at 475, held
   1,941, 55% starved in ~10 days); a carnivore rally/territory has no prey
   sufficiency warning (60 → 36).
10. **Eggs**: group orders and fleeing pull brooding parents away (20–66 chilled
    a day while walking vs 0–6 settled) with no warning; hibernation abandons
    the year's last clutches; lifetime chilled still ~2× hatched for warm egg
    layers; migrating leaves young behind.
11. **Warnings**: predation has no lineage-level warning (5–21 eaten a day, one
    alert all game; instinct kills never name the hunter); the hunger warning
    tells a 91%-asleep herd to "move groups"; ~45 unexplained summer deaths
    filed as "cold, heat or winter sleep".
12. **Fairness**: era ranks use only the final snapshot (a day-313 joiner placed
    2nd in era 1; seasons use the mean); offline protection for lineages ≤ 12
    starts instantly on disconnect; a re-founded lineage can inherit an
    already-expired grace (14 of 20 founders eaten) while a later one gets fresh
    grace, unexplained; starter grace ends exactly at winter's start for a
    day-29 founding; survival credit with no animals for up to half a season;
    the severe winter's survival credit went to the next season.
13. **Renown has no use for big lineages** (6,788 unspent; reinforcements and
    provisions capped at ≤ 300 alive); re-founding charges full points for half
    the founders.
14. **Hall of fame undercounts after 10 seasons** (oldest seasons drop out).
15. **Animals die of thirst shown as "drink"** (possessed male at thirst 75–80
    for days; steep-banked pond despite climbing 0.4).
16. **Phone**: the "Here:" chooser covers the possession bar's buttons until
    closed.

### Low

Builder preview "small" vs size slider "medium" for 4.2 kg; two fertility
figures for one species on the card; cooldown "about 2 min" vs 1.5; reinforce
reply's `group` is a count, not an id; reinforcement clones keep generation 0
(free first winter) and inherited age; "drifted from its home" after a
deliberate order; catastrophe alert a day before its listed start; overlap
refusal names "player 4"; a regretted territory locks claims ~19 days;
`standing_days` "about 0 days"; designer forgets the last design after a retire
(per one report); "cold, heat or winter sleep" label; hunger swings; API
`nearby` capped at 256 and set_view radius clamped (a negative radius pins the
view at 1); 49 KB WS message closes the socket silently; 1.5 MB design body
"couldn't be read"; raw NaN → generic bad_message; reach refusal says "it moved"
when it didn't; retire confirm uses `confirm()` and says "a ranked season's
length" (real window 30 minutes); Ranks values cut off at 1280 px and "6
ranked" lists 5; "merged into #4" by number; species named again with its own
name charged 300; designer "Budget 70.2" vs "costing 56.2"; duplicate mating
updates; listing alerts after a retire say "no longer alive"; Market
compatibility list misses a lineage; retire day off by one between alert and
ledger; forfeit doc comment says "at least 1"; possess/split on a rival's id
answers not_owned vs not_found from anywhere; design screen says "died out"
after a retire in a new browser; `/api/design/compatibility` doesn't check name
impersonation; gene keys normalised silently ("fur " = "FUR"); juvenile gets
same_sex before not_mature; "about 1 real minutes"; provision feeding 12 still
costs 40; offer alert "#0#0"; "exhaustion" vs "attrition"; "Conceived ·
fertility 8% · failure chance 0%"; incoming picker lists juveniles; sterile
listings show Buy; walk hint says "click" on phones; tips recur or appear right
after founding; `laysEggs()` labels a live-bearing lineage an egg layer after one
hybrid egg; `/api/species` namer changes for an already-named species; migrate
"that is water or impassable" for a half-tile nudge; Ranks tab id `tab-seasons`;
help misses egg raiders/attrition causes; starter drowned 2 on its lakeshore.

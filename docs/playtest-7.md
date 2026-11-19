# Playtest 7: six AI players, after the playtest-6 fixes (2026-11-19)

Six AI agents played on one server (`--size 128 --seed 43 --tick-ms 100`:
a game day ≈ 5 s, a year ≈ 10 min) for about 50–57 minutes each, game days
~11–730, on main at 8f6e5e8 (round 7 except the boom session's predator and
warnings work, which was merged afterwards). Personas as before. Full logs:
`docs/playtest-7-logs/`; screenshots: `docs/screenshots/playtest-7/`. The
server stayed up; no page errors or failed requests (only the designer's
expected 400s for refused names).

## Result

| Player | Final share | Notes |
| --- | --- | --- |
| LateComer7 | 17.6–27.1% (1,130–1,560) | Cold live-bearing brumating toxic flier founded on day 362 (era 2's first day); won era 2 (mean 12.0%) and seasons 10–12 |
| PocketNomad7 | 12.3–21.3% (1,740) | Starter crashed in a 76% drought; a copy of the warm small egg-laying hibernator class took it to #1 at day 654 |
| Herdmaster7 | 10.9–14% (951; peak 1,443) | Playtest 6's dominant design now weak (50 → 188 → 117); a live-bearing grazer did best |
| GeneBroker7 | 6.1–8.4% | Era 1 #3; nobody answered its offers; no sale completed all game |
| Rulebender7 | 1% (was #1 at the cap, 22.5%, day 559) | A copy of the warm egg-laying herd reached the cap, then retired twice to test re-founding |
| ApexHunter7 | 0.3–0.9% | Live-bearing carnivore pack held 63–84 for 300 days; egg-laying carnivore lost 212 eggs vs ~20 hatched |

No single lineage ran away for the whole game; shares spread 6–27%. But the two
classes that dominated before (warm small egg-laying toxic hibernators, and the
late cold toxic flier) still took the top spots, and a late joiner founding on an
era's first day won that era for the third playtest in a row.

## Playtest-6 fixes confirmed working

Re-founding costs half the founders (rounded up) and 100 renown, doubling (stated
in the designer and retire dialog; grace carried over); journey estimates on
migrate ("About 27 days… 15% of the food… expect heavy losses"); drought foundings
land by lasting water with a drought provision (no thirst deaths at 76%), strong
designer warning from ~31–40%; territory radar closed (radius clamped, refusals
anonymous and cheap, rival territory outlines public, outgrown waiver); water
counted by drinking spots; species credit only for genuine speciation; names
refuse "Head G.M.", "GMBob", "ClaudeBot", "Wild herd", wild species names;
`/api/species` hides rivals' body plans; WS compatibility and `too_big` coarse;
offline protection capped at 900 game days and dated from leaving; niche prizes and
era ranks by mean; hunters named in summaries and alerts; seller-side meeting point;
auction length note; survey purchase; extinct/retired banner; no native dialogs
(except the survey); one designer budget figure, blank traits, the first-winter
grace rule shown; starter's first winter and an egg layer founded before winter
survive; toxin deaths for predators much rarer; players' toxins no longer mark wild
lineages poisonous.

## Findings, by priority

### High

1. **Hungry hunters still never take warning-coloured toxic prey.** In
   `decide.rs` (~1422–1437) the hunger reduction and the 85% cap apply only to the
   learned-lesson term; the warning-colour group benefit (~1.0 where bright animals
   are mostly toxic) and each animal's permanent `bright_aversion` (1.0 after one bite,
   `apply.rs:857`) are neither capped nor lowered by hunger. Against the dominant
   toxic + bright strategy the round-6 cap and round-7 hungry-hunter rule do nothing
   (a pack starved 2–3 a day beside a bright toxic herd that grew 42 → 342). Every
   player herbivore lineage was toxic and bright.
2. **Two classes still take the top** (warm small incubated-egg hibernator toxic
   bright herds explode each spring, e.g. 668 → 1,575 in 27 days; the late cold
   brumating toxic flier went 62 → 1,560 in 200 days and won era 2); four of six
   players ran the warm class, all mutually fertile; one catastrophe roll decides
   which leads. Herbivore design space is narrow (most body sizes 50–90% fertile with
   a wild species).
3. **Predators can't compete** (pure carnivore plateaus at 60–84 vs herds of
   500–1,300; first winter lost 60% inside grace). (The boom session's round-7 prey
   larder scaling and prey filter were not in this build.)

### Medium

4. **Warnings for slow die-offs** (big lineages lost 16–73 a day for weeks with no
   lineage alert; a pack 63 → 23 over 40 days; no cold-death alert; no repeat after the
   first "grazed bare"; no alert while a migrating group starves on the way; 9 drowned
   at a severe winter's onset with no flood alert). (Boom's round-7 thresholds were not
   in this build.)
5. **Territory and destination food estimates 2–5× too high** (claim "supports 1,044
   for 65.7 days" → 232 of 576 starving 4 days later; destination "2,695 for 50.8
   days" → grazed bare 4 days after arrival); a held territory traps the herd
   (1,144 → 449, recovered to 721 by abandoning); far claims report food at the typed
   radius ("radius 300, food_supports 6,211"); a claim made before a drought rises
   holds the herd on dry ground.
6. **Names**: digits next to staff words ("Claude2 fan" public species, "GM2 herd",
   "Mod2 squad", "Wild2 herd", "Head GM2"), run-together lowercase ("claudebot",
   "GMbob", "ownerbob", "Supportbot"), "WildHerd"/"Wildherd", "Meadow vole 2"/"The
   Meadow vole", exact copies of a rival's lineage name ("Ash Fang Pack"); account
   look-alikes still possible by code.
7. **Market**: a buyer gets a full refund despite a forfeit when the seller's animal
   disappears after the buyer's starved (`after_tick` `_ =>` branch); a steered death
   counts only within 1 game day of the last order (park near a predator, release); a
   buyer can still put the meeting point on a third party's pack; no fee below 10 lets
   three colluders farm the market title; a seller can void a sold mating by retiring;
   the re-found fee is capped at the balance (spend renown first → free) and a died-out
   lineage pays none; the market stayed thin (no sale completed).
8. **`preview_mating` leaks rivals' genes**: `body_plan_gaps` at full precision while
   `problem.detail` rounds — solvable with a few own animals.
9. **Possessed plant eaters still kill predators on command** (adult carnivore
   killed); plant eaters credited with "eating" wolves. (Fixed in boom's round 7.)
10. **Late joiners land in the most crowded corner** (inside a rival territory, four
    rivals within 35 tiles, grace only 5 real minutes at this speed): the founding
    search ignores rival lineages and territories.
11. **Designer**: the server's drought/estivation advice never redraws (`checkWater`
    only calls `update()` for other fields); the inline name warning is skipped when
    two refused names give the same text (Found enabled); the re-found budget
    overstates points left (server checks the full design); the interbreeding card
    lists a rival "100% fertile" then says none is in range; warnings lag one check.
12. **Egg layers**: clutches laid in late autumn, right before bedding down, and in
    early spring still chill (3–15 a day, 0 born); warm egg layers lose more eggs than
    they hatch in drought/spring with no warning tied to the cause; an egg-laying
    carnivore's losses aren't flagged in play.
13. Overlap refusal says "territories show on the map" but outlines are only sent for
    the main view window (split groups claim blind); a migrate/far claim drops the
    territory at once and Clear doesn't restore it (cooldown lockout); a 2-animal group
    can squat a key water spot indefinitely (claims can't overlap).
14. Offline cap 900 game days + up to 1,800 fading ≈ 75 real minutes at playtest speed;
    the Lineage tab shows three different durations ("12 hours", "1 h 12 min", "at most
    12 h in any 24").
15. Survivor achievement for a lineage re-founded mid-catastrophe, and for one that lost
    ~99%.
16. Renown still piles up (600–4,700 unspent).

### Low

Survey still uses native `window.confirm` and the shop shows the first survey's result
after later ones; attack-order warning always says "85%" ignoring hunger; a pinned
`set_view` survives retire and re-found; a second hunting party is blind unless the view
is pinned on it; a pack eating its own worded like a rival kill; pack-mates' kills read
as failures (credited only within 3 tiles of the possessed attacker); "Possessed … is
starving" in the same second as an order; "drifted from its home" repeats for a roaming
pack; blocked reinforcement price shows 600 (real 150); re-found designer "54.5 points
unspent" and "40 founders" when 20 land; food hint rates requested not landing founders;
Genes "Clutches 0 / Eggs 1" vs "no clutch now"; Genes feed keeps the retired lineage's
alerts; learning a new poisonous prey isn't announced; grace-end alert lacks a full stop;
grace text "until that winter ends" though it runs a season past; designer "These 40
qualify" for a quick re-found whose grace was continued; founding day off by one (tab vs
API); size words "medium" vs "small"; tapping an animal on a phone shows nothing until the
Animal tab opens; species-name dialog hides the price when unaffordable and closes on
refusal; raw "body_plan 0.30 apart" in the Market; Help "found fewer" vs designer
"whatever its size"; hibernators bed down two weeks before winter; no "while you were away"
summary; foundings labelled "split" and auto-named "(2)/II/III"; migrate warning says
"hibernating or estivating" for brumators; ~30 founder old-age alerts in 3 minutes;
`/api/metrics` public `players_online`; market listings show herd positions on an 8-tile
grid free; every niche prize went to the wild; a provision quote shows the base price when
blocked; fractional auction `days` → generic bad_message; alert ticks change between reads
(dedup shows some twice); species adult counts with float noise (1739.9999999999998);
retire panel reads as a free re-roll (rule only in the confirm); docs stale on grace and
offline protection; Market adult search finds only animals in view; an extinct species
blocks reusing its name; listings outlive old animals with no warning; escort view and
deadline stale while paused; "End the newcomer grace" after a retire; the reduced-founding
notice offers Cancel after founding; warning colours need coloration > 0.6 while the slider
shows "0.60"; journey warning lower-case "the land"; group counts differ between state and
replies; retire dialog vs toast counts; designer drought % vs `/api/world`; hunger alert
percentage wrong after a retire; `/api/map.png` needs login while `/api/terrain` is public;
no "your founders were watered" message; chill alert blames straying when parents slept.

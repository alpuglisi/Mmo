# Playtest 8: six AI players, after the playtest-7 fixes (2026-11-19)

Six AI agents played on one server (`--size 128 --seed 47 --tick-ms 100`:
a game day ≈ 5 s, a year ≈ 10 min) for about 43–57 minutes each, game days
~11–730, on main at b58b177 (round 8 except the balance session's round-8
changes, which were still being tuned and were merged afterwards: honest
signals for bright toxic prey, player/wild isolation, the egg-loss warning).
Personas as before. Full logs: `docs/playtest-8-logs/`; screenshots:
`docs/screenshots/playtest-8/`. The server stayed up; no page errors or failed
requests (only the designer's expected 400s for refused names and one for an
over-budget design).

## Result

| Player | Final share | Notes |
| --- | --- | --- |
| Herdmaster8 | 31–48.6% (2,084–2,100, the hard cap from ~day 260) | Playtest 6/7's warm small incubated-egg hibernating toxic bright herd (40 founders); won era 1 (mean 18.9%) and era 2 (mean 39.7%), legacy #1 seasons 1–10; 19,990 renown unspent |
| LateComer8 | 14–17.2% (742–909) | The cold brumating toxic flier founded 3 days before era 2; fastest-growing player lineage; 2nd from day 480; did not win an era (first miss in four playtests) |
| PocketNomad8 | 2.7–5.6% (274–323) | Starter grew to 171; an estivating copy of the leader never bred and starved; a re-founded starter grew 20 → 323 through a megadrought |
| ApexHunter8 | 2.2–3% (149, its peak) | Live-bearing pure carnivore pack — best predator of all playtests (27 → 149 through five winters), still ~1/12 of the leading herd; won the carnivore niche prize |
| Rulebender8 | 0.5–1.6% | Retired twice to test re-founding (re-found debt verified) |
| GeneBroker8 | 0.1–0.4% | Era 1 #2 (6.0% mean) with ~770 animals, then retired on purpose; its lineage was transferred to Herdmaster8 |

## Playtest-7 fixes confirmed working

Honest prey count for carnivores (burrowed, heavy, graced, toxic excluded;
"feeds N of the group"); pack larder scales with prey (pack grew past the old
60–100 band) and the first winter is less lethal (38% vs 60%); plant eaters can't
attack on command; "killed by its prey fighting back"; pack-mates' kills credited;
View group control; slow die-off, drowning, en-route starving, repeated
grazed-bare, heat, flood and big-lineage hunger warnings; poisonous-prey lessons
announced; territory food estimates now crowding-aware (held at the cap: "supports
1,174", starved past ~1,190) and computed at the clamped radius; starving holders
forage out (no trap); rival outlines near every group (split groups no longer claim
blind); held territory restored on Clear; journey warnings honest; drought foundings
place founders by lasting water with a provision (0 thirst deaths at 40–70%) and say
so; foundings keep out of rival territories; designer redraws on every server answer,
inline name warnings tracked per name, one budget figure, consistent interbreeding
card, server grace preview; phone animal card on tap, species-name dialog with price
and inline refusal, no native dialogs; "while you were away" summary; retire panel
states the rule; survivor needs a quarter of the lineage; offline protection stated
once in real time; re-found halving + doubling fee + debt repaid from income;
predation and steered deaths no longer excused; market title needs ≥10-renown sales
to 2 buyers; seller-retire refund; names refuse digits next to staff words,
run-together "claudebot", "WildHerd", wild species names with extras, rival copies;
`preview_mating` rounds gaps; `/api/metrics` hides counts from anonymous users;
pinned view cleared on retire.

## Findings, by priority

### High

1. **The warm small incubated-egg toxic bright hibernator still runs away** (cap by
   ~day 260; 30–48% share for whole eras; up to 437 births in a day; through two
   megadroughts and an epidemic). This build had balance's round-7 tuning (clutch
   cover, toxin upkeep) but not round 8 (honest signals: hungry predators may take
   bright toxic prey; player/wild isolation). The hard cap fixes the leader's share
   for whole eras; niche prizes then go to the leader too. Herbivore design space is
   still narrow (body 0.5 grazer 62% fertile with the wild antelope) — addressed by
   balance round 8's wild isolation, merged after this playtest.
2. **Retiring hands a whole lineage to a rival (collusion).** Animals with ≥25% of
   another player's genes join that player with no cap check: GeneBroker8's ~470
   animals bred off Herdmaster8's studs went to Herdmaster8 (share 0.409 → 0.460, to
   the hard cap). A feeder account can build on the leader's studs and retire to gift
   share.
3. **The seller's own pack can kill the buyer's animal and the buyer pays**: the
   meeting point avoids only third-party predators, and `buyer_lost` doesn't check
   whether the killer was the seller. Also (from code) a seller can dodge the meeting
   (move the partner, migrate its group, pick its farthest animal) and `lapse()` still
   blames the buyer, who forfeits to the seller.
4. **Large incubated clutches chill entirely**: a warm incubated-egg design with litter
   8 had 0 births in ~140 days (whole clutches lost); the same design at litter 4 bred
   well. The designer charges for the extra eggs and gives no warning; the alert blames
   straying.

### Medium

5. **Name filter false positives** (ordinary names refused as staff words): "Taproot
   grazers", "Beetroot voles", "Arrowroot herd", "Ecosystem grazers", "Systematic
   grazers", "Observer pack", "Badminton hares", "Distaff herd", "Cooperator ants",
   "Modesty voles", "Modena runners", "Claudette herd", "Wildly happy herd", "Wildrose
   grazers", "Wilder herd", "Oscar Wilde voles", "Supports", "The Help", "Gmelina
   moths", "Staffpick". Still passing: "realclaudebot", "theownerbob", "mygmherd",
   "2claude2 fan", "C laude fan", "2wild herd", copies with extras ("Meadow vole II",
   "Red Foxes", "Sunbright Drove II", "New Sunbright Drove"), "Herdmaster8x"; animal
   names and nameplates skip the wild-name and rival-name checks (a stud listed under a
   rival's lineage name); account look-alikes with a prefix/suffix ("8Rulebender").
6. **Grace farming and window mismatch**: grace carries over only within 2× the base
   grace (~120–240 days) while the re-found window is 375 days; chaining small
   re-foundings >120 days apart gives near-continuous immunity, longer than the
   first-winter extension; the retire panel's "a quick re-found continues your last
   grace" is wrong after that.
7. **Market**: free dam lock with an elderly male (old age excused); stud raid —
   natural mating has no owner check, a possessed male sires a rival herd's young with
   no consent or fee (code); seller-retire penalty capped at the seller's balance;
   `preview_mating` `cost` unrounded (exact mismatch recoverable) and listings send
   `body_plan_fertility` at full precision; logged-in players see world-wide command
   counts; `/api/assign?name=` reveals whether an account exists; the market stays thin
   (no sale completed in two of the reports).
8. **Designer estivation advice is a trap**: "Add estivation (0.6 or more)" at ≥40%
   drought, but the estivating lineage slept in spring, had no summer births and starved
   in its second winter while a plain starter thrived; no warning of those costs. No
   designer warning about chill for incubated eggs (392 lost; spring cold blamed on
   straying).
9. **Migrate warnings**: no warning when most of the group is dormant (the order then
   ends "94% dormant"; the success toast is red); no warning when the destination lies
   inside a visible rival territory; no drought warning for a destination that has dried
   out (megadrought: dryness 0.96, water share 0.02, no warning; ~40 thirst deaths); one
   journey estimate far too optimistic (food_share 2.02 → 194 arrived 128, then 87);
   false "drifted from its home" mid-migration.
10. **Squatting**: a group of 13+ can never be displaced, and young count toward it (a
    breeding pair grows past 12 and becomes permanent).
11. **Late joiners still land near rival herds** (inside a third group's territory
    eight days after landing; beside a pack); nothing tells a founder how far the nearest
    rival is.
12. **Predators still trail the herds** (best pack ~149 vs 2,100); hungry hunters vs
    bright toxic prey unchanged in this build (balance round 8 merged afterwards).
13. **Renown piles up** (3,200–19,990 unspent); survivor achievements near-automatic
    (every player survived all three catastrophes).
14. **Group alerts carry the lineage-wide starvation count** (6-member group "46
    starved"); "obey the claim and stay inside" for groups whose hold was loosened; a
    territory kept during a migration isn't shown to its owner and the reply says it was
    replaced; View group on a scattered group pins the view on empty ground (a lake);
    survey refused with 88 alive because group 0 was empty; attack orders accepted for a
    hibernating animal.

### Low

Survey price rises unexplained; stale survey price in the confirm; attack warning
always "85%" (fixed in balance round 8); "Possessed … is starving" with the order; prey
warning fires everywhere ("feeds 0–2") near the herd a pack lives on; designer first
load lags one check; saved last design has an empty name; "born" and "old age" bars
near-identical greens; re-found fee shown as the balance (14) instead of 100, and the
debt is never shown; mating slots sold with no living lineage; blocked reinforcement
quotes 600 (charged 150); "Attack" offered to a plant eater on the possession bar;
survey omits away territories; `drinkable_tiles` large; "1 rival groups"; offer feed
items without a day; naming an animal needs its number typed; Market copy says a fee
"is refunded if they never do"; tour reappears after Skip; ledger income a day early;
"At the destination: … At the destination:" doubled; "expect heavy losses" at 94% food;
"dying off slowly: 954 → 1251"; reinforcement next day off by one; "lost since founding"
count dropped; two alerts with one id; ledger "spent in the shop 0"; Split greyed at 4
groups with no reason; zoomed-out map paints a small square; founding day API vs UI;
species "split" labels and float noise (fixed in balance round 8); retirement worded as
a die-out after a reload (sessionStorage); View group greyed with a tooltip only;
designer may prefill a stale design from localStorage over the server's; one death
alert per starving founder; founders land old; grace preview changes unexplained;
body-size slider "upkeep +78%" at the default; nested parentheses in summaries;
die-off advice useless in midwinter; over-budget design → 400; "while you were away" on
every load; drift alert during a migration; group sizes differ between state and
replies; hibernators bed down ~2 weeks before winter (by design, documented).

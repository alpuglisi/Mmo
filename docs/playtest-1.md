# Playtest 1: four AI players, competitive (2026-11-17)

Four AI agents played the real web client against each other on one server
(`evo-server --size 128 --seed 7 --tick-ms 100`: a game day ≈ 5 s, a year
≈ 10 min) for about 70 minutes, roughly 7½ game years. Personas: a herbivore
swarm (Herdmaster), an apex predator (ApexHunter), a trader and breeder
(GeneBroker), and a newcomer on an emulated iPhone 13 who later probed for
exploits (PocketNomad). Full logs: `docs/playtest-1-logs/`; a few
screenshots: `docs/screenshots/playtest-1/`.

## Result

| Player | Final share | Alive | Notes |
| --- | --- | --- | --- |
| Herdmaster | 18.7% | 1500 (cap) | Won era 2; second lineage (big swimming, hibernating grazer, 124 founders) |
| GeneBroker | 10.1% | 550 | Won era 1 (omnivore, 70 founders); halved every winter later |
| ApexHunter | 2.6% | 1268 | Fox-like re-found grew to near the cap, but only ~10% of its alleles were its own |
| PocketNomad | 1.5% | 42 | Starter design eaten; a 141-founder copy of GeneBroker peaked at 8.7% |

Wild species held about 67%. The server never went down; no unexpected
console errors or failed requests all session.

## Findings, by priority

### Game design and balance (high)

1. **The mating market has no reason to exist.** Score is your alleles'
   share of breeding adults, so accepting a rival's stud only dilutes you and
   renown buys nothing. Zero player-to-player trades all session; renown
   piled up (2k–11k). A diet gap ≥ 0.30 also makes lineages sterile, which
   splits the market by diet without any warning in the builder.
2. **Hybridisation with wild species is invisible and decisive.** A design
   near a wild species merges with it: ApexHunter's lineage was 65% wild-fox
   and 24% GeneBroker alleles, so ~1,400 animals scored 2.6%. Herdmaster's
   lineage held 57% of the Steppe antelope's alleles. Nothing explains this.
3. **Founders are too cheap.** At 0.7 points each, 130–141 founders give an
   instant 5–13% share (many arrive pregnant); a late joiner can take season
   titles. The starter design leaves 90 points unspent and is eaten.
4. **Large foundings starve:** 80+ founders, especially in summer or drought,
   lose ~90% in 20 days; founders can land on grazed-out ground. No food or
   carrying-capacity hint in the builder; no food overlay on the map.
5. **The 1500 lineage cap is invisible and becomes the game:** births stop with
   hundreds pregnant, juveniles count, and the leader just sits at the cap.
6. **Predators can't hunt other players:** prey must be under 1.5× (4.5× in a
   pack) the hunter's mass, possession has no attack, territory defence ignores
   larger animals. A 40-strong war party into a dormant colony killed nothing.
7. **Hibernators die of thirst** (260 → 79 in an autumn, every winter).

### Bugs and exploits (high)

8. **Migrate drowns and pins groups.** Targets on water or mountains are
   accepted, the route isn't shown, there's no water or ice warning, and the
   order never ends on arrival ("migrating toward…" forever). Groups stay pinned
   through droughts (168 thirst deaths in 30 days) and drown at the melt. Migrate
   without a group applies to every group; rally and territory default to group 0.
9. **No way to retire a lineage → forced-extinction exploit.** Players drown or
   starve their own animals through possession to re-found, and a fresh founding
   spikes share at once (0.07% → 8.1%), which can be timed to season ends.
   Possessed animals with an unreachable order stand until they starve.
10. **Catastrophe warnings are effectively invisible.** World alerts don't toast
    by default (founder deaths do), the start notice falls out of the capped
    feed, and on phones only a truncated top-bar pill shows it.
11. **Griefing:** a pending mating offer strips offline protection from both
    animals (no slot cost, no consent, up to 8 open); buying a rival's dam and
    never bringing the male locks her until the season ends; paid request spam
    with no block or auto-decline.
12. **"Species founded" farmed:** a lineage detected as a new species, merged
    back and detected again counted twice (+100 renown each).

### Client UI (medium)

13. Panels that rebuild every state push lose input: the Matings "My adult"
    select resets after ~3 s (a request went out with the wrong animal); "My
    organisms" rows and Incoming requests' Decline buttons move under clicks.
14. Possession in dense herds: a map click selects a neighbour instead of
    walking; the inspector then hides Release. Phone taps hit neighbours too.
15. Map rendering: stale snow blocks persist through summer until reload, and
    after a reload terrain draws as a coarse blocky grid (chunk seams).
16. Times far too short for people at fast speeds and shown only in game days:
    offers expire at weather-season changes, auctions close in seconds, rally
    lapses in 3 game days. Market "days left" countdown is frozen.
17. Matings tab forgets outcomes that happened while it was closed.
18. Failed commands show as "timeout": `bad_message` replies carry no request
    id, and the raw server text (with the list of command names) leaks out.
19. Mates form: preselects a pregnant female, shows "Compatible 100%", then
    fails with a developer-style toast; phones must type the partner id.

### Wrong or confusing information (low–medium)

20. Founding day differs across Family tab, species list and ledger; ledger
    income labels are a day off; day numbering mixes 0- and 1-based.
21. After extinction, Gene drift shows every gene falling to 0.00.
22. Species tree: lineages counted in two species at once, circular "merged
    into" labels, wrong "founded by"/"100% of alleles" attribution, extinct
    lineage still listed with 1 adult; World tab "you #3" vs row 4.
23. Emptied groups keep their territory; "Migration: none" while migrating.
24. Starving animals shown as "rest" at hunger 100 and energy 100.
25. Misleading errors: "not yours" for an own unpossessed animal, "too_far"
    for a juvenile, off-map rally silently clamped.
26. Text: "The a severe winter is over"; tour/Help tab names differ from the
    phone's (Ranks/Mates/Genes); "season" means two different things.
27. Information leaks: `/api/map.png` needs no login and shows everyone's
    animals; `set_view` can be pointed anywhere; listings reveal exact genes and
    live positions; `already_pregnant` reveals hidden state.
28. Case-sensitive player names allow look-alike accounts (from code).
29. Adult sex ratio skewed male (97 F : 212 M in one lineage).

### Smaller items

Empty "points left" field in the builder; Stalker preset leaves diet as
herbivore; group dropdown truncates; Tools menu behind the phone bottom sheet;
"Buy" looks enabled with no adult picked and explains itself only in a hover
tooltip; one death alert per founder floods the feed and never gives the cause;
`/api/lineage/stats` vitals repeat the current day; a startup TDZ error
(`Cannot access 'Ar' before initialization`) once in ~25 loads.

## What held up

Server-side validation against every malicious input tried (other players'
animals, out-of-range prices and coordinates, durations); rate limits;
refunds and market alerts; the rolling offline-protection budget; re-founding;
split/merge/territory; long-press possess; the onboarding tour and phone
layout (no sideways scroll); season and era records; reconnects and logins.

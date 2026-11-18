# Playtest 2: six AI players, after the playtest-1 fixes (2026-11-17)

Six AI agents played on one server (`--size 128 --seed 11 --tick-ms 100`:
a game day ≈ 5 s, a year ≈ 10 min) for about 60–75 minutes each, game days
~0–875. Personas: herbivore swarm (Herdmaster2), predator hunting players
(ApexHunter2), breeder/trader (GeneBroker2), phone newcomer (PocketNomad2),
late joiner who registered 25 minutes in (LateComer), and an exploit tester
(Rulebender). Full logs: `docs/playtest-2-logs/`; a few screenshots:
`docs/screenshots/playtest-2/`. The server stayed up throughout; no console
errors or failed requests were seen, at a sustained load of ~12 on 4 cores.

## Result

| Player | Final share | Notes |
| --- | --- | --- |
| LateComer | 26–40% (peak 2,990 alive, at the 2× ceiling) | Joined on day 321; egg-laying (sterile with every live-bearer), toxic but drab, hibernating burrowing herbivore founded in winter (winter provision), plus cheap reinforcements; won seasons 10–15 and era 2 |
| Herdmaster2 | 5–10% | 4th lineage (egg-laying hibernating burrower) worked; first three died to winters, droughts, drowning |
| GeneBroker2 | 5.7% | Won era 1 and 7 season crowns; lost the lead when its lineage silently became 52% wild antelope |
| ApexHunter2 | 0.2% | Five lineages; pure carnivores starved, omnivore packs poisoned by toxic prey |
| PocketNomad2 | ~2% (era 1: 2nd, 5.7%) | Small cold-blooded carnivore flier worked until a drought halved it |
| Rulebender | extinct | Re-founded in winter deliberately to test retire/ramp |

## Playtest-1 fixes confirmed working

Retire lineage; migrations end on arrival with a drawn route and refuse
off-map targets; catastrophe notices and toasts; interbreeding card, "who can
breed" list and "Whose genes"; founding and food warnings; food layer; the
soft cap itself; panels keep input (My adult select); Mates history; partner
picker; friendly errors with request ids; `/api/map.png` login; look-alike
names refused; request rate limit, block and auto-decline (API); offers no
longer strip offline protection; listing positions coarse; `already_pregnant`
hidden; share-spike exploit closed (re-founding before a season end lowers
the score); death alerts grouped with causes; tour names match tabs; trades
now happen (rivals accept paid offers). Late joiners can compete.

## Findings, by priority

### Balance and design (high)

1. **One strategy dominates: egg-laying + toxic-but-drab + hibernating burrower.**
   Egg-laying makes a lineage sterile with every live-bearer (no hybrid
   dilution); toxins kill predators that can't tell the prey is poisonous
   (coloration decoupled from toxicity); burrows shelter it; hibernation wins
   winters. With the 2× soft-cap ceiling one player held ~3,000 animals and
   ~40% share. Winter decides the board; a boom cohort then dies of old age
   at once.
2. **Player predators aren't viable.** Obligate carnivores starve; body size
   is free, so player herbivores (29–54 kg) exceed what any surviving predator
   can take even in packs; toxic prey is unrecognisable and poisons hunters.
3. **Interbreeding card is wrong / goes stale.** It compares species means at
   design time and said "no one" for designs that the market showed 20–65%
   fertile; drift then made a "safe" lineage 52% wild. It also calls player
   species "wild". The lineage "compatible" badge overstates real fertility.
4. **Offer timing is broken:** every offer and even accepted agreements expire
   at each weather-season change (24 min at 1 s ticks; ~2.4 min in the
   playtest), so the 30-minute purchase deadline (the dam-lock fix) can never
   fire, and most offers lapse unseen. The per-target request window (3,600
   ticks, not scaled) caps trading more than price does.
5. **Trading is marginal; renown has no sink for leaders** (6,700–9,800 idle).
   Royalties pay 1–4/day vs 10–17 income; rival females are nearly always
   pregnant (partner_unavailable); nobody bought listings. Titles are trivial
   (stud market won with one trade; a survival title went to all 6 players).
6. **Reinforcement loop:** hoard renown, retire, re-found, buy 5 reinforcements
   (+50 adults); the price resets every ranked season and survives retire.
   Retiring also deletes other players' alleles living in the lineage.
7. **Newcomer grace is invisible and one-sided:** 20 game days ≈ 100 s at
   playtest speed, no UI indicator, resets on re-founding, and a graced
   lineage can still attack others.

### Bugs (high–medium)

8. **Soft cap not shown:** the Lineage tab still says births stop at 1,500
   (`client/src/lineageInfo.ts`), though the server sends `crowding`.
9. **Drowning:** hibernators bed down on lake ice and drown at the melt;
   founders drown on landing at coastal homes; migrations to targets later
   flooded keep standing.
10. **Droughts give no warning** (only catastrophes do); on phones the drought
    figure is hidden. Territories and rallies kill herds in droughts (no
    water check on claims; claims half over a lake accepted).
11. **Species flapping:** species split and merge repeatedly ("Steppe antelope
    (2)…(7)", "Loophole Boars (2)…(6)"); the bonus is paid once now but
    counters, achievements and alerts still fire, and players still get
    unearned bonuses.
12. **Attack orders:** territory patrol overrides an attack order silently; no
    kill/miss feedback; refusals toasted twice; victims get one alert per order
    with no rate limit; possessed animals can't drink and die unnoticed (bar
    vanishes silently).
13. **Migrations:** sometimes end early (most of the group still far away);
    to scattered groups never end; herds drift back; `migrate` accepts a
    non-existent group.
14. **Info leaks:** `/api/leaderboard/history?genes=true` gives every lineage's
    full genome without login (a design recipe); `set_view` (32-tile reach +
    32 radius) still reveals most of the map; `/api/spectate` locates every
    lineage; `preview_mating` answers for any organism anywhere.
15. **Buyer not told when a bought mating completes; block/auto-decline have no
    UI.**

### Smaller items

Founder-size hint contradicts itself; season notice computed once (stale
"Winter starts in 2 days"); founding ramp and legacy weighting unexplained;
toxin deaths lumped into "other"; "0 fewer organisms" after a catastrophe;
"A Egg Herd of yours"; ledger summary ignores royalty/purchase; fees of
1–9 renown round to 0; 1-tick auctions; `mating_prefs` accepts players 0 and
non-existent; COMMAND_NAMES lacks the new commands; ledger/day labels still a
day ahead in places; eras/years 0- vs 1-based; Market panel rebuild detaches
buttons; huge "My adult" dropdowns; litter size shown as a raw gene; hint
cards cover the possession bar; map chunk seams and stale snow still appear
after long sessions; parched herbivores "scavenge" instead of drinking.

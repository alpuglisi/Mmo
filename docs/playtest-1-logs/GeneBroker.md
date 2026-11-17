# GeneBroker playtest log

Persona: trader / selective breeder, balanced omnivore. Server http://127.0.0.1:8090.

## 11:54 wall · day 34 — registered and founded
- Registered "GeneBroker" via UI (fine). Design (Simple mode): warm, live birth, litter 3, 70 founders, diet 0.45 (omnivore), starter fur/fat/hibernation + keen nose .5, ears/eyes .4, sentry .5, voice .5, claws .4, burrowing .5, muscle .3, caching .4, legs .3, camo .4, pickiness .3, boldness .35. Cost 75.9/100.
  Strategy: big founder count (cheap at 0.7/founder) = big initial legacy share; alarm calls + shared burrows group traits; low pickiness so cross-breeding is cheap.
- UX: `#design-unspent` rendered empty — budget box shows "75.9 / 100" but no "24.1 unspent" hint; easy to found with points left on the table (I did).
- Minor: preview caption says "small warm-blooded omnivore" while body-size slider caption says "medium ... 4.2 kg" (look size bucket vs slider wording disagree).
- Founders arrive 29/70 already pregnant (nice, but surprising — not mentioned in the builder).
- Headless WebGL warnings "GPU stall due to ReadPixels" (benign, SwiftShader).
## 11:56 · day 38
- BUG (minor): ledger entry day field off by one vs its text: `{"day":38,"text":"Day 39 income: ..."}` (every income entry: day N says "Day N+1 income").
- Market is completely empty (0 listings) at day 38 with 3 player lineages. Renown 80.
## 11:55–12:05 · days 45–83
- BUG (intermittent): on one page load the console threw `pageerror: Cannot access 'Ar' before initialization` (minified TDZ ReferenceError) and my click on the Market tab ~3 s after load did nothing (Lineage tab stayed active, screenshot shots/GeneBroker-05-market.png). Reloading: no error, tab works. Looks like an init-order race in the bundle.
- Listed stud #2040 (fixed 8) and auctioned dam #2053 (reserve 5, 3 days) through the Market tab — UI flow works, My listings card is clear.
- UX/DESIGN: auctions last 1–30 *game* days = 5 s – 2.5 min wall clock. My 3-day auction closed (no bids) ~15 s after I created it. A human cannot realistically discover and bid on an auction; the UI says "closes in 1.9 days" which reads as long. Show wall-clock time too, or scale auction durations in real time.
- UX: the public browse list says "No listings match the filters." while I have 2 listings — own listings are hidden by default with no hint ("2 of your own listings hidden").
- UX: "Closes after (days)" field is shown next to "Expires after" even when "As: fixed fee" is selected (irrelevant field).
- DESIGN/BALANCE (big for the market): body-plan sterility at a diet gap ≥ 0.30 (BODY_PLAN_STERILE) means a "balanced omnivore" (diet .45) is sterile with every herbivore (~0.0–0.05) AND every carnivore player. preview_mating: "different body plans: diet (0.45 apart) differ too much to conceive". So my whole persona (cross-breeding with other players) is impossible; the market is partitioned by diet and with 4 players there is nobody compatible. The builder never warns that diet choice decides who you can ever trade with. Suggest: a builder hint "compatible with: X players' lineages", or a "compatible only" filter that says why the market is empty.
- API validation tested (all correctly refused): negative / float / string fee (bad_message), fee 1,000,001 (bad_price), juvenile (not_mature), double listing (already_listed), buying own listing (own_listing), auction 31 days or days+ticks (bad_duration), other player's organism (not_owned), expires_days 0 (bad_expiry).
  - Minor: `bad_message` results come back without the request `id` (id undefined), so a client can't tell which command failed.
  - Minor wording: "#2040 is already listing #1" → "already in listing #1".
- Day 83 standings: I'm #1 legacy (8.0% raw / 5.6% weighted, 291 alive); Herdmaster 1.6% (crashed 233→49), PocketNomad 0.7%, ApexHunter 0.3%. Big founder count + litter 3 + omnivore diet is paying off. Renown ~ +4–5/day.
## 12:00–12:04 · days 96–157
- Winter (days ~90–120) cut me from 325 → ~250 (starvation mostly, some thirst and *drowning* (5 on day 105) — likely thin ice). Recovered to 689 alive by day 157. Hibernation 0.5 + fat 0.7 is enough.
- BUG: Family tab says "Founding · Day 41 · 70 animals alive" but I founded on day 34/35 (species list: "since day 36", ledger grant day 32/33). Three different founding days across the UI.
- GAP (selective breeding): the Organism inspector shows needs, vision, body-plan words — but **no gene values** for an individual. The only way to see an individual's traits is to put it on the market (listing cards show 14 traits). A "selective breeder" can't choose who to pair. Genetics tab only shows lineage-wide distributions.
- BUG/UX: "My organisms" list (Lineage tab) rebuilds its buttons every state push (~1 s). Playwright could not click a row for 30 s ("element was detached from the DOM" repeatedly); a real click that straddles a re-render (mousedown on old node, mouseup on new) is lost. The sidebar scroll position also jumps (button bbox y went to −370 after scrollIntoView). Needs keyed/in-place updates.
- Directed matings inside my lineage: 18 attempts → 6 conceived instantly, 12 refused `too_far` (must be within 1.5 tiles). Own-lineage pairs fail outright instead of becoming a waiting agreement like cross-player ones; the UI gives no "walk them together" helper, you have to possess and escort. Fine but fiddly; the slot is not spent on too_far (good).
- Day 157: #1 with 10.4% raw / 11.1% weighted, 689 alive. Herdmaster 2.8% (217), PocketNomad 0.1%, ApexHunter 1 alive.
- Tried to create a second account to test renown farming via alt accounts (founding grant 50 + 2/day stipend per account, payable to a main account through stud fees in escrow) — my tool sandbox refused this action, so not demonstrated. From code: registration is only limited by a per-IP login token bucket (http.rs:441), the founding grant is per account (economy.rs FOUNDING_GRANT=50), so multi-account renown/market-board farming looks possible in principle (UNVERIFIED).
## 12:05–12:10 · days 164–193
- Day 165: +100 renown "Species founded: achievement bonus" — the species detector split my own lineage into "Brokers" and "Brokers (2)" (same design, same diet .45, just geographic drift). `/api/species` shows "Brokers (2)" containing **Red fox: 2** members — wild red foxes (diet .60) are within the body-plan gap of my omnivores and interbreed naturally. Nice emergent bridge, but:
  - BALANCE: species-split bonus (100 renown + "species" season category) can be farmed by splitting a group and migrating halves apart; it fired for me with zero intent after ~130 days.
- Herdmaster listed 3 bison studs at fee 3. buy_listing with my female → correctly refused `incompatible_body_plans` (message names diet 0.51; detail lists birth_mode/body_size/carnivory gaps), no escrow taken.
- Renown has no sink for me: balance 1645 by day 193 (income ~10–13/day) and nothing compatible to buy. Economy: renown accumulates without use for anyone whose diet has no partner; there's nothing else to spend it on.
- Help dialog is 4 lines — doesn't mention body-plan compatibility, slots, or that seasons are 5 minutes.
- Phone layout (390×844) OK: no horizontal overflow, bottom tab bar fits.
- Day 193: 6 more own-lineage directed matings, all conceived. #1 with 16.4% raw / 13.1% weighted, 779 alive. ApexHunter re-founded (92 alive, 2.1%), Herdmaster 4.6%.
## 12:10–12:14 · days 193–216
- Info leak / fairness: `GET /api/map.png` (no auth) renders every player's organisms coloured by owner over the full map — anyone can locate rival lineages (e.g. to hunt or crowd them). It also uses a different palette from the client (on map.png Herdmaster is yellow, which is the client's "own organisms" colour), confusing if shown to players.
- Strategy: day ~200, split 345 southern members into Group B (`split_group` with named organisms) and ordered it to migrate to (110,100) — empty-ish forest SE — to spread over more food. Lineage tab shows "Group B · migrating toward 110, 100" correctly, but the summary row above still says "Migration: none" (it only describes the main group).
- CONFUSING: "6 mating slots left this season" — slots reset every *weather* season (30 days), not every ranked "Season" (60 days) shown in the Seasons tab. On day 211 (ranked season 4, days 180–239) slots were back to 6 after I spent all 6 on day 193. The word "season" means two different things in adjacent tabs.
- Minor: "By world season" table on day 211 still labels "Year 2 autumn (now) · days 181–210" although the clock says winter (day 211+).
- Day 216: #1 — 19.3% raw / 16.5% weighted, 1025 alive, renown 2072. Herdmaster 4.9%, ApexHunter 1.3%, PocketNomad ~0.
## (timestamp correction: my "12:10–12:14" headers above were estimates; real wall clock at day 242 was 12:11)
## 12:11–12:14 · days 216–253
- Winter 2 (days 211–240): ~20 deaths/day (starved most, but **drowned 1–8 every day**) — 1028 → 837. Drowning in winter is persistent and unexplained in the UI (no hint about thin ice / which tiles); a player can't act on it.
- Group B reached (110,100). Claimed territory for both groups (`claim_territory` ok).
- Day 241 spring: 6 more directed matings, all conceived. Share 24.4% raw / 22.1% weighted.
- GRIEFING (from code, not exercised — nobody is compatible with me): `buy_listing` on a rival's listed dam makes her `listing_busy` for every other buyer and the agreement only spends a slot when the mating actually runs (sim world/commands.rs:171–175); if the buyer never brings the male, the agreement just sits until the season change and the fee is **refunded**. So anyone can lock up every listed dam (one per open offer, max 8) at zero cost, denying the seller all sales. Suggest a non-refundable deposit or a meet-by deadline.
- UX BUG (possession): Organism tab → Possess → hint "click the map to walk there". In a dense herd every map click lands on another animal and **selects it instead of walking** (inspector switched to #26891; the possessed #3053 didn't go where I clicked). Screenshot shots/GeneBroker-15-walking.png. Click-to-walk should take priority while possessing (or require a modifier to select).
- After the inspector switched, `#btn-release` was not clickable (Playwright timeout 30 s); the only control shown was "Possess (releases #3053)". There's no way to just release from another organism's card.
- Possessed #3053 was starving (toast "Possessed #3053 is starving (hunger 100): it is foraging on its own until you give it an order") and was dead a minute later.
## 12:14–12:16 · days 266–292
- Species tab (World): wild **Red fox merged into "Brokers (2)"** and ApexHunter's "Apex Foxes" (carnivore) also merged into Brokers (2) (69% my alleles). So my omnivores interbreed with wild foxes and with a rival carnivore lineage.
  - UX/BUG: the tree reads circularly: "Red fox · merged into Brokers (2)" with "└ Brokers (2)" under it, and "Brokers · omnivore · merged into Red fox". Hard to understand who absorbed whom.
- TRADE: found ApexHunter organisms by pinning views (set_view) at red dots from /api/map.png. preview_mating my males × their females: compatible, fertility 0.24–0.58, mismatch ~0.8, cost 0. Sent 2 directed requests with price 25 each (escrowed 50). Listed stud #24281 fixed 5 renown (60-day expiry) and auctioned stud #20733 (reserve 1, 30 days) for them to find.
  - Strategy logic: my male × their female = a calf they feed that carries 50% my tagged alleles (pure legacy gain); my female × their male would dilute my own calves. "Genes count wherever they live" makes siring into rivals the dominant trade — sellers of studs win, buyers of studs lose share. Worth a balance look: there's no reason to ever *buy* a stud except for traits.
- DISASTER: summer drought (71%) → day 289: 46 died (25 thirst, 20 starved), day 290: 33, day 291: 22. ~1260 alive on day 266 → 679 on day 291. I had claimed territories (radius 24/20) for both groups; I suspect the claim kept them inside drying areas. Abandoned both territories on day 292 to let them roam to water. The UI gives no drought warning/advice when a claimed territory has no water.
- Alerts: "While you were away, your home range (group A's territory) kept hunters off…" — I'm "away" between my short scripted connections; fine. Daily summary alert again off by one ("day":288 → text "Day 289").
## 12:16–12:20 · days 298–318
- Migrated both groups to the east edge (x≈118) where /api/terrain/dynamic shows shallow water + vegetation: deaths fell from 20–46/day to 0–3/day within ~5 game days. (Had to decode the base64 water/veg layers myself — the client has no water/drought overlay to plan this.)
- Offer outcomes: #1 failed (my stud died of thirst; refund + "Listing #7 closed" alerts arrived correctly), #2 **expired** at the weather-season change (day 300) — ApexHunter never answered. An offer made on day 280 lives only until day 300 = ~100 s real time. Far too short for a human counterparty.
- BUG: Matings tab on a fresh page load shows "My open offers: None" and "Recent outcomes: None yet." even though offers #1/#2 had just failed/expired with refunds (and later while #3–#8 were pending I should re-check). Outcomes seem to be kept only client-side from live `mating_update` events, so anything that happened while the tab was closed is invisible — the ledger is the only record.
- Season 5 ended (day 300): alert "you placed #1 of 4 in legacy, #1 of 4 in lineage."
- Day 313: share fell to 16.5% / 13.8% (drought losses), 662 alive; still #1 (Herdmaster 4.8%).
- Sent 6 new requests at price 40 to ApexHunter females (offers #3–#8, 240 renown escrowed). Observed: the **same male #36880 is in 4 pending offers at once** (#4, #6, #7, #8) — request_mating allows it although buy_listing documents `organism_busy` for an organism in two open agreements. One offer (#7) failed seconds later: their female died of thirst.
- preview at mismatch 1.17 shows cost 0.029 and failure 1.5% — the override-cost curve is visible and gentle.
## 12:17–12:20 · days 323–345
- ApexHunter **declined all 4** remaining paid offers (#3–#6) on day 323; refunds + alerts correct. Re-opened the Matings tab afterwards: still "Recent outcomes: None yet." → confirms outcomes are not persisted/fetched on load.
- BALANCE INSIGHT (market has no equilibrium): score = share of tagged alleles, so accepting a rival's stud into your dam always dilutes you, and renown buys nothing else. A rational player declines every outside stud no matter the price (ApexHunter did), and listing your own stud only helps you if rivals are irrational. Renown also can't be converted into anything that affects score. Result: the market only works between players who don't understand scoring. Ideas: let renown buy something (extra slots, founders on re-found, cosmetic titles), or credit a share of offspring alleles to the dam's owner, or make hybrid vigor real so buying a stud has a genetic payoff.
- My auction #8 (stud #20733, reserve 1, 30 days = 2.5 min) closed with no bid → alert "Auction #8 of #20733 closed without a bid: the lot is yours again." Good.
- "Start a new lineage" button exists in DOM but is properly hidden inside `.lineage-extinct` while alive. OK.
- Renown 4600 by day 339 with nothing to spend it on.
- Started a background helper that every 30 s pairs adjacent own adults with free slots (outside winter) and refreshes the status file.
## 12:20–12:23 · days 346–368 — ERA 1 ENDED, I WON
- Era 1 (days 1–360) final: **GeneBroker #1 with 25.4% (weighted 21.7%)**, Herdmaster 8.3%, ApexHunter 0.5%, PocketNomad 0.0%, wild 65.8%. Niche leader: omnivores (80%) and marsh (73%). Achievements +40 (niche dominance) +25 (Survivor). Alerts and World tab era board render fine.
- Catastrophe: severe winter days 330–360 (world pop 4933 → 2069). I was never warned it had started — no catastrophe banner/alert seen in my scripted session around day 330 (World tab said "None yet" at day 266); only noticed it via /api/catastrophes afterwards. (Possibly the banner shows only in an open client at that moment.)
- My lineage: 768 (day 333) → 383 (day 367). Early spring (days 362–366) **drowned 6, 20, 3, 2** — groups I had parked on the east shore for the drought drowned in the spring melt. Moved both groups ~16 tiles inland. The game punishes "park near water" with no visible flood forecast; weather bar shows flood 1%.
## 12:21 · day 372 (real clock; some earlier headers ran ~2 min fast)
- ROOT CAUSE of several off-by-one notes: the UI mixes **0-based** days (header clock "day 34", API `day`, ledger `day`) with **1-based** days (Seasons tab "Season 5 · days 241–300", World tab "Era 2 · Days 361–720", income/daily-summary texts "Day 39 income" for day 38) while the Lineage tab's weather-season board uses 0-based ranges ("days 360–372"). Pick one base.
- API spot checks fine: /api/seasons, /api/seasons/1, /api/seasons/player/4, /api/achievements, history with from>to → [], /api/seasons/99 → 404 no_season, /api/seasons/abc → 400 bad_season, unknown world → 404 with the world list. Minor: `limit=0` on /api/leaderboard/history returns rows (treated as default?).
- Spring yr 3: population still sliding 486 → 368 (starvation after the severe winter); share 20.0%. Auto-helper paired 6 adjacent couples (slots used).
## 12:23 · day 383
- Family tab now shows the split branch "Brokers (2) · Split on day 166 · 538 adults (305 yours) · Body fat ↓ Fang size ↑ Burrowing ↓" — fox genes introgressing (fangs up) via natural interbreeding: the only "cross-breeding" I got was with wild foxes, not with players.
  - INCONSISTENT: Family header says "Species: Brokers", but the World tab says "Brokers · merged into Red fox" and the living species is "Brokers (2)". Founding still "Day 41".
- Toolbar group dropdown text truncated: "All groups (48…" (shots/GeneBroker-t8-family.png, 1280×800).
- Population: adults are 97 F vs 212 M (day 377). Strong male bias among adults — females (litter 3, directed matings) die much more. Possibly a balance issue with pregnancy cost; it caps growth.
- Recovered to 486 alive; share ~20%.
## 12:24–12:26 · days 398–410
- Command rate limit works: 150 `ledger` commands in a burst → 40 ok, 110 `rate_limited`.
- Market completely empty again (0 listings from anyone). Over ~400 game days I saw ~12 listings total from 4 players and zero completed cross-player trades. The market is effectively unused.
- Summer yr 3 started calmly (drought 33%, thirst 29–40). Population 595, share 16.5% (PocketNomad re-founded and briefly took 8%).
- Expansion: split 212 of group A into Group C and sent it to the central forest (62,28) to tap more food.
## 12:26 · day ~425
- Replayed the onboarding tour (Help → Replay): 9 clear steps. Gaps for a trader: nothing about body-plan compatibility (diet/thermo/birth/size must match within ~0.3), that offers expire at the next weather season (~2 real minutes), or that seasons/years are minutes long in real time.
- Autumn yr 3: 750 alive, share 16.9%. Group C reached the centre forest; group B nudged toward the south lake (thirst 52).
## 12:30–12:33 · days 452–490 — LOST THE LEAD
- Day 483: **Herdmaster overtook me** (17.3% vs 16.5%). My share fell through winter yr 3 while theirs climbed 9% → 17%.
- Cause on my side: last 30 days 124 births vs deaths starved 126, **drowned 68**, thirst 52, old age 34. Group C (sent day ~400 from (100,40) to (62,28)) has the central lake (x≈72–84, y 24–50) straight in its path: it shrank 274 → 140 and is now split on both shores (x 59–68 and 85–94). Drowning in the sim = standing on a tile that is `Traverse::Blocked` for the body (world/apply.rs:1034) — i.e. the animals walk out over winter ice / into water and die when it melts or deepens.
  - BUG/UX (high): a `migrate` order appears to send members straight toward the target with no water-aware routing or warning; in winter the frozen lake looks like land, so a migration across it becomes a mass drowning in the melt. The client neither draws the path nor warns "route crosses water/ice". Drownings were my #2 killer all game (also when I parked groups on shores before the spring melt).
- Cleared group C's migration; moved B 12 tiles back from the south lake shore.
- Herdmaster's herbivore bison seem to do better in late game; my lineage's adult sex ratio is now 286 F vs 437 M (age bands), which caps births.
## 12:33–12:38 · days 527–572
- Herdmaster peaked at 22.9% (1499 alive) vs my ~15–17% (≈1200–1280 alive). Group C in the central forest boomed (140 → 503).
- Winter yr 4 starting: to avoid another ice/melt drowning, split C's 232 east-shore members (x>78) into group 3 and `merge_groups` them into A (same shore, no lake crossing), and moved the rest of C west away from the lake (60,34). split/merge commands worked as documented.
- UX: to do this I had to compute shore sides from raw coordinates + decoded water layer; the client's Split tool only offers "far half" or selected organisms, and there's no lasso / "select members west of here".
## 12:38–12:41 · days 576–614
- Winter yr 4 again halved me: 1280 → 622. Last 40 days: births 231; deaths **thirst 204**, starved 160, drowned 30 (down from 68 after the lake fix), old age 18. Thirst in *winter* is my biggest killer — presumably frozen water; the Organism inspector shows "Thirst" but nothing indicates water is frozen or where open water is. Herdmaster's bison barely dip in winter.
- BALANCE: a hibernating omnivore loses ~50% every winter to thirst/starvation; hibernation 0.5 + fat 0.7 is not enough, and nothing in the builder warns that winter water is the constraint (the starter-design doc measured survival, not share).
- Catastrophes so far: severe winter (330–360), flood year (480–503). Ranked season 11 standings: Herdmaster 23.0% legacy, me 15.6%.
## 12:42–12:46 · days 620–675
- Reloaded the client 6× to chase the startup `Cannot access 'Ar' before initialization` error: not reproduced (1 occurrence in ~25 page loads all session). Whole session's browser log: no failed requests, no HTTP ≥400, only that one pageerror plus SwiftShader GL warnings.
- Summer yr 5: share slid to ~10% (Herdmaster 24%). Last 30 days: births 277 but **thirst 168** deaths. I realised my standing `migrate` orders (A→102,38, B→100,92, C→60,34) keep every group pinned around a point long after arrival, so in a dry spell they can't follow water. Cleared all three.
  - UX: a migration order never completes/expires on arrival; the group tab keeps saying "migrating toward…" forever. It should turn into "settled" (or auto-clear) and the docs/tooltip should say it pins the group.
- Mistake log (my strategy): territory claims and pinned migrations both held groups in place through droughts; for this lineage "roam freely" was the best setting most of the time.
## 12:49–12:55 · days 701–790 — ERA 2 ENDED, WRAP-UP
- Era 2 (days 361–720): Herdmaster 20.8%, **GeneBroker 12.8% (#2)**, ApexHunter 2.6%, PocketNomad 1.4%. Niche "omnivore" went to *wild* this era — the wild red foxes that merged into my species now outweigh my own alleles among omnivores.
- My background auto-pair helper was killed by the tool's background time limit at 12:48; did the last spring pairing by hand (6/6 conceived).
- Final (day 790, 12:55): Herdmaster 16.7% / 1500 alive, **GeneBroker 10.1% / 550 alive (rank 2 of 4)**, ApexHunter 2.7% / 1307, PocketNomad 1.1%. Renown 11,336 (all from income + achievements; 0 from the market). Season titles: legacy + lineage crowns seasons 2–6, species founded season 3; hall of fame leader for most of the session.
### Strategy summary
- Worked: 70 founders + litter 3 (cheap points) → instant big share; won era 1 (25.4%). Migrating to water during droughts; moving off shorelines before spring melt; using every mating slot on adjacent own pairs.
- Didn't work: the trader persona. Diet .45 is body-plan-sterile with every herbivore player; the only compatible player (ApexHunter, via fox kinship) declined every paid offer, which is rational under allele-share scoring. Territory claims and pinned migrations trapped groups in droughts; a migration across a frozen lake drowned ~130.
- Late game the hibernating omnivore halves every winter (thirst+starvation) while Herdmaster's bison keep growing.

# Playtest 2 — GeneBroker2 (breeder / trader)

Server http://127.0.0.1:8091. Start 23:20 UTC wall clock, world day 17 (spring, year 0), only wild lineages alive.

## Log

### 23:20–23:30 (game day 17 → 46) Registration and founding
- Spent ~6 wall-min in the designer; world went day 17 → 44 meanwhile. Founded "Broker voles" (lineage 8) day ~42: warm, live birth, litter 4, body 0.85 herbivore, 40 founders, 94.4 pts. 18 of 40 founders arrived pregnant.
- FIXED (pt1 #2): builder now has an "Interbreeding" box: at body 0.5 it warned "Interbreeds with Steppe antelope (100% fertile) … move body size at least 0.26 further". Clear and useful.
- FIXED (pt1 #4): builder warns large foundings starve and mentions drought + Food layer.
- BUG/CONFUSING (low): founder hint is self-contradictory: with **40** founders it says "40 founders (about 1,640 kg) … large foundings tend to lose most of their founders to hunger … Around 40 founders spread out and grow into the land". Same text at 36 founders. Shot: shots/GeneBroker2-04-contradictory-founder-hint.png. Presumably the warning keys on kg (large body) but the advice is phrased as a head count.
- WRONG NUMBER (medium): builder said at body 0.85 "No wild or rival species can interbreed with this design … mates outside your lineage would be sterile". Right after founding, `compatibility` lists **Steppe antelope compatible, fertility 0.645, gap 0.18 (body_size)** and PocketNomad2 0.30. The builder evidently compares against the species mean (0.54) while compatibility uses the best pair of real adults. A player who trusts the builder will still hybridise with the antelope.
- Builder said "no other player's lineage is within breeding range" — yet Herdmaster2's Thunder Bison turned out 100% fertile with me (gap 0.017). Probably founded while I had the builder open (list not refreshed), but worth noting the builder's rival list goes stale.
- Renown shop reads: balance 65, slot 30, reinforce 150 (only while ≤300 alive), fee 10%, royalty 2/pt. Market completely empty at day 46.
- compatibility: Herdmaster2 Thunder Bison 1.00; antelope 0.645; PocketNomad2 0.30; Rulebender Loophole Boars sterile (carnivory gap 0.304, just over 0.30); ApexHunter2 sterile.

(Correction: my wall clock estimates above were off; real time at founding was ~23:23. Game runs ~5 s/day as advertised.)

### 23:23–23:27 (day 46 → 78) First listings and first offer
- Listed 3 studs via API: #3314 fixed 5, #3318 fixed 0 (expires 10 d), #3316 auction reserve 3 for 6 days. **Auction of 6 game days = 288 ticks ≈ 30 s real**; max allowed is 30 days ≈ 2.5 min. The client now shows "closes in 2.3 days (≈ 12 s)" (pt1 #16 partially fixed: real time is *shown*, but durations are still game-time only and far too short for humans/agents that check in every few minutes). Auction closed unsold with no bidders.
- Market tab: "Who your lineage can breed with" list is clear (fully fertile / 77% / sterile with reason "diet 0.31 apart; 0.30 is sterile"). FIXED pt1 #1-part (diet sterility now explained).
- Compatibility numbers jitter between calls (antelope 0.645 via API, 77% in UI a minute later) — sample of 16 adults; fine but surprising.
- Mates tab partner picker lists rivals' females with distance (good: I could find Herdmaster2's ids), but it also lists Rulebender's females (sterile with me) with no "sterile" tag in the dropdown; you only learn when you pick one.
- UX BUG (medium): first "Send request" click (right after typing the price) did nothing: no toast, no offer, no request. Second click sent "Offer 1 sent". Not reproducible yet; possibly the panel re-render on state push swallowed the click (pt1 #13 family).
- Offer #1: my #3320 × Herdmaster2's #2960, price 10. "Lapses when the season changes, in 13 days (≈ 59 s)". **pt1 #16 still present in substance:** a request sent late in a season gets <1 min real time to be answered.
- Rule noted: `too_many_requests` = max 3 requests to one player per (real) hour. With only ONE fertile rival (Herdmaster2), that caps my outbound trade at 3 attempts/hour regardless of price.

### 23:27–23:30 (day 89 → 125) Offer expiry, reinforcement, prefs
- Offer #1 expired unanswered at the season change (day 90), ~60 s real after sending; refund + market alert arrived correctly (`Refund: 10 renown back from offer #1 with Herdmaster2 (expired)`). Escrow/refund ledger correct.
- My herd was starving on grazed-out ground (veg ≈ 1–15 in my cells). Used `migrate` → reply now includes the full `route` (pt1 #8: route shown/returned — FIXED in API), and the order ended on arrival (no "migrating forever"). Founder deaths keep coming though (large 34 kg herbivores, winter); death alerts now name the cause and batch ("3 founders died: 3 starved (#3338, #3315, #3352)") — pt1 smaller item FIXED.
- **Reinforce** at day 106 (199 alive): 150 renown → 10 adults appeared instantly, price doubled to 300. Took me from #2 to **#1 player (4.78% share vs Herdmaster2 4.54%)** on day 111. Balance concern: for a small lineage 150 renown ≈ 30 days of my income buys +7% adults; renown accumulates quickly early (3–5/day + 50 grant), so every small player should buy it every season — that's basically a timed founder top-up, not a trade incentive.
- **`lineage_stats.score`**: shows my alleles placed 0.37% in Rulebender's *Loophole Boars* — a lineage `compatibility` and the Market tab call **sterile** ("diet 0.31 apart; 0.30 is sterile"). The 16-adult sample says sterile, real individuals near the threshold still interbreed. The "sterile" badge is not trustworthy near 0.30.
- **Block / auto-decline: NO CLIENT UI.** `mating_prefs` exists only in protocol.ts (grep: no caller in client/src/ui). pt1 #11 (request spam, no block) is fixed server-side only; a human player can't reach it.
- `mating_prefs` validation (low): `block: 0` (the wild) and `block: 99999` (no such player) are accepted and stored; `block: <self>` silently ignored.
- `too_many_requests`: README says "at most 3 requests to one player an hour"; code is `request_window_ticks: 3600` = 75 game days ≈ 6 min real at this speed. Docs/UX mismatch.
- Day 120 (spring start) sent offer #2: #3320 × Herdmaster2 #2960 for 25 renown (preview fertility 0.67, distance 2.6) — sent at season start to get the full 30 days ≈ 2.5 min window.

### 23:30–23:34 (day 136 → 170) First accepted trade lost, first sale made
- **Herdmaster2 ACCEPTED my 25-renown offer #2 at day 141** (rational rival accepts a paid stud!) — but the pair was 14.3 tiles apart and the accepted agreement **expired at the weather-season change (day 150) 43 s later** before I escorted. Refunded. HIGH: an *accepted* agreement still lapses at the 30-day weather season, so a request accepted late gives seconds to walk the animals together; nothing escorts automatically and no toast says "accepted — go escort now" unless market/offer toasts are on. The Genes tab alert feed did show "Mating offer 2: accepted (organisms are 14.3 tiles apart; they must be within 1.5)".
- Possession seems tied to the WebSocket connection: possessing via a short-lived socket is dropped when it closes (my #5452 showed possessed:false right after). Also possessing a second organism silently releases the first. Fine for the client, but worth documenting.
- Herd intermixed with Herdmaster2's Thunder Bison around (72,45). Many of their females refused with `partner_unavailable` ("can't mate now") — pregnancy hidden, pt1 #27 leak FIXED.
- **SALE (day 170): Herdmaster2 bought my dam listing #4 (#5197, fixed 30)**: "Sale: 27 renown from Herdmaster2 for listing #4, less a 3 renown market fee". Conceived (fertility 0.64). Fee arithmetic correct (10%, floored). First player-to-player trade of the session for me — pt1 #1 ("market pointless, zero trades") is at least partly FIXED: rivals do buy dam matings (their alleles → my lineage = royalty for them).
- Sent two more 25-renown offers (#4 #5452×#7870, #5 #11734×#12807). PocketNomad2 and ApexHunter2 currently have 0 alive (lineages wiped/re-founding).
- Leaderboard day ~160: me 6.9% (585 alive), Herdmaster2 3.0%, Rulebender 2.3%.

### 23:34–23:37 (day 170 → 190) The trade throughput wall
- Offers #4/#5 (sent day 170) expired at day 180 unanswered (17 s left when I looked). I keep sending late in the 30-day season because nothing in the request form warns "this lapses in N s".
- **`too_many_requests` counts expired and refused-by-nobody offers**: 3 requests per 3,600 ticks (75 game days ≈ 6 min) per target. With one fertile rival, that is ~1.2 offers per weather season — the binding constraint on trading, far tighter than slots (6/season) or renown.
- **Pregnancy oracle still exists (low)**: `request_mating` to a rival's female answers `partner_unavailable` ("can't mate now") *before* checking the rate limit, so I can probe which rival females are pregnant for free, even while rate-limited (`too_many_requests` only appeared for a non-pregnant target). pt1 #27 only renamed the code.
- Seller-side escort works: my watcher possessed my dam and walked her to Herdmaster2's stud; completed in 4 s.
- My dam #5197 (sold, pregnant by Herdmaster2's #2941) died ~20 days later → listing closed. The buyer's 30 renown stays paid (fair: "conceived" was delivered).
- Stud auction #5 (30 days, reserve 2) closed without a bid. Alert "Auction #5 of #3328 closed without a bid: the lot is yours again." — clear.
- Validation probes all held: fee 1,000,001 → bad_price; −5 / 2.5 → bad_message (generic); juvenile of a rival → not_owned; days+ticks → bad_duration; expires 0/366 → bad_expiry; bad cursor → bad_cursor; withdraw other's listing → unknown_listing. A **1-tick auction with reserve 0 is allowed** (closes before anyone can see it) — harmless but pointless; minimum should be real-time.
- Achievements feed: PocketNomad2 credited "Species founded" twice: day 45 "Pocket grazers" and day 100 "Pocket grazers (2) split off" while the first lineage still lived → pt1 #12 (species-founded farming) may still be present (+100 renown each).
- Standing day 186: me 9.17% (weighted 11.0%), 737 alive; Herdmaster2 4.89%; Rulebender 2.08%; PocketNomad2 1.33%; ApexHunter2 1.17%.
- **Economy observation (HIGH, design):** once over 300 alive (reinforce cap) my renown has no use except paying for offers, which the rate limit caps. Balance 450+ and rising. Selling dam matings to the #2 player only feeds *their* royalty/reinforcements. As the leader, the rational move is to stop trading — same as playtest 1, just shifted to "once you're big".

(Wall-clock check: it is 23:36 at day ~205; my earlier timestamps between 23:27 and 23:34 were a few minutes ahead of reality.)

### 23:35–23:37 (day 190 → 205)
- Day 195 (new request window): all 7 adult Herdmaster2 females in sight answer `partner_unavailable` — couldn't place a single offer. With one fertile rival whose females are mostly pregnant, the request route is nearly dead.
- Migrated main group to (40,90) before winter (veg there ~170 vs 8–80 in the north). ApexHunter2's "Apex Raptors" sit on my south-east cluster; founders being eaten.
- **Ranks tab:** I won season 3 legacy + largest lineage + **"Stud market 1st of 1 · 27 renown"** — one 30-renown dam sale wins a season title. Trivial to farm with a friend (or a cheap self-arranged sale), and with only 5 players often uncontested.
- **CODE FINDING (HIGH, griefing pt1 #11 still open):** `after_tick` expires *every* open offer at each weather-season change (30 days = 1,440 ticks), but the new purchase-lapse rule (buyer who never comes forfeits 10% / a slot) only fires after `purchase_deadline_ticks = 1,800`. 1,800 > 1,440, so the lapse can never trigger at default settings: a buyer who buys a rival's dam listing at a season start and never brings a male locks her (`listing_busy`, held from breeding) for the whole season and is then **fully refunded**. Same for an accepted request never escorted (my offer #2 was refunded in full after Herdmaster2 accepted and I failed to meet).

### 23:37–23:42 (day 205 → 250) Winter crash, reinforcement again, requests impossible
- Winter (day 210–239): my lineage fell 822 → ~296 (40–50 deaths/day, mostly **starved**, many while `dormant` at hunger 85–100). Hibernators starving in their dens: pt1 #7 (hibernators die every winter) **still present**, now as starvation rather than thirst. The `migrate` order to (40,90) was still "migrating toward 40, 90" 15+ days later — dormant animals don't walk, so the order never completes while half the group sleeps.
- Lineage tab text is stale: "Each player may have at most 1,500 living organisms on this map, young included. At the cap births pause until some of yours die." — the README says the cap is now soft (litters halve at 1,500, hard ceiling 3,000).
- Day 245: alive dropped to ≤300 → **reinforce** (150) → 306 alive, blocked again ("yours has 306"). The 300 threshold makes reinforcements a post-crash refill for the leader too, not only a catch-up for small players. Balance still 1,300+.
- **Royalty is negligible**: with 0.5–1.0% of all alleles exported, royalty is 1–2 renown/day against 10–12 income. Nobody will trade for that.
- Spring (day 240+), fresh request window: tried priced requests (40 renown) to **all 15** adult Herdmaster2 females in sight — **15/15 `partner_unavailable`** (pregnant). Large-bodied lineages are pregnant almost all the time, so directed requests to them basically never go through; the error gives no hint when she'll be free. (Code: `partner_unavailable` is only pregnancy.)
- Ledger renders fine; `royalty` rows appear daily ("Day 250 royalty: 0.97% of all alleles are yours and live outside your lineage").
- ENV NOTE (not a game bug): my background watcher was named `watch.mjs`; at 23:32 I restarted it with `pkill -f watch.mjs`, which may also have killed another player's same-named watcher on this shared machine (a `scratchpad/hm/watch.mjs` is running now). Mine in turn died at 23:37 (probably someone else's pkill). Renamed to gb2-watcher.mjs and kill by PID only from now on. Market/offer events between 23:37 and 23:42 were not logged.

### 23:42–23:48 (day 250 → 332) Market dead from the rivals' side; natural mixing dwarfs trade
- Listings put up at day ~252: dam #25897 fixed 50, dam auction #25617 reserve 20 / 30 days, stud #27035 fixed 0, stud auction #16889 reserve 5 / 30 days. **No buys, no bids.** Both auctions closed unsold (alerts arrived). At day 292 relisted the dam at **fee 0** (a free mating for anyone who wants to export alleles into the leader): no taker either, over ~40 game days.
- Countdown on auction cards now moves ("closes in 28.2 days (≈ 2 min)" → 26.1 days 10 s later): pt1 "days left frozen" FIXED.
- Directed requests to Rulebender (whose boars are now 11% fertile with me) after `set_view` to their herd: 9 of 11 females `partner_unavailable`, 2 `incompatible_body_plans` — 0 offers placed. Every AI player's adult females are pregnant nearly all the time in spring/summer.
- `/api/map.png` now 401 without a token (pt1 #27 FIXED). Listing positions are coarsened to multiples of 8 (FIXED).
- `compatibility` drift: Steppe antelope is now **100% fertile (gap 0.10)** and Rulebender 11% — at founding the builder said "No wild or rival species can interbreed with this design". Lineages converge through hybridisation, so the builder's promise expires within a year. `score.carried` at day 332: my animals carry **9.3% wild alleles** and 7.6% Herdmaster2 alleles; 7.1% of *my* alleles live in Herdmaster2's lineage and 2.4% in wild antelope — all from chance meetings, none from my one sale. **Natural mixing between compatible lineages moves 10–50× more alleles than the market**, and royalty pays for it regardless of trading.
- Market fee is `amount * 10 / 100` (floor): **trades priced 1–9 renown pay no fee** (minor abuse; split purchases to dodge it).
- Score day 332: share 12.3%, head 13.4%, royalty 2/day; balance ~1,850 renown with nothing to buy.

### 23:47–23:50 (day 337 → 371) Era 1 won
- Bought 3 extra slots (30 → 60 → 120, doubling works, `slots_left` 6 → 9) purely to test; they reset to 6 at the next weather season (day 360). Useless to me: I never got to use more than 2 slots in a season because every target female is pregnant.
- Herdmaster2 listed a stud (#12, fee 10) around 23:44 and it was gone within 2 min — missed it (my watcher polled every 60 s). Now polling every 20 s with an auto-buyer for any compatible rival listing ≤ 60 renown.
- **Era 1 (days 0–359) final: GeneBroker2 16.1% (1st)**, PocketNomad2 5.7%, ApexHunter2 3.4%, Rulebender 2.3%, Herdmaster2 1.6%, LateComer 1.5% (a sixth player who joined late); wild 69%. I also hold the tundra niche (78.6%).
- **Herdmaster2 retired Thunder Bison and re-founded "Drought Bison"** (69 adults). Retiring *removed* every Thunder Bison animal — including the ~7% of **my** alleles living in them via chance matings. So alleles you export to a rival (the thing royalty is meant to reward) can be deleted at will by that rival with `retire_lineage`. Conversely Herdmaster2 still scores 3.5% with 69 animals because their alleles live on in *my* lineage (7.6% of my animals' alleles). Hosting a rival's alleles keeps a retired player's score alive.
- Day ~372: Herdmaster2's fresh Drought Bison founders: **31 of 31 adult females `partner_unavailable` (pregnant)** at founding. Founders arriving pregnant (pt1 #3 note) is now universal, which also makes a fresh lineage impossible to trade with for its first weeks.
- LateComer's Late Eggrunners: `compatibility` says "compatible, fertility 0.44 (birth_mode gap 0.22)", yet **13 of 17** real female pairs returned `incompatible_body_plans`. The lineage-level badge overstates who you can actually breed with. One offer placed: #6 my #46373 × LateComer #51872, 40 renown.
- `compatibility` with Steppe antelope is now gap **0.012** (100%): one year after a design the builder said was out of range, my lineage and the antelope have converged completely.

### 23:50–23:54 (day 377 → 420) Second completed trade
- Offer #6 to LateComer (sent day 377) expired at day 390 unanswered (~65 s). 
- Day 395 (5 days into the new season) sent 4 offers at 50 renown: LateComer #7, Herdmaster2 #8/#9/#10. **LateComer accepted #7 within ~40 s**; my watcher's auto-escort chased their wandering female (3.4 → 11.5 → 3.3 tiles) and it **completed at day 405: conceived, fertility 0.20** (mismatch 1.18, cost 0.03). My ledger shows only the escrow line (−50) at creation — no "paid" line at completion, so the payer's ledger never says the money actually left escrow for good (low).
- Herdmaster2's three targets **starved** before meeting (offers failed, refunded with cause "organism 52352 starved before the mating" — good message). Their re-founded Drought Bison is collapsing.
- So: with priced requests sent *early* in a season, 2 of 3 rival players accepted at least once (Herdmaster2 at 25, LateComer at 50). **Rational rivals do accept paid matings** — the problem is logistics (pregnant females, season expiry, escorting), not willingness.

### 23:55 (day ~426) World tab / species
- **pt1 #12 species-founded farming STILL PRESENT (high, economy):** Rulebender has `species_founded: 4` — "Loophole Boars (2)" day 230, "(3)" day 290, "(4)" day 330, "(5)" day 340, each "split off as a species of its own", each later "merged into Red fox", each +100 renown. Same lineage, merging into the wild fox and splitting off again. PocketNomad2 got 2 the same way ("Pocket grazers", "Pocket grazers (2)"). Shot: shots/GeneBroker2-19-world.png.
- Species tree: the wild **Steppe antelope is listed as "merged into Broker voles"**, with "Steppe antelope (3)…(7)" detected every ~10 days and merged into my species; my species line reads "Broker voles · ~1381 adults · **wild 58% of alleles**". Confusing next to `lineage_stats` (my *lineage* is 9% wild): the species and lineage views disagree on how "wild" I am, and the antelope effectively disappeared into my species.
- Era table and day numbering consistent ("Era 1 · days 1–360", "Days 361–720 · 295 days left").
- **Ledger summary bug (low):** Market tab "Recent: earned X, paid into escrow Y, sales Z, refunds W" ignores the new ledger kinds: `ledgerTotals` (client/src/ui/marketData.ts:227) has no case for `royalty` or `purchase`, so royalties aren't counted as earned and shop spending (reinforcements, slots: 630 renown for me) appears nowhere in the summary.
- Phone (390×844) Market tab: no horizontal scroll, compatibility list readable. The "A pregnancy" one-time hint covers the top of the map until dismissed.
- Market tab "Who your lineage can breed with" shows only the first 6 sterile lineages (`sterile.slice(0,6)`) — fine.
- 23:56 started gb2-trader.sh: at each weather-season start, tries 50-renown offers to every rival lineage's densest cell.

### 23:55–23:58 (day 451 → 478) Winter 2 held; rivals hunting me
- Trader at day 451: offers #14/#15 to LateComer placed (50 each); offers #11–#13 were made by *other* players in the meantime (rivals now trade among themselves). One attempt hit `not_found` for a female listed in `nearby` a second earlier, one `too_many_requests`.
- Winter 2: my lineage held at ~1,230 (vs −60% last winter) — births continued, deaths 8–15/day (eaten, disease, thirst). New death cause "disease" appears.
- `attack` alerts (20 so far): "A Broker voles of yours (#61385) is being hunted by a player's Loophole Boars II (#65331)" — Rulebender and ApexHunter2 are using attack orders on me. Alerts work; there's nothing for me to do about it from the alert (no "flee"/"defend" action).
- Renown 4,460 and rising ~17/day with no sink (reinforce needs ≤300 alive; slots useless).

### 00:00–00:07 (day 509 → 555) Lost the lead to LateComer — my lineage is now majority wild
- Rulebender listed a stud auction (#23, reserve 5) that `browse_market` marked `compatible: false, body_plan_fertility 0` (sterile badge data present); it vanished within 20 s, before my UI screenshot loop caught it. Added an auto-screenshot hook.
- **Seasons:** I won legacy + lineage in seasons 3–9. Season 7's "market" title went to **LateComer for 45 renown — the net of the 50-renown offer *I* paid them** (so paying a rival for a mating hands them a season title). Season 8 "survival" title went to **all six players** (everyone survived the day-436 epidemic) — a title everyone gets is meaningless.
- **Day 551: LateComer 10.47% overtook me (9.02%)** despite my 1,331 animals vs their 1,011. Reason, from `lineage_stats.score`: **my animals now carry 51.9% wild alleles** (Genes tab: "Only 42% of your animals' genes are yours · wild 52.1%"), up from 9% at day 332 and ~0% at founding; head_share 17% but share 8.9%. The Steppe antelope converged onto my body plan and flooded in (the species list shows it "merged into Broker voles"). Shot: shots/GeneBroker2-22-majority-wild.png.
  - **pt1 #2 (hybridisation decides games) is NOT fixed — only warned about at design time, and the warning was wrong for my design** ("No wild or rival species can interbreed with this design"). The builder doesn't account for (a) individual variation (best pair, not species means) or (b) convergence over generations. My low mate choosiness (0.2) surely helped; the builder never mentions pickiness/tolerance as a defence.
  - There is no in-game remedy short of retiring the lineage: reinforcements copy living adults (now mostly wild alleles), the market can't buy purity, possession-breeding doesn't scale.
- Soft cap visible: crowding count 919 (adults + ¼ juveniles), litter ×0.88; I briefly had 1,669 alive.

### 00:04–00:08 (day 555 → 586) Deciding to re-found
- Winter 3 crash: 1,331 → 743 alive (day 551 → 586).
- Plan: retire the 52%-wild lineage and re-found a *pure* one (choosy mates, body plan far from every herbivore), then use my idle 6,500 renown on **chained reinforcements** (150 → 300 → 600 → 1,200 → 2,400: +50 adults instantly while ≤300 alive). That's the economy-abuse test: renown hoarded as a big lineage converts into an instant head start for the next one.
- UI: "Retire this lineage" sits at the very bottom of the 900-row "My organisms" list (after "…and 913 more") as a collapsed `<details>`; the real button and a `window.confirm` are inside. Findable only by scrolling past every organism. (My automation crashed on the confirm — my bug, not the game's.)
- Map rendering (pt1 #15) still shows **rectangular chunk seams** after the browser had been open ~45 min: a lighter block on the left edge and hard-edged rectangular snow patches. Shot: shots/GeneBroker2-24-after-retire.png.
- Possession lingers: animals my escort possessed stayed possessed after the agreement ended, and the game then reported "Possessed #48373 is starving … foraging on its own" and "Possessed #48373 was eaten". (The game behaves sensibly; worth a "release on agreement end" option.)

### 00:08–00:10 (day 602 → 620) Retired, re-founded, chained reinforcements (ABUSE CONFIRMED)
- Pre-retire (day 602): 833 alive, share 9.24% (home 7.9%, exported 1.4%), head share 16.5%, foreign 8.6%. `retire_lineage` → `{removed: 833}` (API). After retiring, a reload shows the game view with "Group A (main) · 0" and a **"Start a new lineage"** button (good; pt1 #9 retire FIXED). Note the new browser context replayed the onboarding tour.
- Re-founded day 616 via the UI: "Broker tortoises" — **cold-blooded**, live birth, litter 4, body 0.55, herbivore, scales/armor, brumation 0.6, mate choosiness 0.85, size tolerance 0.15, 52 founders, 89.8 pts. Builder: "No wild or rival species can interbreed". Cold blood = endothermy gap ~1.0 vs every warm species, so this time it should be genuinely isolated.
- **Reinforcement chain:** five `reinforce` calls in a row: 150 → 300 → 600 → 1,200 → 2,400 renown = **4,650 renown for +50 adults instantly** (52 founders → 102 adults on day 616; the 6th failed only for funds at 4,800). **HIGH balance/abuse:** renown hoarded by a large lineage (which has nothing else to buy) converts into ~doubling a new founding, far beyond the 100-point design budget. The price resets every ranked season (60 days ≈ 5 min real), so the next 150/300/600 is available in 5 minutes. A leader can retire and come back at 2× founders whenever convenient; "catch-up for small lineages" becomes "re-found bonus for the rich".

### 00:10–00:14 (day 620 → 664) Pure lineage growing
- `compatibility` for the new lineage: **sterile with everything** (endothermy gap ~0.6 to all warm species, birth-mode/size gaps to the newt). Genetic purity and tradability are mutually exclusive here: the only way to keep the wild out also takes me out of the market entirely. The design rewards isolation, the economy rewards compatibility, and compatibility loses to wild hybridisation over a year.
- Day 661 (new ranked season, reinforce price back to 150): already **372 alive > 300**, so no more reinforcements — cold-blooded tortoises bred fast in spring.
- `score` day 664: share 4.85% = home 3.1% + **exported 1.75%** → royalty 4/day. **The exported alleles of my *retired* lineage (in wild antelope and Herdmaster2's animals) still score for me and still pay royalty.** Retiring therefore costs less than it looks and keeps an income stream; combined with the reinforcement chain, retire-and-refound is cheap.
- **LateComer runaway:** 2,722 alive and 30.7% share on day 664 (1,923 alive at day 620) — well past the 1,500 soft cap (litters ×0.5 at 1,500, ×0.06 at 3,000). The soft cap isn't holding back a leader whose lineage is food-rich; hard ceiling is 3,000.
- **Sterile badge on listings works** (00:09, day ~616): Rulebender's dam #79773 (20 renown) showed in red "Sterile with your lineage: body plans too different." Shot: shots/GeneBroker2-21-rival-listing-auto1.png. Minor: the greyed "Buy for 20" button's stated reason is "Can't buy: pick your adult in 'For my adult' above" — it should say the pair is sterile (picking an adult wouldn't help). The countdown "expires in 28 days (≈ 2 min)" ticked down correctly across 5 snapshots.
- `set_view` correctly refuses places I have no organisms (`out_of_sight`: "you can only look where your organisms are") — pt1 #27 FIXED.

### 00:14–00:17 (day 664 → 680)
- **pt1 #7 still present for brumators:** day 672–675, 341 of 366 tortoises were already `dormant` in *autumn* (brumation 0.6) and dying of **thirst at 12/day** with shallow water 3 tiles from the group centre. Dormant animals don't drink or follow a migrate order, so the player can do nothing. Same pattern as my first lineage's winter starvation.
- **Zero incoming mating requests all session** (no `mating_request` ever reached me), so the Incoming-requests/Decline UI and block/auto-decline went untested in practice. Meanwhile **78 `attack` alerts** ("A Broker voles of yours (#…) is being hunted by a player's Apex Bears (#…)"), toasted by default — a toast flood with nothing actionable.
- Server health fine: tick p50 24 ms / p99 114 ms, 6 conns, 0 dropped; no failed requests or console errors in my client all session (only WebGL "GPU stall due to ReadPixels" warnings from headless SwiftShader).

### 00:17–00:19 (day 705 → 725) Era 2 ends
- **Survivor farmed by re-founding (medium):** I re-founded on day 616 inside a flood-year catastrophe that ended day 623 and got "Survivor: your lineage came through a flood year" **+25 renown** and the season-11 "survival" title for a lineage 7 days old. Combined with the survival title going to all 6 players in season 8, the survival category rewards nothing.
- Catastrophe end text: "The flood year is over: **0 fewer organisms** in the world than at its start." (reads oddly; say "no change" or give the real delta).
- "Season 12 has ended … Era 2 has ended" alerts arrive together; "season" still means both the 60-day ranked season and the 30-day weather season (offers "lapse when the season changes") — pt1 #26 still present.
- **Era 2 (days 360–719) final:** LateComer 40.0% (1st), Herdmaster2 6.8%, **GeneBroker2 6.4% (3rd)**, ApexHunter2 1.0%; wild 45.7%. LateComer took 4 niches.

### 00:19–00:25 (day 725 → 793) Final stretch
- Pure tortoises reached 740 alive and **7.1% (#2)** on day 745 — with 0% foreign alleles (share > head share thanks to the retired lineage's exported alleles).
- Then autumn drought (50%): 250 of 485 already `dormant` in early autumn, **25–50 thirst deaths a day** (day 788–792) → 507 alive, 4.9% (#3). Migrated the group toward shallow water at (17,41); only the awake half can follow.
- LateComer sits at exactly **3,000 alive = the hard ceiling** with ~30–33% share; Herdmaster2's third founding ("Egg Herd") grew to 907 and passed me.

## Final standing (00:26 wall, day 824)
- Leaderboard: LateComer 39.9% (2,772 alive), Herdmaster2 9.8% (871), **GeneBroker2 5.67% (354 alive, pure re-founded lineage) — 3rd**, ApexHunter2 0.26%, PocketNomad2 0.01%; wild 44%.
- Record: 13 ranked seasons, **7 legacy crowns (seasons 3–9), 18 titles; won Era 1 (16.1%)**, 3rd in Era 2 (6.4%). Peak 15.4% / 1,669 alive (season 9).
- Trades: 2 completed (sold a dam mating to Herdmaster2 for 30 → 27 net; paid LateComer 50 for a stud mating), 1 accepted-then-expired, ~14 offers expired/failed, ~100 attempts refused `partner_unavailable`. Renown earned ~11,000 over the session; spent 630 (slots/reinforce, lineage 1) + 4,650 (reinforcement chain) ; ~4,100 left idle.
- Stopped my watcher, trader and browser at 00:26.

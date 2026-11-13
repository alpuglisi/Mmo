#!/usr/bin/env bash
# Balance smoke check: run the standard world (4 wild species + 4 sample
# player lineages) with the default `Balance` on a few seeds and fail only if
# the ecosystem collapses badly. It is a tripwire for rule changes that wreck
# coexistence, not a tuning target: the thresholds sit well below what the
# current rules score (21 of 24 surviving lineage-worlds, score 10.4).
#
#   scripts/balance-smoke.sh            # needs target/release/evo-sim
#
# Env: EVO_SIM_BIN (default target/release/evo-sim), SEEDS (3), DAYS (60),
# SIZE (64), MIN_SURVIVORS (14: lineage-worlds out of 8 x SEEDS that still
# hold at least 4 members including 2 adults), MIN_SCORE (7.0 of 12).
set -euo pipefail

bin=${EVO_SIM_BIN:-target/release/evo-sim}
seeds=${SEEDS:-3}
days=${DAYS:-60}
size=${SIZE:-64}
min_survivors=${MIN_SURVIVORS:-14}
min_score=${MIN_SCORE:-7.0}

csv=$(mktemp "${TMPDIR:-/tmp}/balance-smoke.XXXXXX")
trap 'rm -f "$csv"' EXIT

"$bin" sweep --size "$size" --trials 1 --seeds "$seeds" --rounds 1 --days "$days" --out "$csv"

# One round, one trial (the default balance): the CSV's single data row has
# the mean score and, per lineage, the share of seeds it survived.
awk -F, -v seeds="$seeds" -v min_surv="$min_survivors" -v min_score="$min_score" '
  NR == 1 { for (i = 1; i <= NF; i++) if ($i ~ /^survival /) { col[i] = substr($i, 10); lineages++ } next }
  NR == 2 {
    score = $3
    for (i = 1; i <= NF; i++) if (i in col) {
      n = int($i * seeds + 0.5); surv += n
      printf "  %-22s survived %d of %d seeds\n", col[i], n, seeds
    }
  }
  END {
    if (NR < 2) { print "FAIL: no sweep result"; exit 1 }
    total = lineages * seeds
    printf "balance smoke: score %.2f (min %.2f), surviving lineage-worlds %d of %d (min %d)\n", score, min_score, surv, total, min_surv
    if (score + 0 < min_score + 0 || surv < min_surv + 0) { print "FAIL: the ecosystem collapsed"; exit 1 }
    print "ok"
  }' "$csv"

# Genetics validation

A long statistical run that checks the simulation's population genetics
against theory over many generations and many replicate seeds:
`evo-sim genetics` (`crates/sim-cli/src/genetics/`). This document explains
the method, summarizes what was found, and includes the full report of the
latest long run.

```sh
cargo run --release --bin evo-sim -- genetics                      # quick genome level (~5 s), as in CI
cargo run --release --bin evo-sim -- genetics --long --years 15 --out out/genetics   # both levels (~15–22 min on 4 cores)
cargo run --release --bin evo-sim -- genetics --long --level genome --only "linkage"  # one group
cargo test --release -p evo-sim-cli -- --ignored long_              # the long runs as #[test]s
```

Output: `report.md` (this document's results section), `checks.csv` (one
row per check) and, with the ecology level, `ecology.csv` (a time series per
world, lineage and sample). The process exits non-zero if any strict check
fails. Every replicate draws from its own stream
(`rng_for(seed, STREAM, check, replicate)`), so a run is reproducible and
independent of the thread count. Nothing in the harness changes a world:
no balance or default changed, no simulation hook was added; the ecology
level only reads organisms between steps.

## Method

### Genome level (strict)

Wright–Fisher populations of fixed size `N`: each generation draws `N`
children, each from a uniformly random mother and father (selfing allowed,
so every gene copy is a uniform draw from the parents' `2N`, the textbook
model), through exactly the calls the world makes at a birth:
`Genome::inherit` (genes, obedience, morphology and load loci) and
`Genome::mutate_morph`. Selection, where a check needs it, is viability
selection on the children. Theory then applies exactly (or to the stated
approximation).

The genome's loci reach their random numbers by different code paths, so
every result is reported per **segment**:

| Segment | Loci | Random stream |
|---|---|---|
| genes | the 46 genes before `Obedience` | caller's stream |
| obedience | `Obedience` | own stream, seeded by a hash of the child's other gene alleles |
| load | 4 hidden-recessive loci | caller's stream (the world uses `stream::GENETICS`) |
| morph discrete | eye, ear, leg counts, ear and tail shapes | own stream, seeded by a hash of the child's gene alleles |
| morph continuous | eye azimuth, snout length | same as morph discrete |

Drift is measured on biallelic **markers** (a gene allele 0.25 or 0.75, a
load allele functional or harmful, a discrete allele the default or one step
from it). Pass rule: the expectation lies within the observed mean ±
3.29 SE (a 99.9% interval) plus any model tolerance the check states
(stated in `checks.csv` and the details below); proportions use Wilson
intervals; exact invariants must be exactly 0.

| Group | What is checked | Theory |
|---|---|---|
| Neutral drift | heterozygosity at t = N and 3N; Var(p) at 2N; fixation probability; mean absorption time and conditional fixation time, for p₀ = 0.5 and 0.2, N = 50, 2000 replicates | `E[H_t] = H₀(1 − 1/2N)^t`, `Var(p_t) = p₀q₀[1 − (1 − 1/2N)^t]`, `P(fix) = p₀`, times from the exact Wright–Fisher Markov chain (diffusion approximation `−4N[p ln p + q ln q]` shown alongside) |
| Mutation–drift | infinite-alleles heterozygosity (allele identity = exact value) and within-population allele variance at equilibrium, N = 50 (two starts) and 100 | exact WF recursion `F' = (1−μ)²[1/2N + (1 − 1/2N)F]` (≈ 4Nμ/(1+4Nμ)); `V_eq = (2N − 1)μσ²` |
| Morph step mutation | long-run mean allele distribution over positions per discrete locus (step rate raised to 0.05 so the chain mixes); every allele and expressed value valid in every genome of every generation; variants hidden in carriers | expected allele frequencies follow the mutation chain, so their limit is its stationary distribution: a walk on a path with ends moving inward, ∝ degree (e.g. eyes 1–6: 1:2:2:2:2:1) |
| Mutation rate | mutations per allele per generation per segment (1M children), step SD, every allele's origin tag, step direction and adjacency at rate 0.5 from every value | binomial; Gaussian step σ = 0.07; ends move inward; tags = child's owner |
| Dominance | F1 all dominant and F2 3:1 for default vs variant and for non-default pairs (nearer the default wins, lower count on a tie, shape order); F2 1:2:1 and heterozygote expression for `Coloration`, `Fur` (h = 0.5), `Camouflage` (h = −0.5), `BodySize` (h = 0) | Mendel; `expressed = mean + h|a − b|/2`, F2 mean `0.5 + 0.15h` for 0.2 × 0.8 crosses |
| Additive inheritance | offspring − midparent; segregation variance; within-family variance | `E = 0`; `Var = [(m₁−m₂)² + (f₁−f₂)²]/16`; full sibs vary by `V_A/2` |
| Recessive load | harmful-allele frequency under selection s = 1 and 0.3 (N = 2000, mutation 0.002); homozygous loci per outcrossed and per full-sib-mated offspring | deterministic mutation–selection balance (≈ √(μ/s)); full-sib offspring see the identical-by-descent term ¼q |
| Legacy tags | an owner's allele share (legacy tally with morphology alleles) and share of individuals over 4N generations, children owned by the mother's owner; the designer's share decaying as mutations retag alleles to the owner | martingale: `E[share_t] = share₀`; `E[share_t] = Σ_segments w(1 − μ_s)^t` |
| Linkage | transmission ratio at each of 59 loci; parental-strand agreement for all 1711 locus pairs (phase-known test cross, 400k offspring); Σz² | independent assortment: 0.5, χ²(1711) |

### Ecology level (soft)

Standard worlds (wild ecosystem plus the four sample player lineages,
default balance), 96×96, 15 years each, 4 seeds in parallel. Every day the
harness compares the day's newborns with both parents' genomes (mutations
per segment, origin tags, morphology validity, litter-mates' gene
backgrounds); every 10 days it records per lineage the census, generation,
infinite-alleles heterozygosity, allele variance, divergence from the
founders, genetic distance and fertility between the west and east halves
of the map, morphology allele and phenotype frequencies and the hidden load,
and distance, fertility and hybrid count for every pair of lineages that
was interfertile at founding. Neutral loci are the seasonal genes a lineage
does not express (founders' mean < 0.25, far below the 0.5 threshold, and
outside the distance). Real ecology breaks every idealisation (selection,
overlapping generations, fluctuating census, space), so these checks are
direction and magnitude only; the invariants (valid morphology, the birth
mutation rates, tags) are strict.

## Findings

**No bug in the genome code was found.** Every strict check passes at both
sizes; see the results below. Mutation rates per segment, step mutation
(symmetric, adjacent, inward at the ends, never an impossible value),
dominance (contract rule included), additive inheritance, mutation–drift and
mutation–selection balance, legacy-tag martingales and independent
assortment across all segments all match theory, in Wright–Fisher
populations and, for rates, tags and validity, in real births in the world
(about 475,000 newborns inspected).

Two checks failed in the first long run; both were errors in the harness's
expectations, not in the simulation, and were fixed in the harness:

1. *Mutation step SD 0.0697 vs 0.0700* (consistent over 4 seeds, ~5σ). The
   harness took a mutated allele's source to be the nearer of the parent's
   two alleles (0.3 and 0.7): a step beyond 0.2 (2.9σ) lands nearer the
   other allele and was measured short. With parents homozygous at a value
   of their own (fresh parents per child) the source is exact and the SD is
   0.0700 ± 0.0001. `rng::normal` itself has variance 0.9997 ± 0.0003 over
   20M draws.
2. *Homozygous loci per full-sib-mated offspring 0.0483 vs 0.0539.* The
   textbook `q² + F q(1 − q)` (F = ¼) assumes Hardy–Weinberg grandparents.
   Under selection against homozygotes (s = 1) no adult is homozygous, so
   the quarter of sib-offspring allele pairs that are the two alleles of one
   grandparent are never both harmful. The expectation
   `¼q + ¼·P(both alleles of one adult harmful) + ½g²` gives 0.0482;
   observed 0.0483 ± 0.0002. Inbreeding raises homozygote deaths 5.8-fold
   over outcrossing (expected 5.7).

**Hash-seeded streams (a latent property, not fixed).** The obedience and
morphology loci draw from streams seeded by a hash of the child's gene
alleles (and the parents' alleles at those loci), so that adding them left
every world's shared streams untouched. The consequence: two children with
identical gene alleles at every gene get identical obedience and morphology
segregation and mutation draws. The harness demonstrates it (`hash-seeded
streams` rows): an eye 2/4 × 2/4 cross between two genomes with identical,
fully homozygous gene loci gives one genotype only (never Mendel's ¼ 4/4)
with mutation off, and 8% 4/4 with default mutation (only children carrying
a new gene mutation draw afresh). In drift runs with every gene a marker and
mutation off, the gene background fixes after ~3N generations and the
hash-seeded segments' absorption times come out 2–3% short of theory
(reported as `info`, not judged). It never arises in a world: founders vary,
the gene mutation rate keeps every lineage highly polymorphic (H ≈ 0.6–0.9),
and **0 of 559,676 litter-mate pairs** in the ecology run shared their
gene background. With a polymorphic background (the realistic case) the
hash-seeded segments pass every check, including segregation, drift,
equilibria and independence from the genes (strand agreement 0.5 for all
gene × morphology and gene × obedience pairs). Removing the property would
change every world's draws (a gated `Balance` tunable plus a snapshot
version), which is not justified by an effect that needs a genetically
uniform population.

**Expected deviations from idealised theory** (all within the stated
tolerances):

- *Allele variance at mutation–drift equilibrium* sits 2–9% below
  `(2N − 1)μσ²`, most for the morphology continuous loci. Alleles are
  clamped to [0, 1]; the population mean random-walks (by ~0.2 over the
  sampled 2000 generations), and `eye_azimuth` starts at 0.33 (60°), nearer
  the bound, so clamped alleles pile up at exactly 0 (which also makes them
  identical by value and lowers that segment's infinite-alleles
  heterozygosity slightly).
- *Mutation–selection balance* in a finite population (N = 2000) is ~1%
  below the deterministic value: drift loses rare recessives. The game's
  hazard model (`recessive_hazard` 0.08/day per affected locus) implies a
  per-generation s ≈ 0.91 at 30 days to breeding, so q̂ ≈ 0.045, close to
  the lethal case.
- *Absorption times* use the exact Wright–Fisher chain; the diffusion
  approximation (2.77N at p = 0.5) is ~1% off at N = 50.

**Ecology.** In 15-year worlds the fast lineages reach 35–41 generations
(voles), the slowest ~18 (antelope, herd grazers); 19 lineages pass 20. All
strict invariants hold in real births: mutation rates per segment equal the
configured rates (genes 0.00402 vs 0.004 over 27M alleles, morph steps
0.00050), every visible mutation carries the newborn's owner's tag, and no
impossible morphology appears. Heterozygosity falls from the founders'
level in every lineage toward a mutation–drift balance whose implied
effective size is about half the harmonic-mean census (Ne/N ≈ 0.5, the
usual range for real populations). Lineages keep diverging from their
founders, and expressed genes shift 2.5× as far as the unexpressed neutral
seasonal loci (selection on top of drift). New morphology variants appear
by step mutation in every lineage and stay mostly hidden in carriers.
Two expectations from the brief turned out wrong for this world, for good
reasons, and the soft checks now test what theory predicts instead:

- *West vs east halves do not diverge*: members range across a 96-tile map
  in a few generations, so gene flow (4Nm ≫ 1) keeps each lineage one gene
  pool (net divergence < 0.01, fertility between halves > 0.94); the
  founding placement's small initial difference decays. Isolation by
  distance would need a map many home ranges wide.
- *Interfertile lineages diverge only without gene flow*: marsh newts and
  toxic frogs (no hybrids) drift apart in every world (distance 0.135 →
  0.141, fertility 0.75 → 0.69), while antelope and herd grazers, which
  hybridize constantly, converge (0.063 → 0.042): introgression. Red foxes
  and pack hunters (both extinct by day 200–520, years 2 to 5, in
  these worlds, an ecological outcome diagnosed in the README's *Predators
  over the years*) converge where they hybridized more
  (25–142 hybrids: distance down to 0.04) and drift apart where they barely
  did (9 hybrids, below the 10 the check takes as gene flow).

## Latest results

Produced by `evo-sim genetics --long --years 15` (seed 1) on 4 cores; the
same command reproduces it exactly.

Profile `long`, base seed 1, levels: genome ecology (4 worlds, 96×96, 15 years each). Runtime 895.6 s (genome level 107.0 s, ecology level 788.6 s).

**122 strict checks passed, 0 failed; 9 soft checks passed, 0 soft warnings; 29 informational rows.**

### Results

| Level | Group | Check | Expected | Observed | CI (99.9%) | Result |
|---|---|---|---|---|---|---|
| genome | neutral drift | H_t/H_0 at t=50, genes (all loci as markers, no mutation, p0=0.5) | 0.6050 | 0.6062 | [0.6025, 0.6100] | PASS |
| genome | neutral drift | H_t/H_0 at t=150, genes (all loci as markers, no mutation, p0=0.5) | 0.2215 | 0.2224 | [0.2186, 0.2262] | PASS |
| genome | neutral drift | Var(p_t) at t=100, genes (p0=0.5) | 0.1585 | 0.1583 | [0.1573, 0.1594] | PASS |
| genome | neutral drift | fixation probability, genes (all loci as markers, no mutation, p0=0.5) | 0.5000 | 0.5016 (47146/94000) | [0.4962, 0.5069] | PASS |
| genome | neutral drift | mean time to absorption (gens), genes (all loci as markers, no mutation, p0=0.5) | 136.6 | 136.8 | [135.7, 137.8] | PASS |
| genome | neutral drift | mean time to fixation given fixation, genes (all loci as markers, no mutation, p0=0.5) | 136.6 | 136.8 | [135.3, 138.4] | PASS |
| genome | neutral drift | H_t/H_0 at t=50, obedience (all loci as markers, no mutation, p0=0.5) | 0.6050 | 0.5949 | [0.5683, 0.6214] | info |
| genome | neutral drift | H_t/H_0 at t=150, obedience (all loci as markers, no mutation, p0=0.5) | 0.2215 | 0.2194 | [0.1933, 0.2456] | info |
| genome | neutral drift | fixation probability, obedience (all loci as markers, no mutation, p0=0.5) | 0.5000 | 0.5171 (1030/1992) | [0.4802, 0.5537] | info |
| genome | neutral drift | mean time to absorption (gens), obedience (all loci as markers, no mutation, p0=0.5) | 136.6 | 130.8 | [124.4, 137.1] | info |
| genome | neutral drift | mean time to fixation given fixation, obedience (all loci as markers, no mutation, p0=0.5) | 136.6 | 131.9 | [122.8, 140.9] | info |
| genome | neutral drift | H_t/H_0 at t=50, load (all loci as markers, no mutation, p0=0.5) | 0.6050 | 0.5954 | [0.5818, 0.6090] | PASS |
| genome | neutral drift | H_t/H_0 at t=150, load (all loci as markers, no mutation, p0=0.5) | 0.2215 | 0.2177 | [0.2047, 0.2307] | PASS |
| genome | neutral drift | fixation probability, load (all loci as markers, no mutation, p0=0.5) | 0.5000 | 0.4945 (3956/8000) | [0.4761, 0.5129] | PASS |
| genome | neutral drift | mean time to absorption (gens), load (all loci as markers, no mutation, p0=0.5) | 136.6 | 134.5 | [130.8, 138.2] | PASS |
| genome | neutral drift | mean time to fixation given fixation, load (all loci as markers, no mutation, p0=0.5) | 136.6 | 134.9 | [129.6, 140.1] | PASS |
| genome | neutral drift | H_t/H_0 at t=50, morph discrete (all loci as markers, no mutation, p0=0.5) | 0.6050 | 0.6044 | [0.5928, 0.6161] | info |
| genome | neutral drift | H_t/H_0 at t=150, morph discrete (all loci as markers, no mutation, p0=0.5) | 0.2215 | 0.2231 | [0.2116, 0.2345] | info |
| genome | neutral drift | fixation probability, morph discrete (all loci as markers, no mutation, p0=0.5) | 0.5000 | 0.5065 (5033/9937) | [0.4900, 0.5230] | info |
| genome | neutral drift | mean time to absorption (gens), morph discrete (all loci as markers, no mutation, p0=0.5) | 136.6 | 133.5 | [130.4, 136.6] | info |
| genome | neutral drift | mean time to fixation given fixation, morph discrete (all loci as markers, no mutation, p0=0.5) | 136.6 | 131.4 | [127.1, 135.7] | info |
| genome | neutral drift | H_t/H_0 at t=50, morph continuous (all loci as markers, no mutation, p0=0.5) | 0.6050 | 0.6056 | [0.5867, 0.6244] | info |
| genome | neutral drift | H_t/H_0 at t=150, morph continuous (all loci as markers, no mutation, p0=0.5) | 0.2215 | 0.2277 | [0.2088, 0.2467] | info |
| genome | neutral drift | fixation probability, morph continuous (all loci as markers, no mutation, p0=0.5) | 0.5000 | 0.5073 (2019/3980) | [0.4812, 0.5333] | info |
| genome | neutral drift | mean time to absorption (gens), morph continuous (all loci as markers, no mutation, p0=0.5) | 136.6 | 133.8 | [129.1, 138.5] | info |
| genome | neutral drift | mean time to fixation given fixation, morph continuous (all loci as markers, no mutation, p0=0.5) | 136.6 | 133.9 | [127.3, 140.5] | info |
| genome | neutral drift | H_t/H_0 at t=50, genes (all loci as markers, no mutation, p0=0.2) | 0.6050 | 0.6057 | [0.5990, 0.6123] | PASS |
| genome | neutral drift | H_t/H_0 at t=150, genes (all loci as markers, no mutation, p0=0.2) | 0.2215 | 0.2208 | [0.2157, 0.2258] | PASS |
| genome | neutral drift | fixation probability, genes (all loci as markers, no mutation, p0=0.2) | 0.2000 | 0.2013 (18925/94000) | [0.1971, 0.2057] | PASS |
| genome | neutral drift | mean time to absorption (gens), genes (all loci as markers, no mutation, p0=0.2) | 98.3 | 98.0 | [97.0, 99.1] | PASS |
| genome | neutral drift | mean time to fixation given fixation, genes (all loci as markers, no mutation, p0=0.2) | 176.3 | 175.8 | [173.2, 178.3] | PASS |
| genome | neutral drift | fixation probability, obedience (all loci as markers, no mutation, p0=0.2) | 0.2000 | 0.2143 (426/1988) | [0.1856, 0.2461] | info |
| genome | neutral drift | mean time to absorption (gens), obedience (all loci as markers, no mutation, p0=0.2) | 98.3 | 95.3 | [89.2, 101.4] | info |
| genome | neutral drift | mean time to fixation given fixation, obedience (all loci as markers, no mutation, p0=0.2) | 176.3 | 169.1 | [156.0, 182.3] | info |
| genome | neutral drift | fixation probability, load (all loci as markers, no mutation, p0=0.2) | 0.2000 | 0.1990 (1592/8000) | [0.1847, 0.2141] | PASS |
| genome | neutral drift | mean time to absorption (gens), load (all loci as markers, no mutation, p0=0.2) | 98.3 | 98.2 | [94.7, 101.8] | PASS |
| genome | neutral drift | mean time to fixation given fixation, load (all loci as markers, no mutation, p0=0.2) | 176.3 | 175.4 | [166.8, 184.0] | PASS |
| genome | neutral drift | fixation probability, morph discrete (all loci as markers, no mutation, p0=0.2) | 0.2000 | 0.2005 (1997/9959) | [0.1876, 0.2140] | info |
| genome | neutral drift | mean time to absorption (gens), morph discrete (all loci as markers, no mutation, p0=0.2) | 98.3 | 95.1 | [92.1, 98.1] | info |
| genome | neutral drift | mean time to fixation given fixation, morph discrete (all loci as markers, no mutation, p0=0.2) | 176.3 | 170.5 | [162.4, 178.6] | info |
| genome | neutral drift | fixation probability, morph continuous (all loci as markers, no mutation, p0=0.2) | 0.2000 | 0.2016 (800/3968) | [0.1815, 0.2234] | info |
| genome | neutral drift | mean time to absorption (gens), morph continuous (all loci as markers, no mutation, p0=0.2) | 98.3 | 95.7 | [91.2, 100.2] | info |
| genome | neutral drift | mean time to fixation given fixation, morph continuous (all loci as markers, no mutation, p0=0.2) | 176.3 | 170.7 | [160.1, 181.3] | info |
| genome | neutral drift | H_t/H_0 at t=50, load (load + morph discrete markers, gene mutation on, p0=0.5) | 0.6050 | 0.5982 | [0.5851, 0.6114] | PASS |
| genome | neutral drift | H_t/H_0 at t=150, load (load + morph discrete markers, gene mutation on, p0=0.5) | 0.2215 | 0.2189 | [0.2055, 0.2323] | PASS |
| genome | neutral drift | fixation probability, load (load + morph discrete markers, gene mutation on, p0=0.5) | 0.5000 | 0.4979 (3983/8000) | [0.4795, 0.5163] | PASS |
| genome | neutral drift | mean time to absorption (gens), load (load + morph discrete markers, gene mutation on, p0=0.5) | 136.6 | 136.0 | [132.2, 139.8] | PASS |
| genome | neutral drift | mean time to fixation given fixation, load (load + morph discrete markers, gene mutation on, p0=0.5) | 136.6 | 134.7 | [129.5, 139.9] | PASS |
| genome | neutral drift | H_t/H_0 at t=50, morph discrete (load + morph discrete markers, gene mutation on, p0=0.5) | 0.6050 | 0.6095 | [0.5977, 0.6212] | PASS |
| genome | neutral drift | H_t/H_0 at t=150, morph discrete (load + morph discrete markers, gene mutation on, p0=0.5) | 0.2215 | 0.2212 | [0.2096, 0.2328] | PASS |
| genome | neutral drift | fixation probability, morph discrete (load + morph discrete markers, gene mutation on, p0=0.5) | 0.5000 | 0.5007 (5007/10000) | [0.4843, 0.5171] | PASS |
| genome | neutral drift | mean time to absorption (gens), morph discrete (load + morph discrete markers, gene mutation on, p0=0.5) | 136.6 | 136.0 | [132.7, 139.3] | PASS |
| genome | neutral drift | mean time to fixation given fixation, morph discrete (load + morph discrete markers, gene mutation on, p0=0.5) | 136.6 | 135.8 | [131.1, 140.5] | PASS |
| genome | mutation-drift | equilibrium heterozygosity, genes (N=50, founder-varied start) | 0.4459 | 0.4449 | [0.4417, 0.4482] | PASS |
| genome | mutation-drift | equilibrium allele variance, genes (N=50, founder-varied start) | 1.940e-3 | 1.904e-3 | [1.853e-3, 1.955e-3] | PASS |
| genome | mutation-drift | equilibrium heterozygosity, obedience (N=50, founder-varied start) | 0.4459 | 0.4464 | [0.4261, 0.4666] | PASS |
| genome | mutation-drift | equilibrium allele variance, obedience (N=50, founder-varied start) | 1.940e-3 | 1.917e-3 | [1.554e-3, 2.281e-3] | PASS |
| genome | mutation-drift | equilibrium heterozygosity, morph continuous (N=50, founder-varied start) | 0.4459 | 0.4331 | [0.4201, 0.4461] | PASS |
| genome | mutation-drift | equilibrium allele variance, morph continuous (N=50, founder-varied start) | 1.940e-3 | 1.772e-3 | [1.555e-3, 1.990e-3] | PASS |
| genome | mutation-drift | equilibrium heterozygosity, genes (N=50, uniform start) | 0.4459 | 0.4456 | [0.4424, 0.4488] | PASS |
| genome | mutation-drift | equilibrium allele variance, genes (N=50, uniform start) | 1.940e-3 | 1.889e-3 | [1.827e-3, 1.951e-3] | PASS |
| genome | mutation-drift | equilibrium heterozygosity, obedience (N=50, uniform start) | 0.4459 | 0.4399 | [0.4169, 0.4628] | PASS |
| genome | mutation-drift | equilibrium allele variance, obedience (N=50, uniform start) | 1.940e-3 | 2.006e-3 | [1.630e-3, 2.381e-3] | PASS |
| genome | mutation-drift | equilibrium heterozygosity, morph continuous (N=50, uniform start) | 0.4459 | 0.4423 | [0.4282, 0.4565] | PASS |
| genome | mutation-drift | equilibrium allele variance, morph continuous (N=50, uniform start) | 1.940e-3 | 1.864e-3 | [1.623e-3, 2.104e-3] | PASS |
| genome | mutation-drift | equilibrium heterozygosity, genes (N=100, founder-varied start) | 0.6168 | 0.6139 | [0.6110, 0.6169] | PASS |
| genome | mutation-drift | equilibrium allele variance, genes (N=100, founder-varied start) | 3.900e-3 | 3.804e-3 | [3.666e-3, 3.943e-3] | PASS |
| genome | mutation-drift | equilibrium heterozygosity, obedience (N=100, founder-varied start) | 0.6168 | 0.6140 | [0.5924, 0.6355] | PASS |
| genome | mutation-drift | equilibrium allele variance, obedience (N=100, founder-varied start) | 3.900e-3 | 3.847e-3 | [2.738e-3, 4.957e-3] | PASS |
| genome | mutation-drift | equilibrium heterozygosity, morph continuous (N=100, founder-varied start) | 0.6168 | 0.6109 | [0.5935, 0.6284] | PASS |
| genome | mutation-drift | equilibrium allele variance, morph continuous (N=100, founder-varied start) | 3.900e-3 | 3.684e-3 | [3.040e-3, 4.329e-3] | PASS |
| genome | morph step mutation | stationary allele distribution, eye_count | [0.100, 0.200, 0.200, 0.200, 0.200, 0.100] | [0.096, 0.194, 0.199, 0.203, 0.204, 0.104] | max \|z\| = 1.22 | PASS |
| genome | morph step mutation | stationary allele distribution, ear_count | [0.250, 0.500, 0.250] | [0.248, 0.502, 0.250] | max \|z\| = 0.35 | PASS |
| genome | morph step mutation | stationary allele distribution, ear_shape | [0.167, 0.333, 0.333, 0.167] | [0.165, 0.334, 0.334, 0.167] | max \|z\| = 0.35 | PASS |
| genome | morph step mutation | stationary allele distribution, leg_count | [0.125, 0.250, 0.250, 0.250, 0.125] | [0.125, 0.250, 0.252, 0.249, 0.124] | max \|z\| = 0.42 | PASS |
| genome | morph step mutation | stationary allele distribution, tail_shape | [0.125, 0.250, 0.250, 0.250, 0.125] | [0.122, 0.246, 0.249, 0.254, 0.129] | max \|z\| = 0.89 | PASS |
| genome | morph step mutation | impossible morphology values in 14400000 genomes (step rate 0.05) | 0 | 0 | - | PASS |
| genome | morph step mutation | carriers of a non-default allele vs expressing one | expressing < carrying | 1.0000 carry, 0.9933 express | - | PASS |
| genome | mutation rate | mutations per allele per generation, genes | 4.000e-3 | 3.986e-3 (374696/94000000) | [3.965e-3, 4.008e-3] | PASS |
| genome | mutation rate | mutations per allele per generation, obedience | 4.000e-3 | 4.002e-3 (8005/2000000) | [3.858e-3, 4.152e-3] | PASS |
| genome | mutation rate | mutations per allele per generation, load | 2.000e-3 | 2.011e-3 (16085/8000000) | [1.959e-3, 2.063e-3] | PASS |
| genome | mutation rate | mutations per allele per generation, morph discrete | 5.000e-4 | 5.075e-4 (5075/10000000) | [4.846e-4, 5.315e-4] | PASS |
| genome | mutation rate | mutations per allele per generation, morph continuous | 4.000e-3 | 4.032e-3 (16128/4000000) | [3.929e-3, 4.138e-3] | PASS |
| genome | mutation rate | mutation step SD, genes | 0.0700 | 0.0700 | [0.0698, 0.0703] | PASS |
| genome | mutation rate | mutation step SD, obedience | 0.0700 | 0.0702 | [0.0684, 0.0720] | PASS |
| genome | mutation rate | mutation step SD, morph continuous | 0.0700 | 0.0699 | [0.0686, 0.0712] | PASS |
| genome | mutation rate | alleles whose origin tag disagrees with mutation status | 0 | 0 | - | PASS |
| genome | mutation rate | step direction from interior values, eye_count (share up) | 0.5000 | 0.5007 (66739/133286) | [0.4962, 0.5052] | PASS |
| genome | mutation rate | step direction from interior values, ear_count (share up) | 0.5000 | 0.4986 (33153/66496) | [0.4922, 0.5050] | PASS |
| genome | mutation rate | step direction from interior values, ear_shape (share up) | 0.5000 | 0.5006 (50135/100154) | [0.4954, 0.5058] | PASS |
| genome | mutation rate | step direction from interior values, leg_count (share up) | 0.5000 | 0.4977 (59669/119892) | [0.4929, 0.5024] | PASS |
| genome | mutation rate | step direction from interior values, tail_shape (share up) | 0.5000 | 0.4980 (59886/120250) | [0.4933, 0.5028] | PASS |
| genome | mutation rate | step mutations that skip a value, leave the range or stay put at an end | 0 | 0 | - | PASS |
| genome | dominance | F2 share showing eye_count=2 in 2/2 × 4/4 (default vs variant) | 0.7500 | 0.7520 (75199/100000) | [0.7475, 0.7565] | PASS |
| genome | dominance | F2 share showing leg_count=4 in 4/4 × 6/6 (default vs variant) | 0.7500 | 0.7512 (75117/100000) | [0.7466, 0.7556] | PASS |
| genome | dominance | F2 share showing ear_count=2 in 2/2 × 0/0 (default vs variant) | 0.7500 | 0.7501 (75007/100000) | [0.7455, 0.7545] | PASS |
| genome | dominance | F2 share showing tail_shape=0 in 0/0 × 3/3 (default vs variant) | 0.7500 | 0.7501 (75012/100000) | [0.7456, 0.7546] | PASS |
| genome | dominance | F2 share showing ear_shape=0 in 0/0 × 3/3 (default vs variant) | 0.7500 | 0.7505 (75051/100000) | [0.7460, 0.7550] | PASS |
| genome | dominance | F2 share showing eye_count=3 in 5/5 × 3/3 (contract rule) | 0.7500 | 0.7501 (75006/100000) | [0.7455, 0.7545] | PASS |
| genome | dominance | F2 share showing eye_count=1 in 3/3 × 1/1 (contract rule) | 0.7500 | 0.7508 (75081/100000) | [0.7463, 0.7553] | PASS |
| genome | dominance | F2 share showing leg_count=2 in 6/6 × 2/2 (contract rule) | 0.7500 | 0.7520 (75199/100000) | [0.7475, 0.7565] | PASS |
| genome | dominance | F2 share showing leg_count=6 in 8/8 × 6/6 (contract rule) | 0.7500 | 0.7489 (74892/100000) | [0.7444, 0.7534] | PASS |
| genome | dominance | F2 share showing tail_shape=2 in 3/3 × 2/2 (contract rule) | 0.7500 | 0.7519 (75194/100000) | [0.7474, 0.7564] | PASS |
| genome | dominance | F2 share showing tail_shape=1 in 4/4 × 1/1 (contract rule) | 0.7500 | 0.7499 (74986/100000) | [0.7453, 0.7543] | PASS |
| genome | dominance | F2 share showing ear_shape=1 in 2/2 × 1/1 (contract rule) | 0.7500 | 0.7502 (75017/100000) | [0.7456, 0.7546] | PASS |
| genome | dominance | F2 heterozygote share, coloration (h=0.5) | 0.5000 | 0.5008 (50083/100000) | [0.4956, 0.5060] | PASS |
| genome | dominance | F2 mean expressed value, coloration (h=0.5) | 0.5750 | 0.5751 | [0.5728, 0.5775] | PASS |
| genome | dominance | F2 heterozygote share, fur (h=0.5) | 0.5000 | 0.5016 (50157/100000) | [0.4964, 0.5068] | PASS |
| genome | dominance | F2 mean expressed value, fur (h=0.5) | 0.5750 | 0.5741 | [0.5718, 0.5765] | PASS |
| genome | dominance | F2 heterozygote share, camouflage (h=-0.5) | 0.5000 | 0.5013 (50131/100000) | [0.4961, 0.5065] | PASS |
| genome | dominance | F2 mean expressed value, camouflage (h=-0.5) | 0.4250 | 0.4253 | [0.4230, 0.4277] | PASS |
| genome | dominance | F2 heterozygote share, body size (h=0) | 0.5000 | 0.4987 (49872/100000) | [0.4935, 0.5039] | PASS |
| genome | dominance | F2 mean expressed value, body size (h=0) | 0.5000 | 0.4997 | [0.4975, 0.5019] | PASS |
| genome | additive inheritance | offspring − midparent, genes (additive loci) | 0 | -6.960e-6 | [-9.872e-5, 8.480e-5] | PASS |
| genome | additive inheritance | segregation variance / Mendelian expectation, genes | 1.0000 | 0.9998 | [0.9985, 1.0010] | PASS |
| genome | additive inheritance | within-family variance / (V_A/2), genes | 1.0000 | 1.0021 | [0.9944, 1.0098] | PASS |
| genome | additive inheritance | offspring − midparent, obedience (additive loci) | 0 | -5.578e-5 | [-6.709e-4, 5.593e-4] | PASS |
| genome | additive inheritance | segregation variance / Mendelian expectation, obedience | 1.0000 | 1.0018 | [0.9943, 1.0094] | PASS |
| genome | additive inheritance | within-family variance / (V_A/2), obedience | 1.0000 | 0.9974 | [0.9456, 1.0493] | PASS |
| genome | additive inheritance | offspring − midparent, morph continuous (additive loci) | 0 | -8.747e-5 | [-5.476e-4, 3.727e-4] | PASS |
| genome | additive inheritance | segregation variance / Mendelian expectation, morph continuous | 1.0000 | 0.9986 | [0.9939, 1.0034] | PASS |
| genome | additive inheritance | within-family variance / (V_A/2), morph continuous | 1.0000 | 0.9985 | [0.9622, 1.0348] | PASS |
| genome | recessive load | harmful-allele frequency at mutation–selection balance (s=1, N=2000) | 0.0428 | 0.0424 | [0.0418, 0.0430] | PASS |
| genome | recessive load | harmful-allele frequency at mutation–selection balance (s=0.3, N=2000) | 0.0798 | 0.0791 | [0.0777, 0.0804] | PASS |
| genome | recessive load | game's implied selection and balance | s ≈ 0.9093 | q̂ ≈ 0.0450 | - | info |
| genome | recessive load | affected (homozygous) loci per outcrossed offspring | 8.396e-3 | 8.270e-3 | [7.935e-3, 8.605e-3] | PASS |
| genome | recessive load | affected (homozygous) loci per full-sib-mated offspring | 0.0482 | 0.0483 | [0.0475, 0.0491] | PASS |
| genome | recessive load | inbreeding raises homozygote deaths (sib / outcross) | 5.7365 (> 1) | 5.8408 | [5.5197, 6.1890] | PASS |
| genome | legacy tags | owner's allele share at t=25 (start 0.3) | 0.3000 | 0.2977 | [0.2905, 0.3049] | PASS |
| genome | legacy tags | owner's allele share at t=50 (start 0.3) | 0.3000 | 0.2984 | [0.2900, 0.3068] | PASS |
| genome | legacy tags | owner's allele share at t=100 (start 0.3) | 0.3000 | 0.2993 | [0.2870, 0.3115] | PASS |
| genome | legacy tags | owner's allele share at t=200 (start 0.3) | 0.3000 | 0.3011 | [0.2803, 0.3220] | PASS |
| genome | legacy tags | owner's share of individuals at t=200 (start 0.3) | 0.3000 | 0.3031 | [0.2558, 0.3505] | PASS |
| genome | legacy tags | designer's share after t=25 gens of mutation tagged to the owner | 0.9148 | 0.9150 | [0.9138, 0.9163] | PASS |
| genome | legacy tags | designer's share after t=50 gens of mutation tagged to the owner | 0.8376 | 0.8369 | [0.8345, 0.8392] | PASS |
| genome | legacy tags | designer's share after t=100 gens of mutation tagged to the owner | 0.7037 | 0.7056 | [0.7019, 0.7094] | PASS |
| genome | legacy tags | designer's share after t=200 gens of mutation tagged to the owner | 0.5023 | 0.5033 | [0.4986, 0.5080] | PASS |
| genome | linkage | transmission ratio at each of 59 loci (Mendel's first law) | 0.5 | worst body size = 0.5022 | max \|z\| = 2.77 (limit 4.30) | PASS |
| genome | linkage | parental-strand agreement for all 1711 locus pairs (independent assortment) | 0.5 (unlinked) | worst birth mode–boldness = 0.4970 | max \|z\| = 3.77 (limit 5.00) | PASS |
| genome | linkage | Σ z² over all locus pairs (global independence) | 1711 | 1797 | [1605, 1990] | PASS |
| genome | hash-seeded streams | eye 2/4 × 2/4 with identical homozygous gene backgrounds (mutation off) | 0.25 eye 4/4 (Mendel) | 0 4/4, 1 distinct genotypes | - | info |
| genome | hash-seeded streams | eye 2/4 × 2/4 with identical homozygous gene backgrounds (default mutation) | 0.25 eye 4/4 (Mendel) | 0.0770 4/4, 10 distinct genotypes | - | info |
| ecology | morphology | impossible morphology values (every living organism every sample, every newborn) | 0 | 0 | - | PASS |
| ecology | births | mutations per allele per birth in the world, genes | 4.000e-3 | 4.019e-3 (107138/26655655) | [3.979e-3, 4.060e-3] | PASS |
| ecology | births | mutations per allele per birth in the world, obedience | 4.000e-3 | 3.956e-3 (3758/949990) | [3.749e-3, 4.174e-3] | PASS |
| ecology | births | mutations per allele per birth in the world, load | 2.000e-3 | 1.967e-3 (7240/3680449) | [1.893e-3, 2.045e-3] | PASS |
| ecology | births | mutations per allele per birth in the world, morph discrete | 5.000e-4 | 5.027e-4 (2367/4708839) | [4.698e-4, 5.378e-4] | PASS |
| ecology | births | mutations per allele per birth in the world, morph continuous | 4.000e-3 | 3.954e-3 (7513/1899975) | [3.807e-3, 4.107e-3] | PASS |
| ecology | births | mutated alleles not tagged with the newborn's owner | 0 | 0 | - | PASS |
| ecology | hash-seeded streams | litter-mates with identical gene backgrounds | ≈ 0 | 0 of 559676 pairs | - | info |
| ecology | generations | lineages reaching a mean generation of 20 | ≥ 1 per world | 19 lineages (deepest mean generation 40.7) | - | pass (soft) |
| ecology | heterozygosity | lineages whose heterozygosity fell from the founders' level | most (founders are all distinct; drift outpaces μ) | 24 of 24 | - | pass (soft) |
| ecology | heterozygosity | implied Ne / harmonic-mean census (lineages ≥ 20 generations) | 0.01–2 (typically 0.1–0.5 in real populations) | mean 0.5213 over 19 lineages | - | pass (soft) |
| ecology | divergence | lineages still diverging from their founders (last sample vs first quarter) | most | 22 of 24 | - | pass (soft) |
| ecology | divergence | within a lineage, west and east halves stay one gene pool | most: net divergence < 0.01, fertility > 0.9 | 24 of 24 (net divergence grew in 3) | - | pass (soft) |
| ecology | drift | neutral loci (unexpressed seasonal genes) drifting from the founders | most | 24 of 24 | - | pass (soft) |
| ecology | morphology | lineages carrying new morphology variants, mostly hidden | variants appear by step mutation; carriers outnumber those expressing | 24 lineages carry variants; hidden in 24 | - | pass (soft) |
| ecology | selection | mean shift of expressed genes vs neutral loci (lineages ≥ 10 generations) | both move; selected traits respond | genes 0.0194 vs neutral 7.478e-3 (25 lineages) | - | info |
| ecology | divergence | interfertile lineage pairs without gene flow drift apart | distance grows (fertility falls) | 5 of 5 pairs more distant at the end | - | pass (soft) |
| ecology | divergence | interfertile lineage pairs with gene flow | introgression: distance stays or shrinks | 7 of 7 pairs closer at the end | - | pass (soft) |

### Check details

- **neutral drift: H_t/H_0 at t=50, genes (all loci as markers, no mutation, p0=0.5)** (PASS): E[H_t] = H_0 (1 - 1/2N)^t, N=50, 2000 replicates × 47 loci; H = 2p(1-p) of the population.
- **neutral drift: H_t/H_0 at t=150, genes (all loci as markers, no mutation, p0=0.5)** (PASS): E[H_t] = H_0 (1 - 1/2N)^t, N=50, 2000 replicates × 47 loci; H = 2p(1-p) of the population.
- **neutral drift: Var(p_t) at t=100, genes (p0=0.5)** (PASS): Var(p_t) = p0 q0 [1 - (1 - 1/2N)^t].
- **neutral drift: fixation probability, genes (all loci as markers, no mutation, p0=0.5)** (PASS): A neutral allele fixes with probability equal to its initial frequency. 0 loci still segregating at t=3000 (excluded).
- **neutral drift: mean time to absorption (gens), genes (all loci as markers, no mutation, p0=0.5)** (PASS): Exact Wright–Fisher chain (2N=100); diffusion approximation -4N[p ln p + q ln q] = 138.6 (2.7726N).
- **neutral drift: mean time to fixation given fixation, genes (all loci as markers, no mutation, p0=0.5)** (PASS): Exact Wright–Fisher chain; diffusion approximation -4N q ln q / p = 138.6.
- **neutral drift: H_t/H_0 at t=50, obedience (all loci as markers, no mutation, p0=0.5)** (info): E[H_t] = H_0 (1 - 1/2N)^t, N=50, 2000 replicates × 1 loci; H = 2p(1-p) of the population. Not judged: with every gene a marker and mutation off, the gene background fixes and this segment's hash-seeded stream stops varying (see 'Hash-seeded streams').
- **neutral drift: H_t/H_0 at t=150, obedience (all loci as markers, no mutation, p0=0.5)** (info): E[H_t] = H_0 (1 - 1/2N)^t, N=50, 2000 replicates × 1 loci; H = 2p(1-p) of the population. Not judged: with every gene a marker and mutation off, the gene background fixes and this segment's hash-seeded stream stops varying (see 'Hash-seeded streams').
- **neutral drift: fixation probability, obedience (all loci as markers, no mutation, p0=0.5)** (info): A neutral allele fixes with probability equal to its initial frequency. 8 loci still segregating at t=3000 (excluded).
- **neutral drift: mean time to absorption (gens), obedience (all loci as markers, no mutation, p0=0.5)** (info): Exact Wright–Fisher chain (2N=100); diffusion approximation -4N[p ln p + q ln q] = 138.6 (2.7726N).
- **neutral drift: mean time to fixation given fixation, obedience (all loci as markers, no mutation, p0=0.5)** (info): Exact Wright–Fisher chain; diffusion approximation -4N q ln q / p = 138.6.
- **neutral drift: H_t/H_0 at t=50, load (all loci as markers, no mutation, p0=0.5)** (PASS): E[H_t] = H_0 (1 - 1/2N)^t, N=50, 2000 replicates × 4 loci; H = 2p(1-p) of the population.
- **neutral drift: H_t/H_0 at t=150, load (all loci as markers, no mutation, p0=0.5)** (PASS): E[H_t] = H_0 (1 - 1/2N)^t, N=50, 2000 replicates × 4 loci; H = 2p(1-p) of the population.
- **neutral drift: fixation probability, load (all loci as markers, no mutation, p0=0.5)** (PASS): A neutral allele fixes with probability equal to its initial frequency. 0 loci still segregating at t=3000 (excluded).
- **neutral drift: mean time to absorption (gens), load (all loci as markers, no mutation, p0=0.5)** (PASS): Exact Wright–Fisher chain (2N=100); diffusion approximation -4N[p ln p + q ln q] = 138.6 (2.7726N).
- **neutral drift: mean time to fixation given fixation, load (all loci as markers, no mutation, p0=0.5)** (PASS): Exact Wright–Fisher chain; diffusion approximation -4N q ln q / p = 138.6.
- **neutral drift: H_t/H_0 at t=50, morph discrete (all loci as markers, no mutation, p0=0.5)** (info): E[H_t] = H_0 (1 - 1/2N)^t, N=50, 2000 replicates × 5 loci; H = 2p(1-p) of the population. Not judged: with every gene a marker and mutation off, the gene background fixes and this segment's hash-seeded stream stops varying (see 'Hash-seeded streams').
- **neutral drift: H_t/H_0 at t=150, morph discrete (all loci as markers, no mutation, p0=0.5)** (info): E[H_t] = H_0 (1 - 1/2N)^t, N=50, 2000 replicates × 5 loci; H = 2p(1-p) of the population. Not judged: with every gene a marker and mutation off, the gene background fixes and this segment's hash-seeded stream stops varying (see 'Hash-seeded streams').
- **neutral drift: fixation probability, morph discrete (all loci as markers, no mutation, p0=0.5)** (info): A neutral allele fixes with probability equal to its initial frequency. 63 loci still segregating at t=3000 (excluded).
- **neutral drift: mean time to absorption (gens), morph discrete (all loci as markers, no mutation, p0=0.5)** (info): Exact Wright–Fisher chain (2N=100); diffusion approximation -4N[p ln p + q ln q] = 138.6 (2.7726N).
- **neutral drift: mean time to fixation given fixation, morph discrete (all loci as markers, no mutation, p0=0.5)** (info): Exact Wright–Fisher chain; diffusion approximation -4N q ln q / p = 138.6.
- **neutral drift: H_t/H_0 at t=50, morph continuous (all loci as markers, no mutation, p0=0.5)** (info): E[H_t] = H_0 (1 - 1/2N)^t, N=50, 2000 replicates × 2 loci; H = 2p(1-p) of the population. Not judged: with every gene a marker and mutation off, the gene background fixes and this segment's hash-seeded stream stops varying (see 'Hash-seeded streams').
- **neutral drift: H_t/H_0 at t=150, morph continuous (all loci as markers, no mutation, p0=0.5)** (info): E[H_t] = H_0 (1 - 1/2N)^t, N=50, 2000 replicates × 2 loci; H = 2p(1-p) of the population. Not judged: with every gene a marker and mutation off, the gene background fixes and this segment's hash-seeded stream stops varying (see 'Hash-seeded streams').
- **neutral drift: fixation probability, morph continuous (all loci as markers, no mutation, p0=0.5)** (info): A neutral allele fixes with probability equal to its initial frequency. 20 loci still segregating at t=3000 (excluded).
- **neutral drift: mean time to absorption (gens), morph continuous (all loci as markers, no mutation, p0=0.5)** (info): Exact Wright–Fisher chain (2N=100); diffusion approximation -4N[p ln p + q ln q] = 138.6 (2.7726N).
- **neutral drift: mean time to fixation given fixation, morph continuous (all loci as markers, no mutation, p0=0.5)** (info): Exact Wright–Fisher chain; diffusion approximation -4N q ln q / p = 138.6.
- **neutral drift: H_t/H_0 at t=50, genes (all loci as markers, no mutation, p0=0.2)** (PASS): E[H_t] = H_0 (1 - 1/2N)^t, N=50, 2000 replicates × 47 loci; H = 2p(1-p) of the population.
- **neutral drift: H_t/H_0 at t=150, genes (all loci as markers, no mutation, p0=0.2)** (PASS): E[H_t] = H_0 (1 - 1/2N)^t, N=50, 2000 replicates × 47 loci; H = 2p(1-p) of the population.
- **neutral drift: fixation probability, genes (all loci as markers, no mutation, p0=0.2)** (PASS): A neutral allele fixes with probability equal to its initial frequency. 0 loci still segregating at t=3000 (excluded).
- **neutral drift: mean time to absorption (gens), genes (all loci as markers, no mutation, p0=0.2)** (PASS): Exact Wright–Fisher chain (2N=100); diffusion approximation -4N[p ln p + q ln q] = 100.1 (2.0016N).
- **neutral drift: mean time to fixation given fixation, genes (all loci as markers, no mutation, p0=0.2)** (PASS): Exact Wright–Fisher chain; diffusion approximation -4N q ln q / p = 178.5.
- **neutral drift: fixation probability, obedience (all loci as markers, no mutation, p0=0.2)** (info): A neutral allele fixes with probability equal to its initial frequency. 12 loci still segregating at t=3000 (excluded).
- **neutral drift: mean time to absorption (gens), obedience (all loci as markers, no mutation, p0=0.2)** (info): Exact Wright–Fisher chain (2N=100); diffusion approximation -4N[p ln p + q ln q] = 100.1 (2.0016N).
- **neutral drift: mean time to fixation given fixation, obedience (all loci as markers, no mutation, p0=0.2)** (info): Exact Wright–Fisher chain; diffusion approximation -4N q ln q / p = 178.5.
- **neutral drift: fixation probability, load (all loci as markers, no mutation, p0=0.2)** (PASS): A neutral allele fixes with probability equal to its initial frequency. 0 loci still segregating at t=3000 (excluded).
- **neutral drift: mean time to absorption (gens), load (all loci as markers, no mutation, p0=0.2)** (PASS): Exact Wright–Fisher chain (2N=100); diffusion approximation -4N[p ln p + q ln q] = 100.1 (2.0016N).
- **neutral drift: mean time to fixation given fixation, load (all loci as markers, no mutation, p0=0.2)** (PASS): Exact Wright–Fisher chain; diffusion approximation -4N q ln q / p = 178.5.
- **neutral drift: fixation probability, morph discrete (all loci as markers, no mutation, p0=0.2)** (info): A neutral allele fixes with probability equal to its initial frequency. 41 loci still segregating at t=3000 (excluded).
- **neutral drift: mean time to absorption (gens), morph discrete (all loci as markers, no mutation, p0=0.2)** (info): Exact Wright–Fisher chain (2N=100); diffusion approximation -4N[p ln p + q ln q] = 100.1 (2.0016N).
- **neutral drift: mean time to fixation given fixation, morph discrete (all loci as markers, no mutation, p0=0.2)** (info): Exact Wright–Fisher chain; diffusion approximation -4N q ln q / p = 178.5.
- **neutral drift: fixation probability, morph continuous (all loci as markers, no mutation, p0=0.2)** (info): A neutral allele fixes with probability equal to its initial frequency. 32 loci still segregating at t=3000 (excluded).
- **neutral drift: mean time to absorption (gens), morph continuous (all loci as markers, no mutation, p0=0.2)** (info): Exact Wright–Fisher chain (2N=100); diffusion approximation -4N[p ln p + q ln q] = 100.1 (2.0016N).
- **neutral drift: mean time to fixation given fixation, morph continuous (all loci as markers, no mutation, p0=0.2)** (info): Exact Wright–Fisher chain; diffusion approximation -4N q ln q / p = 178.5.
- **neutral drift: H_t/H_0 at t=50, load (load + morph discrete markers, gene mutation on, p0=0.5)** (PASS): E[H_t] = H_0 (1 - 1/2N)^t, N=50, 2000 replicates × 4 loci; H = 2p(1-p) of the population.
- **neutral drift: H_t/H_0 at t=150, load (load + morph discrete markers, gene mutation on, p0=0.5)** (PASS): E[H_t] = H_0 (1 - 1/2N)^t, N=50, 2000 replicates × 4 loci; H = 2p(1-p) of the population.
- **neutral drift: fixation probability, load (load + morph discrete markers, gene mutation on, p0=0.5)** (PASS): A neutral allele fixes with probability equal to its initial frequency. 0 loci still segregating at t=3000 (excluded).
- **neutral drift: mean time to absorption (gens), load (load + morph discrete markers, gene mutation on, p0=0.5)** (PASS): Exact Wright–Fisher chain (2N=100); diffusion approximation -4N[p ln p + q ln q] = 138.6 (2.7726N).
- **neutral drift: mean time to fixation given fixation, load (load + morph discrete markers, gene mutation on, p0=0.5)** (PASS): Exact Wright–Fisher chain; diffusion approximation -4N q ln q / p = 138.6.
- **neutral drift: H_t/H_0 at t=50, morph discrete (load + morph discrete markers, gene mutation on, p0=0.5)** (PASS): E[H_t] = H_0 (1 - 1/2N)^t, N=50, 2000 replicates × 5 loci; H = 2p(1-p) of the population.
- **neutral drift: H_t/H_0 at t=150, morph discrete (load + morph discrete markers, gene mutation on, p0=0.5)** (PASS): E[H_t] = H_0 (1 - 1/2N)^t, N=50, 2000 replicates × 5 loci; H = 2p(1-p) of the population.
- **neutral drift: fixation probability, morph discrete (load + morph discrete markers, gene mutation on, p0=0.5)** (PASS): A neutral allele fixes with probability equal to its initial frequency. 0 loci still segregating at t=3000 (excluded).
- **neutral drift: mean time to absorption (gens), morph discrete (load + morph discrete markers, gene mutation on, p0=0.5)** (PASS): Exact Wright–Fisher chain (2N=100); diffusion approximation -4N[p ln p + q ln q] = 138.6 (2.7726N).
- **neutral drift: mean time to fixation given fixation, morph discrete (load + morph discrete markers, gene mutation on, p0=0.5)** (PASS): Exact Wright–Fisher chain; diffusion approximation -4N q ln q / p = 138.6.
- **mutation-drift: equilibrium heterozygosity, genes (N=50, founder-varied start)** (PASS): Infinite alleles (every mutation a new value): exact WF recursion F' = (1-μ)²[1/2N + (1-1/2N)F], μ=0.004000000189989805; 4Nμ/(1+4Nμ) = 0.4444. Burn-in 600 gens, 2000 gens sampled every 10, 64 replicates. Tolerance 0.01 for value collisions at the clamp bounds.
- **mutation-drift: equilibrium allele variance, genes (N=50, founder-varied start)** (PASS): Additive neutral locus: V' = (1-1/2N)(V + μσ²) ⇒ V_eq = (2N-1)μσ², σ=0.07000000029802322. Tolerance 5% for clamping at [0, 1].
- **mutation-drift: equilibrium heterozygosity, obedience (N=50, founder-varied start)** (PASS): Infinite alleles (every mutation a new value): exact WF recursion F' = (1-μ)²[1/2N + (1-1/2N)F], μ=0.004000000189989805; 4Nμ/(1+4Nμ) = 0.4444. Burn-in 600 gens, 2000 gens sampled every 10, 64 replicates. Tolerance 0.01 for value collisions at the clamp bounds.
- **mutation-drift: equilibrium allele variance, obedience (N=50, founder-varied start)** (PASS): Additive neutral locus: V' = (1-1/2N)(V + μσ²) ⇒ V_eq = (2N-1)μσ², σ=0.07000000029802322. Tolerance 5% for clamping at [0, 1].
- **mutation-drift: equilibrium heterozygosity, morph continuous (N=50, founder-varied start)** (PASS): Infinite alleles (every mutation a new value): exact WF recursion F' = (1-μ)²[1/2N + (1-1/2N)F], μ=0.004000000189989805; 4Nμ/(1+4Nμ) = 0.4444. Burn-in 600 gens, 2000 gens sampled every 10, 64 replicates. Tolerance 0.01 for value collisions at the clamp bounds.
- **mutation-drift: equilibrium allele variance, morph continuous (N=50, founder-varied start)** (PASS): Additive neutral locus: V' = (1-1/2N)(V + μσ²) ⇒ V_eq = (2N-1)μσ², σ=0.07000000029802322. Tolerance 5% for clamping at [0, 1].
- **mutation-drift: equilibrium heterozygosity, genes (N=50, uniform start)** (PASS): Infinite alleles (every mutation a new value): exact WF recursion F' = (1-μ)²[1/2N + (1-1/2N)F], μ=0.004000000189989805; 4Nμ/(1+4Nμ) = 0.4444. Burn-in 600 gens, 2000 gens sampled every 10, 64 replicates. Tolerance 0.01 for value collisions at the clamp bounds.
- **mutation-drift: equilibrium allele variance, genes (N=50, uniform start)** (PASS): Additive neutral locus: V' = (1-1/2N)(V + μσ²) ⇒ V_eq = (2N-1)μσ², σ=0.07000000029802322. Tolerance 5% for clamping at [0, 1].
- **mutation-drift: equilibrium heterozygosity, obedience (N=50, uniform start)** (PASS): Infinite alleles (every mutation a new value): exact WF recursion F' = (1-μ)²[1/2N + (1-1/2N)F], μ=0.004000000189989805; 4Nμ/(1+4Nμ) = 0.4444. Burn-in 600 gens, 2000 gens sampled every 10, 64 replicates. Tolerance 0.01 for value collisions at the clamp bounds.
- **mutation-drift: equilibrium allele variance, obedience (N=50, uniform start)** (PASS): Additive neutral locus: V' = (1-1/2N)(V + μσ²) ⇒ V_eq = (2N-1)μσ², σ=0.07000000029802322. Tolerance 5% for clamping at [0, 1].
- **mutation-drift: equilibrium heterozygosity, morph continuous (N=50, uniform start)** (PASS): Infinite alleles (every mutation a new value): exact WF recursion F' = (1-μ)²[1/2N + (1-1/2N)F], μ=0.004000000189989805; 4Nμ/(1+4Nμ) = 0.4444. Burn-in 600 gens, 2000 gens sampled every 10, 64 replicates. Tolerance 0.01 for value collisions at the clamp bounds.
- **mutation-drift: equilibrium allele variance, morph continuous (N=50, uniform start)** (PASS): Additive neutral locus: V' = (1-1/2N)(V + μσ²) ⇒ V_eq = (2N-1)μσ², σ=0.07000000029802322. Tolerance 5% for clamping at [0, 1].
- **mutation-drift: equilibrium heterozygosity, genes (N=100, founder-varied start)** (PASS): Infinite alleles (every mutation a new value): exact WF recursion F' = (1-μ)²[1/2N + (1-1/2N)F], μ=0.004000000189989805; 4Nμ/(1+4Nμ) = 0.6154. Burn-in 1200 gens, 2000 gens sampled every 10, 64 replicates. Tolerance 0.01 for value collisions at the clamp bounds.
- **mutation-drift: equilibrium allele variance, genes (N=100, founder-varied start)** (PASS): Additive neutral locus: V' = (1-1/2N)(V + μσ²) ⇒ V_eq = (2N-1)μσ², σ=0.07000000029802322. Tolerance 5% for clamping at [0, 1].
- **mutation-drift: equilibrium heterozygosity, obedience (N=100, founder-varied start)** (PASS): Infinite alleles (every mutation a new value): exact WF recursion F' = (1-μ)²[1/2N + (1-1/2N)F], μ=0.004000000189989805; 4Nμ/(1+4Nμ) = 0.6154. Burn-in 1200 gens, 2000 gens sampled every 10, 64 replicates. Tolerance 0.01 for value collisions at the clamp bounds.
- **mutation-drift: equilibrium allele variance, obedience (N=100, founder-varied start)** (PASS): Additive neutral locus: V' = (1-1/2N)(V + μσ²) ⇒ V_eq = (2N-1)μσ², σ=0.07000000029802322. Tolerance 5% for clamping at [0, 1].
- **mutation-drift: equilibrium heterozygosity, morph continuous (N=100, founder-varied start)** (PASS): Infinite alleles (every mutation a new value): exact WF recursion F' = (1-μ)²[1/2N + (1-1/2N)F], μ=0.004000000189989805; 4Nμ/(1+4Nμ) = 0.6154. Burn-in 1200 gens, 2000 gens sampled every 10, 64 replicates. Tolerance 0.01 for value collisions at the clamp bounds.
- **mutation-drift: equilibrium allele variance, morph continuous (N=100, founder-varied start)** (PASS): Additive neutral locus: V' = (1-1/2N)(V + μσ²) ⇒ V_eq = (2N-1)μσ², σ=0.07000000029802322. Tolerance 5% for clamping at [0, 1].
- **morph step mutation: stationary allele distribution, eye_count** (PASS): Under neutral drift the expected allele frequencies follow the mutation chain, so their long-run mean is its stationary distribution (∝ degree on the path of positions, ends move inward). Step rate raised to 0.05 so the chain mixes; N=50, burn-in 1500, 3000 gens sampled every 10, 64 replicates. Pass: every position within 4.5 SE (across replicates, floored at 0.005; several positions per locus).
- **morph step mutation: stationary allele distribution, ear_count** (PASS): Under neutral drift the expected allele frequencies follow the mutation chain, so their long-run mean is its stationary distribution (∝ degree on the path of positions, ends move inward). Step rate raised to 0.05 so the chain mixes; N=50, burn-in 1500, 3000 gens sampled every 10, 64 replicates. Pass: every position within 4.5 SE (across replicates, floored at 0.005; several positions per locus).
- **morph step mutation: stationary allele distribution, ear_shape** (PASS): Under neutral drift the expected allele frequencies follow the mutation chain, so their long-run mean is its stationary distribution (∝ degree on the path of positions, ends move inward). Step rate raised to 0.05 so the chain mixes; N=50, burn-in 1500, 3000 gens sampled every 10, 64 replicates. Pass: every position within 4.5 SE (across replicates, floored at 0.005; several positions per locus).
- **morph step mutation: stationary allele distribution, leg_count** (PASS): Under neutral drift the expected allele frequencies follow the mutation chain, so their long-run mean is its stationary distribution (∝ degree on the path of positions, ends move inward). Step rate raised to 0.05 so the chain mixes; N=50, burn-in 1500, 3000 gens sampled every 10, 64 replicates. Pass: every position within 4.5 SE (across replicates, floored at 0.005; several positions per locus).
- **morph step mutation: stationary allele distribution, tail_shape** (PASS): Under neutral drift the expected allele frequencies follow the mutation chain, so their long-run mean is its stationary distribution (∝ degree on the path of positions, ends move inward). Step rate raised to 0.05 so the chain mixes; N=50, burn-in 1500, 3000 gens sampled every 10, 64 replicates. Pass: every position within 4.5 SE (across replicates, floored at 0.005; several positions per locus).
- **morph step mutation: impossible morphology values in 14400000 genomes (step rate 0.05)** (PASS): Every discrete allele and expressed value checked every generation: eyes 1–6, ears 0/2/4, legs 0/2/4/6/8, known shapes.
- **morph step mutation: carriers of a non-default allele vs expressing one** (PASS): The default is dominant, so variants hide in carriers.
- **mutation rate: mutations per allele per generation, genes** (PASS): 1000000 children, each of two fresh parents; a mutation is an allele tagged with the child's owner.
- **mutation rate: mutations per allele per generation, obedience** (PASS): 1000000 children, each of two fresh parents; a mutation is an allele tagged with the child's owner.
- **mutation rate: mutations per allele per generation, load** (PASS): 1000000 children, each of two fresh parents; a mutation is an allele tagged with the child's owner.
- **mutation rate: mutations per allele per generation, morph discrete** (PASS): 1000000 children, each of two fresh parents; a mutation is an allele tagged with the child's owner.
- **mutation rate: mutations per allele per generation, morph continuous** (PASS): 1000000 children, each of two fresh parents; a mutation is an allele tagged with the child's owner.
- **mutation rate: mutation step SD, genes** (PASS): Gaussian step N(0, σ²) of a mutated allele, σ=0.07; mean step 2.290e-5 (expected 0).
- **mutation rate: mutation step SD, obedience** (PASS): Gaussian step N(0, σ²) of a mutated allele, σ=0.07; mean step 4.706e-4 (expected 0).
- **mutation rate: mutation step SD, morph continuous** (PASS): Gaussian step N(0, σ²) of a mutated allele, σ=0.07; mean step -6.635e-4 (expected 0).
- **mutation rate: alleles whose origin tag disagrees with mutation status** (PASS): A mutated allele carries the child's owner's tag; an inherited one keeps its parent's tag (every allele of every child checked).
- **mutation rate: step direction from interior values, eye_count (share up)** (PASS): Step rate 0.5, every interior value as the homozygous parental value; 200000 children.
- **mutation rate: step direction from interior values, ear_count (share up)** (PASS): Step rate 0.5, every interior value as the homozygous parental value; 200000 children.
- **mutation rate: step direction from interior values, ear_shape (share up)** (PASS): Step rate 0.5, every interior value as the homozygous parental value; 200000 children.
- **mutation rate: step direction from interior values, leg_count (share up)** (PASS): Step rate 0.5, every interior value as the homozygous parental value; 200000 children.
- **mutation rate: step direction from interior values, tail_shape (share up)** (PASS): Step rate 0.5, every interior value as the homozygous parental value; 200000 children.
- **mutation rate: step mutations that skip a value, leave the range or stay put at an end** (PASS): A discrete mutation moves one step (legs and ears two units) to an adjacent valid value; at an end it moves inward.
- **dominance: F2 share showing eye_count=2 in 2/2 × 4/4 (default vs variant)** (PASS): 3:1 Mendelian ratio; 0 of 200000 F1 did not show the dominant 2, 0 F2 whose phenotype disagreed with their genotype (recessive shows only when homozygous).
- **dominance: F2 share showing leg_count=4 in 4/4 × 6/6 (default vs variant)** (PASS): 3:1 Mendelian ratio; 0 of 200000 F1 did not show the dominant 4, 0 F2 whose phenotype disagreed with their genotype (recessive shows only when homozygous).
- **dominance: F2 share showing ear_count=2 in 2/2 × 0/0 (default vs variant)** (PASS): 3:1 Mendelian ratio; 0 of 200000 F1 did not show the dominant 2, 0 F2 whose phenotype disagreed with their genotype (recessive shows only when homozygous).
- **dominance: F2 share showing tail_shape=0 in 0/0 × 3/3 (default vs variant)** (PASS): 3:1 Mendelian ratio; 0 of 200000 F1 did not show the dominant 0, 0 F2 whose phenotype disagreed with their genotype (recessive shows only when homozygous).
- **dominance: F2 share showing ear_shape=0 in 0/0 × 3/3 (default vs variant)** (PASS): 3:1 Mendelian ratio; 0 of 200000 F1 did not show the dominant 0, 0 F2 whose phenotype disagreed with their genotype (recessive shows only when homozygous).
- **dominance: F2 share showing eye_count=3 in 5/5 × 3/3 (contract rule)** (PASS): 3:1 Mendelian ratio; 0 of 200000 F1 did not show the dominant 3, 0 F2 whose phenotype disagreed with their genotype (recessive shows only when homozygous).
- **dominance: F2 share showing eye_count=1 in 3/3 × 1/1 (contract rule)** (PASS): 3:1 Mendelian ratio; 0 of 200000 F1 did not show the dominant 1, 0 F2 whose phenotype disagreed with their genotype (recessive shows only when homozygous).
- **dominance: F2 share showing leg_count=2 in 6/6 × 2/2 (contract rule)** (PASS): 3:1 Mendelian ratio; 0 of 200000 F1 did not show the dominant 2, 0 F2 whose phenotype disagreed with their genotype (recessive shows only when homozygous).
- **dominance: F2 share showing leg_count=6 in 8/8 × 6/6 (contract rule)** (PASS): 3:1 Mendelian ratio; 0 of 200000 F1 did not show the dominant 6, 0 F2 whose phenotype disagreed with their genotype (recessive shows only when homozygous).
- **dominance: F2 share showing tail_shape=2 in 3/3 × 2/2 (contract rule)** (PASS): 3:1 Mendelian ratio; 0 of 200000 F1 did not show the dominant 2, 0 F2 whose phenotype disagreed with their genotype (recessive shows only when homozygous).
- **dominance: F2 share showing tail_shape=1 in 4/4 × 1/1 (contract rule)** (PASS): 3:1 Mendelian ratio; 0 of 200000 F1 did not show the dominant 1, 0 F2 whose phenotype disagreed with their genotype (recessive shows only when homozygous).
- **dominance: F2 share showing ear_shape=1 in 2/2 × 1/1 (contract rule)** (PASS): 3:1 Mendelian ratio; 0 of 200000 F1 did not show the dominant 1, 0 F2 whose phenotype disagreed with their genotype (recessive shows only when homozygous).
- **dominance: F2 heterozygote share, coloration (h=0.5)** (PASS): 1:2:1 genotypes (24961:50083:24956); heterozygote expression 0.5 + h·0.3 = 0.6500, max error 2.38e-8.
- **dominance: F2 mean expressed value, coloration (h=0.5)** (PASS): expressed = mean + h|a-b|/2: F2 mean = 0.5 + 0.15h.
- **dominance: F2 heterozygote share, fur (h=0.5)** (PASS): 1:2:1 genotypes (25109:50157:24734); heterozygote expression 0.5 + h·0.3 = 0.6500, max error 2.38e-8.
- **dominance: F2 mean expressed value, fur (h=0.5)** (PASS): expressed = mean + h|a-b|/2: F2 mean = 0.5 + 0.15h.
- **dominance: F2 heterozygote share, camouflage (h=-0.5)** (PASS): 1:2:1 genotypes (24845:50131:25024); heterozygote expression 0.5 + h·0.3 = 0.3500, max error 5.96e-9.
- **dominance: F2 mean expressed value, camouflage (h=-0.5)** (PASS): expressed = mean + h|a-b|/2: F2 mean = 0.5 + 0.15h.
- **dominance: F2 heterozygote share, body size (h=0)** (PASS): 1:2:1 genotypes (25115:49872:25013); heterozygote expression 0.5 + h·0.3 = 0.5000, max error 0.00e0.
- **dominance: F2 mean expressed value, body size (h=0)** (PASS): expressed = mean + h|a-b|/2: F2 mean = 0.5 + 0.15h.
- **additive inheritance: offspring − midparent, genes (additive loci)** (PASS): 40000 families × 4 children from founder pools (allele SD 0.15), mutation off.
- **additive inheritance: segregation variance / Mendelian expectation, genes** (PASS): Var(child − midparent) = [(m1−m2)² + (f1−f2)²]/16 for parents (m1,m2), (f1,f2).
- **additive inheritance: within-family variance / (V_A/2), genes** (PASS): In a random-mating (Hardy–Weinberg) population full sibs vary by half the additive variance.
- **additive inheritance: offspring − midparent, obedience (additive loci)** (PASS): 40000 families × 4 children from founder pools (allele SD 0.15), mutation off.
- **additive inheritance: segregation variance / Mendelian expectation, obedience** (PASS): Var(child − midparent) = [(m1−m2)² + (f1−f2)²]/16 for parents (m1,m2), (f1,f2).
- **additive inheritance: within-family variance / (V_A/2), obedience** (PASS): In a random-mating (Hardy–Weinberg) population full sibs vary by half the additive variance.
- **additive inheritance: offspring − midparent, morph continuous (additive loci)** (PASS): 40000 families × 4 children from founder pools (allele SD 0.15), mutation off.
- **additive inheritance: segregation variance / Mendelian expectation, morph continuous** (PASS): Var(child − midparent) = [(m1−m2)² + (f1−f2)²]/16 for parents (m1,m2), (f1,f2).
- **additive inheritance: within-family variance / (V_A/2), morph continuous** (PASS): In a random-mating (Hardy–Weinberg) population full sibs vary by half the additive variance.
- **recessive load: harmful-allele frequency at mutation–selection balance (s=1, N=2000)** (PASS): Deterministic equilibrium of q → q+μ(1−q) → selection 1−s on homozygotes (μ=0.0020000000949949026); sqrt(μ/s) = 0.0447. Mean over gens 150–400, 40 replicates. Tolerance 10% for drift in a finite population (rare recessives are lost by drift, so a finite population sits a little below the deterministic value).
- **recessive load: harmful-allele frequency at mutation–selection balance (s=0.3, N=2000)** (PASS): Deterministic equilibrium of q → q+μ(1−q) → selection 1−s on homozygotes (μ=0.0020000000949949026); sqrt(μ/s) = 0.0816. Mean over gens 150–400, 40 replicates. Tolerance 10% for drift in a finite population (rare recessives are lost by drift, so a finite population sits a little below the deterministic value).
- **recessive load: game's implied selection and balance** (info): The world kills a homozygote with a daily hazard recessive_hazard=0.08 per affected locus, so s = 1 − exp(−hazard × days to breeding); 30 days gives s=0.9093, and the balance q̂ above.
- **recessive load: affected (homozygous) loci per outcrossed offspring** (PASS): Σ q_g² over the 4 load loci (gamete frequency q_g = q + μ(1−q)); 20000 offspring per population × 40 s=1 populations at balance; Poisson SE of the pooled count.
- **recessive load: affected (homozygous) loci per full-sib-mated offspring** (PASS): Per locus ¼q (identical by descent) + ¼·P(a grandparent's two alleles both harmful) + ½g² (g = q + 2μ(1−q) after two meioses). The textbook q² + Fq(1−q) with F = ¼ assumes Hardy–Weinberg grandparents; under selection against homozygotes (s=1) none of them is homozygous, so the middle term is ≈ 0 instead of ¼q². Poisson SE.
- **recessive load: inbreeding raises homozygote deaths (sib / outcross)** (PASS): Full-sib mating exposes hidden recessives; pass when the sib rate exceeds the outcross rate beyond both CIs.
- **legacy tags: owner's allele share at t=25 (start 0.3)** (PASS): Neutral drift is a martingale: the expected share is preserved. Legacy tally with the morphology alleles (count_origins_with(.., true)); children belong to the mother's owner and mutations are tagged with it; N=50, 1000 replicates, default mutation rates.
- **legacy tags: owner's allele share at t=50 (start 0.3)** (PASS): Neutral drift is a martingale: the expected share is preserved. Legacy tally with the morphology alleles (count_origins_with(.., true)); children belong to the mother's owner and mutations are tagged with it; N=50, 1000 replicates, default mutation rates.
- **legacy tags: owner's allele share at t=100 (start 0.3)** (PASS): Neutral drift is a martingale: the expected share is preserved. Legacy tally with the morphology alleles (count_origins_with(.., true)); children belong to the mother's owner and mutations are tagged with it; N=50, 1000 replicates, default mutation rates.
- **legacy tags: owner's allele share at t=200 (start 0.3)** (PASS): Neutral drift is a martingale: the expected share is preserved. Legacy tally with the morphology alleles (count_origins_with(.., true)); children belong to the mother's owner and mutations are tagged with it; N=50, 1000 replicates, default mutation rates.
- **legacy tags: owner's share of individuals at t=200 (start 0.3)** (PASS): Ownership follows the maternal line: also a martingale.
- **legacy tags: designer's share after t=25 gens of mutation tagged to the owner** (PASS): E[share] = Σ_segments w (1−μ)^t: genes and continuous morph μ=0.004000000189989805, load (functional → harmful) 0.0020000000949949026, discrete morph 0.0005000000237487257. Drift leaves the expectation alone.
- **legacy tags: designer's share after t=50 gens of mutation tagged to the owner** (PASS): E[share] = Σ_segments w (1−μ)^t: genes and continuous morph μ=0.004000000189989805, load (functional → harmful) 0.0020000000949949026, discrete morph 0.0005000000237487257. Drift leaves the expectation alone.
- **legacy tags: designer's share after t=100 gens of mutation tagged to the owner** (PASS): E[share] = Σ_segments w (1−μ)^t: genes and continuous morph μ=0.004000000189989805, load (functional → harmful) 0.0020000000949949026, discrete morph 0.0005000000237487257. Drift leaves the expectation alone.
- **legacy tags: designer's share after t=200 gens of mutation tagged to the owner** (PASS): E[share] = Σ_segments w (1−μ)^t: genes and continuous morph μ=0.004000000189989805, load (functional → harmful) 0.0020000000949949026, discrete morph 0.0005000000237487257. Drift leaves the expectation alone.
- **linkage: transmission ratio at each of 59 loci (Mendel's first law)** (PASS): 400000 offspring of a phase-known heterozygote; Bonferroni limit at two-sided 0.001.
- **linkage: parental-strand agreement for all 1711 locus pairs (independent assortment)** (PASS): Share of offspring that took the same maternal strand at both loci; 0.5 means a recombination fraction of 1/2 (no linkage), including across the hash-seeded segments.
- **linkage: Σ z² over all locus pairs (global independence)** (PASS): Chi-square with 1711 degrees of freedom (pairwise strand agreements are uncorrelated under independence): mean 1711, SD √(2·1711).
- **hash-seeded streams: eye 2/4 × 2/4 with identical homozygous gene backgrounds (mutation off)** (info): 25000 children of two genomes with identical, fully homozygous gene loci (Genome::uniform). The morphology stream is seeded from the child's gene alleles and the parents' morphology alleles, so without gene variation every child draws the same segregation; with default mutation only the children that carry a new gene mutation draw afresh. Founders vary (Genome::founder), and the mutation rate keeps every real lineage polymorphic, so this never arises in a world (see the ecology level's sibling-background measurement).
- **hash-seeded streams: eye 2/4 × 2/4 with identical homozygous gene backgrounds (default mutation)** (info): 25000 children of two genomes with identical, fully homozygous gene loci (Genome::uniform). The morphology stream is seeded from the child's gene alleles and the parents' morphology alleles, so without gene variation every child draws the same segregation; with default mutation only the children that carry a new gene mutation draw afresh. Founders vary (Genome::founder), and the mutation rate keeps every real lineage polymorphic, so this never arises in a world (see the ecology level's sibling-background measurement).
- **morphology: impossible morphology values (every living organism every sample, every newborn)** (PASS): Odd legs, ears not in {0, 2, 4}, eyes outside 1–6, unknown shapes.
- **births: mutations per allele per birth in the world, genes** (PASS): 474995 newborns compared with both parents' genomes on their day of birth (alleles whose mutation would be invisible, e.g. a parent at a clamp bound or heterozygous at a discrete locus, are skipped).
- **births: mutations per allele per birth in the world, obedience** (PASS): 474995 newborns compared with both parents' genomes on their day of birth (alleles whose mutation would be invisible, e.g. a parent at a clamp bound or heterozygous at a discrete locus, are skipped).
- **births: mutations per allele per birth in the world, load** (PASS): 474995 newborns compared with both parents' genomes on their day of birth (alleles whose mutation would be invisible, e.g. a parent at a clamp bound or heterozygous at a discrete locus, are skipped).
- **births: mutations per allele per birth in the world, morph discrete** (PASS): 474995 newborns compared with both parents' genomes on their day of birth (alleles whose mutation would be invisible, e.g. a parent at a clamp bound or heterozygous at a discrete locus, are skipped).
- **births: mutations per allele per birth in the world, morph continuous** (PASS): 474995 newborns compared with both parents' genomes on their day of birth (alleles whose mutation would be invisible, e.g. a parent at a clamp bound or heterozygous at a discrete locus, are skipped).
- **births: mutated alleles not tagged with the newborn's owner** (PASS): Every visible mutation in a newborn carries its owner's ID (the mother's owner).
- **hash-seeded streams: litter-mates with identical gene backgrounds** (info): Litter-mates sharing every gene allele would also share their obedience and morphology draws (see the genome level's hash-seeded streams row). Measured on every litter born.
- **generations: lineages reaching a mean generation of 20** (pass (soft)): 4 worlds of 96×96, 15 years each.
- **heterozygosity: lineages whose heterozygosity fell from the founders' level** (pass (soft)): Infinite-alleles heterozygosity (allele identity by exact value) over the distance genes.
- **heterozygosity: implied Ne / harmonic-mean census (lineages ≥ 20 generations)** (pass (soft)): Ne from H = 4Neμ/(1+4Neμ) (μ=0.004000000189989805); assumes mutation–drift balance, which selection, sweeps and founder effects bend. For scale: H_eq(Ne=50) = 0.4444, H_eq(Ne=500) = 0.8889.
- **divergence: lineages still diverging from their founders (last sample vs first quarter)** (pass (soft)): Mean |lineage mean − founders' mean| over the distance genes: drift and selection move it away; it keeps growing.
- **divergence: within a lineage, west and east halves stay one gene pool** (pass (soft)): Net divergence = mean distance between members in different halves of the map − mean within halves (last quarter of samples). On a map this size members range across it in a few generations: gene flow (4Nm ≫ 1) keeps F_ST near 0, so the halves do not diverge; the founding placement's initial difference decays instead. Isolation by distance would need a map many home ranges wide.
- **drift: neutral loci (unexpressed seasonal genes) drifting from the founders** (pass (soft)): Seasonal genes far below the strategy threshold do nothing and are outside the distance: pure mutation and drift (reflected at 0).
- **morphology: lineages carrying new morphology variants, mostly hidden** (pass (soft)): Step rate 0.0005 per allele; the default is dominant.
- **selection: mean shift of expressed genes vs neutral loci (lineages ≥ 10 generations)** (info): Neutral loci start at 0 (clamped), so mutation reflected at the bound pushes their mean up: their shift is drift plus the boundary. The genes' shift includes selection.
- **divergence: interfertile lineage pairs without gene flow drift apart** (pass (soft)): Pairs of lineages with non-zero fertility at founding and fewer than 10 hybrid offspring: separate gene pools diverge by drift and selection.
- **divergence: interfertile lineage pairs with gene flow** (pass (soft)): Pairs with at least 10 hybrid offspring: hybrids carry alleles both ways, so the two gene pools converge instead of diverging (migration–drift: F_ST ≈ 1/(1 + 4Nm)).

### Neutral drift: heterozygosity decay

```text
Mean heterozygosity 2p(1-p) of the gene markers vs generation (N=50, p0=0.5, 2000 replicates)
   0.5000 |*                                                  
          | **                                                
          |   **                                              
          |     **                                            
          |       **                                          
          |         ***                                       
          |            ***                                    
          |               ***                                 
          |                  ***                              
          |                     ****                          
          |                         *****                     
          |                              *******              
          |                                     ********      
   0.0670 |                                             ******
          +---------------------------------------------------
           0 … 200.0
           . expected H0(1-1/2N)^t, o observed ('*' overlap)
```


### Morphology step mutation: stationary allele distribution

| Locus | Position (value) | Expected | Observed ± SE |
|---|---|---|---|
| eye_count | 0 (1) | 0.1000 | 0.0959 ± 3.095e-3 |
| eye_count | 1 (2) | 0.2000 | 0.1939 ± 4.785e-3 |
| eye_count | 2 (3) | 0.2000 | 0.1990 ± 2.352e-3 |
| eye_count | 3 (4) | 0.2000 | 0.2032 ± 2.362e-3 |
| eye_count | 4 (5) | 0.2000 | 0.2043 ± 4.622e-3 |
| eye_count | 5 (6) | 0.1000 | 0.1037 ± 3.165e-3 |
| ear_count | 0 (0) | 0.2500 | 0.2482 ± 1.720e-3 |
| ear_count | 1 (2) | 0.5000 | 0.5016 ± 1.132e-3 |
| ear_count | 2 (4) | 0.2500 | 0.2502 ± 1.697e-3 |
| ear_shape | 0 (0) | 0.1667 | 0.1649 ± 2.228e-3 |
| ear_shape | 1 (1) | 0.3333 | 0.3339 ± 2.186e-3 |
| ear_shape | 2 (2) | 0.3333 | 0.3340 ± 2.440e-3 |
| ear_shape | 3 (3) | 0.1667 | 0.1672 ± 2.108e-3 |
| leg_count | 0 (0) | 0.1250 | 0.1248 ± 2.679e-3 |
| leg_count | 1 (2) | 0.2500 | 0.2501 ± 3.615e-3 |
| leg_count | 2 (4) | 0.2500 | 0.2521 ± 1.663e-3 |
| leg_count | 3 (6) | 0.2500 | 0.2494 ± 3.624e-3 |
| leg_count | 4 (8) | 0.1250 | 0.1237 ± 2.642e-3 |
| tail_shape | 0 (0) | 0.1250 | 0.1219 ± 2.309e-3 |
| tail_shape | 1 (1) | 0.2500 | 0.2456 ± 3.270e-3 |
| tail_shape | 2 (4) | 0.2500 | 0.2491 ± 1.781e-3 |
| tail_shape | 3 (2) | 0.2500 | 0.2543 ± 3.108e-3 |
| tail_shape | 4 (3) | 0.1250 | 0.1291 ± 2.264e-3 |


### Morphology step-mutation spectrum (rate 0.5)

| Locus | From | Down | Stay | Up | Up share (interior) |
|---|---|---|---|---|---|
| eye_count | 1 | 0 | 33387 | 33301 | - |
| eye_count | 2 | 16647 | 33385 | 16656 | 0.5001 |
| eye_count | 3 | 16721 | 33414 | 16553 | 0.4975 |
| eye_count | 4 | 16631 | 33233 | 16824 | 0.5029 |
| eye_count | 5 | 16548 | 33434 | 16706 | 0.5024 |
| eye_count | 6 | 33257 | 33303 | 0 | - |
| ear_count | 0 | 0 | 66788 | 66588 | - |
| ear_count | 2 | 33343 | 66880 | 33153 | 0.4986 |
| ear_count | 4 | 66534 | 66714 | 0 | - |
| ear_shape | 0 | 0 | 50167 | 49929 | - |
| ear_shape | 1 | 24982 | 49926 | 25060 | 0.5008 |
| ear_shape | 2 | 25037 | 49856 | 25075 | 0.5004 |
| ear_shape | 3 | 49699 | 50269 | 0 | - |
| leg_count | 0 | 0 | 40239 | 39761 | - |
| leg_count | 2 | 20315 | 39795 | 19890 | 0.4947 |
| leg_count | 4 | 19967 | 40149 | 19884 | 0.4990 |
| leg_count | 6 | 19941 | 40164 | 19895 | 0.4994 |
| leg_count | 8 | 39855 | 40145 | 0 | - |
| tail_shape | 0 | 0 | 39834 | 40166 | - |
| tail_shape | 1 | 20167 | 39992 | 19841 | 0.4959 |
| tail_shape | 4 | 20003 | 39900 | 20097 | 0.5012 |
| tail_shape | 2 | 20194 | 39858 | 19948 | 0.4969 |
| tail_shape | 3 | 40048 | 39952 | 0 | - |


### Recessive load: approach to mutation–selection balance

```text
Mean harmful-allele frequency vs generation (s=1, N=2000, μ=0.0020000000949949026)
   0.0440 |.............*****.***.*.*******************..*****.*.********************.***.*.............*******
          |        ooooo     o   o o                   oo     o o                    o   o ooooooooooooo       
          |      oo                                                                                            
          |     o                                                                                              
          |    o                                                                                               
          |                                                                                                    
          |   o                                                                                                
          |  o                                                                                                 
          |                                                                                                    
          | o                                                                                                  
          |                                                                                                    
 1.931e-3 |o                                                                                                   
          +----------------------------------------------------------------------------------------------------
           0 … 396.0
           . deterministic equilibrium, o observed ('*' overlap)
```


### Linkage by segment

| Segment pair | Pairs | Mean abs(r − 0.5) |
|---|---|---|
| genes × genes | 1081 | 6.495e-4 |
| genes × obedience | 47 | 7.246e-4 |
| genes × load | 188 | 5.801e-4 |
| genes × morph discrete | 235 | 6.408e-4 |
| genes × morph continuous | 94 | 6.718e-4 |
| obedience × load | 4 | 8.519e-4 |
| obedience × morph discrete | 5 | 8.465e-4 |
| obedience × morph continuous | 2 | 2.213e-4 |
| load × load | 6 | 6.129e-4 |
| load × morph discrete | 20 | 6.991e-4 |
| load × morph continuous | 8 | 4.725e-4 |
| morph discrete × morph discrete | 10 | 4.880e-4 |
| morph discrete × morph continuous | 10 | 7.648e-4 |
| morph continuous × morph continuous | 1 | 4.075e-4 |

Expected mean |r − 0.5| under independence: 6.308e-4 (n = 400000).


### Ecology: lineages at the end of each world

| World | Lineage | Alive | Mean gen. (max) | H founders → end | H neutral end | Implied Ne (H) | Harm. mean N | Drift genes / neutral | Within dist. | E–W net div. start → end | E–W fertility end | Morph variant alleles / expressing | Harmful / org |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 0 | Meadow vole | 1069 | 40.7 (52) | 0.8803 → 0.7549 | 0.7075 | 192.5 | 833.9 | 0.0178 / 9.709e-3 | 0.0315 | 6.504e-4 → 8.984e-4 | 0.9954 | 0.0132 / 0.0112 | 0.2021 |
| 0 | Steppe antelope | 475 | 20.1 (26) | 0.8763 → 0.8384 | 0.6843 | 324.3 | 327.5 | 0.0224 / 5.348e-3 | 0.0409 | 1.870e-3 → 5.686e-4 | 0.9770 | 6.526e-3 / 0 | 0.1853 |
| 0 | Marsh newt | 788 | 23.0 (29) | 0.8652 → 0.7413 | 0.6026 | 179.1 | 560.3 | 0.0178 / 6.360e-3 | 0.0320 | 6.176e-4 → 1.417e-3 | 0.9525 | 3.299e-3 / 0 | 0.2690 |
| 0 | Red fox | extinct by day 310 | | | | | | | | | | | |
| 0 | Pack hunters | extinct by day 230 | | | | | | | | | | | |
| 0 | Herd grazers | 692 | 20.6 (26) | 0.8910 → 0.8384 | 0.6816 | 324.3 | 554.7 | 0.0287 / 5.332e-3 | 0.0425 | 2.239e-3 → 7.524e-4 | 0.9926 | 5.925e-3 / 0 | 0.1662 |
| 0 | Toxic frogs | 843 | 21.8 (31) | 0.8810 → 0.8153 | 0.7971 | 275.9 | 671.3 | 0.0110 / 8.694e-3 | 0.0286 | 1.134e-3 → 3.809e-4 | 0.9470 | 5.338e-3 / 2.372e-3 | 0.1542 |
| 0 | Burrowing omnivores | 229 | 27.2 (31) | 0.8716 → 0.7608 | 0.5874 | 198.8 | 267.2 | 0.0157 / 6.314e-3 | 0.0295 | 2.586e-3 → 2.498e-3 | 0.9827 | 5.677e-3 / 0 | 0.1092 |
| 1 | Meadow vole | 874 | 36.8 (42) | 0.8813 → 0.7735 | 0.7440 | 213.4 | 714.7 | 0.0175 / 0.0139 | 0.0298 | 7.767e-4 → 1.908e-3 | 0.9999 | 0.0168 / 2.288e-3 | 0.2860 |
| 1 | Steppe antelope | 551 | 18.0 (24) | 0.8754 → 0.8738 | 0.7370 | 432.9 | 407.5 | 0.0245 / 3.463e-3 | 0.0437 | 2.383e-3 → 6.203e-4 | 0.9826 | 3.267e-3 / 0 | 0.1416 |
| 1 | Marsh newt | 791 | 25.5 (30) | 0.8747 → 0.7927 | 0.6794 | 239.0 | 542.1 | 0.0146 / 0.0131 | 0.0292 | 1.867e-3 → 2.940e-4 | 0.9727 | 0.0131 / 0 | 0.1719 |
| 1 | Red fox | extinct by day 320 | | | | | | | | | | | |
| 1 | Pack hunters | extinct by day 520 | | | | | | | | | | | |
| 1 | Herd grazers | 843 | 17.9 (23) | 0.8956 → 0.8778 | 0.7533 | 449.1 | 582.7 | 0.0233 / 2.533e-3 | 0.0422 | 4.809e-3 → -6.323e-4 | 0.9843 | 8.066e-3 / 0 | 0.1613 |
| 1 | Toxic frogs | 841 | 21.0 (27) | 0.8805 → 0.7943 | 0.7288 | 241.3 | 709.5 | 0.0132 / 5.427e-3 | 0.0309 | 4.799e-3 → 2.453e-4 | 0.9875 | 0.0151 / 2.378e-3 | 0.2723 |
| 1 | Burrowing omnivores | 96 | 27.1 (35) | 0.8771 → 0.6882 | 0.5860 | 137.9 | 220.4 | 0.0217 / 8.364e-3 | 0.0286 | -4.115e-4 → 6.451e-3 | 0.9855 | 0.0156 / 0 | 0.0833 |
| 2 | Meadow vole | 1064 | 40.1 (47) | 0.8769 → 0.7528 | 0.7146 | 190.3 | 621.5 | 0.0169 / 4.542e-3 | 0.0302 | 1.006e-3 → 1.010e-3 | 0.9991 | 0.0101 / 5.639e-3 | 0.2566 |
| 2 | Steppe antelope | 551 | 19.4 (25) | 0.8752 → 0.8508 | 0.6851 | 356.4 | 359.9 | 0.0233 / 2.608e-3 | 0.0418 | 2.762e-3 → 1.296e-4 | 0.9951 | 4.537e-3 / 0 | 0.2686 |
| 2 | Marsh newt | 791 | 22.8 (30) | 0.8817 → 0.6438 | 0.4887 | 113.0 | 239.6 | 0.0203 / 0.0142 | 0.0262 | 1.145e-3 → 8.518e-4 | 1.0000 | 0.0269 / 8.850e-3 | 0.1327 |
| 2 | Red fox | extinct by day 340 | | | | | | | | | | | |
| 2 | Pack hunters | extinct by day 290 | | | | | | | | | | | |
| 2 | Herd grazers | 759 | 19.0 (24) | 0.8976 → 0.8556 | 0.7060 | 370.4 | 549.8 | 0.0298 / 2.793e-3 | 0.0399 | 4.494e-3 → 9.373e-4 | 0.9958 | 7.773e-3 / 0 | 0.2556 |
| 2 | Toxic frogs | 842 | 20.3 (26) | 0.8706 → 0.7853 | 0.6575 | 228.6 | 606.3 | 0.0141 / 5.518e-3 | 0.0299 | 3.029e-3 → 2.509e-4 | 0.9931 | 5.226e-3 / 0 | 0.1021 |
| 2 | Burrowing omnivores | 202 | 25.7 (30) | 0.8732 → 0.6747 | 0.6342 | 129.6 | 228.1 | 0.0188 / 6.714e-3 | 0.0280 | 5.173e-3 → 1.009e-3 | 0.9991 | 4.455e-3 / 0 | 0.1634 |
| 3 | Meadow vole | 902 | 35.4 (47) | 0.8788 → 0.7780 | 0.6975 | 219.0 | 672.7 | 0.0163 / 0.0162 | 0.0337 | 8.208e-4 → 1.869e-3 | 0.9739 | 0.0103 / 1.109e-3 | 0.2616 |
| 3 | Steppe antelope | 437 | 23.1 (28) | 0.8747 → 0.8565 | 0.7699 | 373.1 | 338.4 | 0.0283 / 7.207e-3 | 0.0420 | 3.074e-3 → -9.005e-4 | 0.9932 | 0.0133 / 4.577e-3 | 0.2151 |
| 3 | Marsh newt | 788 | 20.7 (26) | 0.8728 → 0.7899 | 0.6837 | 235.0 | 605.6 | 0.0154 / 0.0102 | 0.0294 | 6.474e-4 → 1.317e-3 | 0.9853 | 2.030e-3 / 0 | 0.1904 |
| 3 | Red fox | extinct by day 200 | | | | | | | | | | | |
| 3 | Pack hunters | extinct by day 230 | | | | | | | | | | | |
| 3 | Herd grazers | 688 | 21.5 (25) | 0.8857 → 0.8609 | 0.7555 | 386.8 | 494.9 | 0.0239 / 4.175e-3 | 0.0427 | 3.362e-3 → -2.484e-4 | 0.9892 | 0.0142 / 0.0102 | 0.1977 |
| 3 | Toxic frogs | 841 | 19.0 (27) | 0.8798 → 0.8104 | 0.6674 | 267.1 | 683.5 | 0.0120 / 2.715e-3 | 0.0307 | 3.674e-3 → 1.015e-3 | 0.9997 | 2.735e-3 / 0 | 0.1463 |
| 3 | Burrowing omnivores | 652 | 22.8 (29) | 0.8789 → 0.7385 | 0.7343 | 176.5 | 295.1 | 0.0213 / 0.0126 | 0.0372 | 3.172e-3 → 9.310e-4 | 0.9786 | 0.0681 / 0.0798 | 0.0813 |


### Ecology: lineage pairs that could hybridize at founding

| World | Pair | Hybrids born | Distance start → end | Fertility start → end |
|---|---|---|---|---|
| 0 | Steppe antelope × Herd grazers | 9711 | 0.0634 → 0.0415 | 0.9738 → 0.9900 |
| 0 | Marsh newt × Toxic frogs | 3 | 0.1357 → 0.1385 | 0.7466 → 0.6996 |
| 0 | Red fox × Pack hunters | 25 | 0.1087 → 0.0876 | 0.8560 → 0.9229 |
| 1 | Steppe antelope × Herd grazers | 8595 | 0.0626 → 0.0419 | 0.9671 → 0.9793 |
| 1 | Marsh newt × Toxic frogs | 0 | 0.1367 → 0.1441 | 0.7411 → 0.6338 |
| 1 | Red fox × Pack hunters | 9 | 0.1071 → 0.1141 | 0.8521 → 0.8272 |
| 2 | Steppe antelope × Herd grazers | 9203 | 0.0632 → 0.0403 | 0.9758 → 0.9987 |
| 2 | Marsh newt × Toxic frogs | 0 | 0.1355 → 0.1412 | 0.7478 → 0.7217 |
| 2 | Red fox × Pack hunters | 142 | 0.1121 → 0.0366 | 0.8403 → 1.0000 |
| 3 | Steppe antelope × Herd grazers | 10176 | 0.0641 → 0.0437 | 0.9813 → 0.9972 |
| 3 | Marsh newt × Toxic frogs | 0 | 0.1343 → 0.1437 | 0.7532 → 0.7012 |
| 3 | Red fox × Pack hunters | 25 | 0.1088 → 0.0475 | 0.8252 → 1.0000 |


### Ecology: runtimes

- world 0: 789 s, 117393 newborns inspected
- world 1: 765 s, 111564 newborns inspected
- world 2: 748 s, 126379 newborns inspected
- world 3: 724 s, 119659 newborns inspected

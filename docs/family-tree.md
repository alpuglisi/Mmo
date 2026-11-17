# Family tree

Nov 17, 2026 · the player-facing view that makes a lineage's evolution
visible (the **Family** tab). Screenshots: `docs/screenshots/family-tree/`.

## What it shows

- **The trunk**: the lineage from its founding to today, as a column of stage
  cards joined by a line. Each card has a portrait drawn from the creature
  art library (the founder avatar, `world/avatar.ts`) of the *average animal*
  of that day: its real covering, colours, body plan, horns and coat pattern.
  Under it are up to three labels naming what changed since the stage above,
  in the players' trait names (`traitName`): "Ear size ↑", "Stripes
  appeared", "Antlers appeared", "Six legs", "Became cold-blooded".
- **Stages kept**: the founding, today (or "Last seen" when the lineage died
  out), and up to four days in between on which the look changed visibly (a
  gene drift of 0.08, an axis change of 0.15, eye placement 20°, any new or
  lost structure or shape), the largest changes first (`checkpoints`).
- **Branches**: the species the lineage's members split into (`origin`
  split), hanging off the stage they split from; a split of a split is
  indented under its parent. Each shows its portrait, its changes against
  the stage (or parent species) it split from, and its status: adults (and
  how many are yours), extinct on day N, or merged back.
- **Extinct** branches and a dead lineage are greyed, dashed and badged.
- One column on a phone; on a wide panel branches sit beside their stage.

## Data (`GET /api/lineage/tree`, server `game::family`)

- **Timeline**: every daily record that kept the lineage's gene means, plus
  today's living members. Daily records now also keep each player lineage's
  typical body plan (`LineageEntry::morph`, `BodyPlanMode`: counts and shapes
  by majority, placement, snout, horn size and wing shape by mean), stored,
  thinned and stripped with the gene means. It is optional and absent in
  older records, so stored history reads as before (those days draw the
  default body plan).
- **Species**: every species whose last detection counted the lineage, and
  their ancestors. Genes are the species representatives' means and counts
  and shapes their majority (`Species::reps`, a new read-only accessor; the
  snapshot format is unchanged). The tracker keeps nothing else, so
  placement, snout, horns, wing shape and pattern come from the lineage's
  sample nearest the species' last sighting.
- The endpoint is new; the WebSocket protocol and its wire fixture are
  unchanged. `client/src/familyTree.fixture.json` (written by
  `game::family::tests::the_client_family_fixture_is_current`,
  `UPDATE_FAMILY_FIXTURE=1`) keeps the client's model in step with the
  server's shape.

## Client

- `client/src/familyTree.ts`: the pure model (checkpoints, change labels,
  branches, extinct marks), unit-tested on a made-up tree
  (`ui/familyDemo.ts`) and the server fixture.
- `client/src/ui/familyTree.ts` + `.css`: the view. Dev harness:
  `/?builder=family`.
- The phone bottom bar now holds eight tabs: they share the width equally
  and four use short labels there (Animal, Mates, Genes, Ranks; the full
  name stays the accessible name and shows on desktop).
- Tests: `familyTree.test.ts`; e2e `e2e/family.mjs` (harness on a desktop,
  iPhone 13 and iPhone SE: portraits, labels, branches, extinct marking, no
  sideways scroll) and a step in `e2e/smoke.mjs` (the real Family tab after
  founding a three-eyed, six-legged lineage).

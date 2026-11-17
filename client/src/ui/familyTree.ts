// The family tree view (the Family tab): the lineage's stages from its
// founding to today as a column of cards joined by a line, each with its
// portrait (the founder avatar from the art library, `world/avatar.ts`,
// wearing the stage's real covering and colours) and the key changes since
// the stage before; the species its members split into hang off the stage
// they split from, extinct ones greyed and marked. One column on a phone;
// on a wider panel the branches sit beside their stage.
//
// The model is `familyTree.ts` (pure, tested); this only draws it.

import './familyTree.css';
import { h } from './dom';
import { avatarSvg, describeAvatar, morphologyOf } from '../world/avatar';
import { describeFamily, type FamilyBranch, type FamilyModel, type FamilyNode } from '../familyTree';

let portraits = 0;

/** One stage's card: portrait, title, day line and change chips. */
export function nodeCard(n: FamilyNode, opts: { compact?: boolean } = {}): HTMLElement {
  const m = morphologyOf(n.design, n.body);
  const fig = h('div', { class: 'ft-portrait', role: 'img', 'aria-label': describeAvatar(m) });
  fig.innerHTML = avatarSvg(m, { animate: false, id: `ft${++portraits}` });
  const chips = n.changes.length
    ? h('ul', { class: 'ft-changes', 'aria-label': 'Changes' }, ...n.changes.map((c) => h('li', { class: 'ft-chip' }, c)))
    : n.kind === 'founding'
      ? h('p', { class: 'ft-note muted small' }, 'As designed')
      : h('p', { class: 'ft-note muted small' }, 'No visible change');
  const badge = n.extinct ? h('span', { class: 'ft-badge', title: 'Extinct' }, 'Extinct') : null;
  return h('article', {
    class: `ft-node ft-${n.kind}${n.extinct ? ' ft-extinct' : ''}${opts.compact ? ' ft-compact' : ''}`,
    'data-node': n.key, 'data-kind': n.kind,
  },
    fig,
    h('div', { class: 'ft-text' },
      h('h4', { class: 'ft-title' }, n.title, badge),
      h('p', { class: 'ft-sub muted small' }, n.subtitle),
      chips));
}

/** The branches hanging off one trunk stage (nested splits indented under their parent). */
function branchList(bs: FamilyBranch[]): HTMLElement | null {
  if (!bs.length) return null;
  return h('div', { class: 'ft-branches', role: 'list', 'aria-label': 'Species that split off' },
    ...bs.map((b) => h('div', { class: 'ft-branch', role: 'listitem', 'data-species': String(b.species), style: `--depth:${b.depth}` },
      nodeCard(b.node, { compact: true }))));
}

/** Draw the model into `root` (replacing its contents). */
export function renderFamilyTree(root: HTMLElement, m: FamilyModel): void {
  if (m.empty) {
    root.replaceChildren(h('p', { class: 'muted', id: 'family-empty' }, 'Found a lineage to start its family tree: each stage of its evolution shows here.'));
    return;
  }
  const head = h('div', { class: 'ft-head' },
    h('h3', {}, m.name, m.extinct ? h('span', { class: 'ft-badge' }, 'Died out') : null),
    m.speciesName ? h('p', { class: 'muted small' }, `Species: ${m.speciesName}`) : null,
    h('p', { class: 'ft-summary small', id: 'family-summary' }, describeFamily(m)));
  const rows = m.trunk.map((n, i) => {
    const off = m.branches.filter((b) => b.fromTrunk === i);
    return h('li', { class: `ft-row${off.length ? ' ft-has-branches' : ''}${i === m.trunk.length - 1 ? ' ft-last' : ''}` },
      h('div', { class: 'ft-stem' }, nodeCard(n)), branchList(off));
  });
  root.replaceChildren(head, h('ol', { class: 'ft-trunk', 'aria-label': 'Stages of the lineage, oldest first' }, ...rows),
    h('p', { class: 'ft-legend muted small' }, 'Each stage shows the average animal of the lineage on that day; labels name what changed since the stage above. Branches are species that split off.'));
}

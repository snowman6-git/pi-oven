// A session file is a tree of entries; what a person thinks in is *turns*: one message of theirs
// plus everything that followed it until their next. This turns the entry tree into a tree of turns.

export type Kind = 'user' | 'assistant' | 'tool' | 'bash' | 'compaction' | 'branch' | 'other';

export interface Entry {
  id: string;
  parentId: string | null;
  kind: Kind;
  text: string;
  tools: string[];
  ts?: string | null;
  error: boolean;
}

export interface Turn {
  /** The id of the user entry that opens the turn (or of its first entry, for the opening stretch). */
  id: string;
  user: Entry | null;
  entries: Entry[];
  /** The last entry of the turn in file order: where "continue from here" cuts the session. */
  tip: Entry;
  parent: Turn | null;
  children: Turn[];
  /** On the path from the start to where the session stands now. */
  onPath: boolean;
  /** The turn the session stands in. */
  current: boolean;
  label?: string;
  reply: string;
  tools: string[];
  toolCalls: number;
  errors: number;
}

export interface TurnTree {
  roots: Turn[];
  byId: Map<string, Turn>;
  /** Turns the person can see, in the order a list should show them. */
  rows: { turn: Turn; depth: number }[];
}

const meaningful = (e: Entry) => e.kind !== 'other';

export function buildTurns(entries: Entry[], leafId: string | null, labels: Record<string, string> = {}): TurnTree {
  const owner = new Map<string, Turn>(); // entry id -> its turn
  const byEntry = new Map(entries.map((e) => [e.id, e]));
  const turns: Turn[] = [];

  for (const e of entries) {
    const parentTurn = e.parentId ? owner.get(e.parentId) : undefined;
    let turn: Turn;
    if (e.kind === 'user' || !parentTurn) {
      // A user message starts a turn; so does an entry with nothing before it (the opening stretch).
      turn = { id: e.id, user: e.kind === 'user' ? e : null, entries: [], tip: e, parent: parentTurn ?? null, children: [], onPath: false, current: false, reply: '', tools: [], toolCalls: 0, errors: 0 };
      turns.push(turn);
      if (parentTurn) parentTurn.children.push(turn);
    } else {
      turn = parentTurn;
    }
    turn.entries.push(e);
    turn.tip = e;
    owner.set(e.id, turn);
    if (e.kind === 'assistant') {
      if (!turn.reply && e.text) turn.reply = e.text;
      for (const t of e.tools) {
        turn.toolCalls++;
        if (!turn.tools.includes(t)) turn.tools.push(t);
      }
    }
    if (e.error) turn.errors++;
    if (labels[e.id]) owner.get(e.id)!.label = labels[e.id];
  }

  // An opening stretch with nothing in it a person would recognise (model and thinking-level settings)
  // is not worth a row of its own: its children become the roots.
  const roots: Turn[] = [];
  const hidden = new Set<Turn>();
  for (const t of turns) {
    if (t.parent === null && !t.user && !t.entries.some(meaningful)) hidden.add(t);
  }
  for (const t of turns) {
    if (hidden.has(t)) continue;
    if (t.parent === null) roots.push(t);
    else if (hidden.has(t.parent)) {
      t.parent = null;
      roots.push(t);
    }
  }
  for (const h of hidden) h.children = [];

  // Where the session stands, and the way there.
  let cursor = leafId ? byEntry.get(leafId) : undefined;
  const leafTurn = cursor ? owner.get(cursor.id) : undefined;
  if (leafTurn && !hidden.has(leafTurn)) leafTurn.current = true;
  for (let guard = 0; cursor && guard <= entries.length; guard++) {
    const t = owner.get(cursor.id);
    if (t) t.onPath = true;
    cursor = cursor.parentId ? byEntry.get(cursor.parentId) : undefined;
  }

  // The straight line continues along the path to now; alternatives are indented and listed first, right
  // after the point they branch from. An explicit stack keeps long conversations from nesting calls.
  const rows: TurnTree['rows'] = [];
  const stack: [Turn, number][] = roots.map((r) => [r, 0] as [Turn, number]).reverse();
  while (stack.length) {
    const [turn, depth] = stack.pop()!;
    rows.push({ turn, depth });
    const trunk = turn.children.find((c) => c.onPath) ?? turn.children[turn.children.length - 1];
    if (trunk) stack.push([trunk, depth]);
    for (let i = turn.children.length - 1; i >= 0; i--) {
      if (turn.children[i] !== trunk) stack.push([turn.children[i], depth + 1]);
    }
  }

  return { roots, byId: new Map(turns.filter((t) => !hidden.has(t)).map((t) => [t.id, t])), rows };
}

/** How many separate lines of conversation the tree holds (1 means it never branched). */
export function branchCount(tree: TurnTree): number {
  let leaves = 0;
  for (const { turn } of tree.rows) if (turn.children.length === 0) leaves++;
  return leaves;
}

// The order background tasks take in the row. Tasks that a script starts together are created in no
// particular order (each call lands a few milliseconds apart), so start time alone would shuffle them;
// within such a group the names decide, with numbers read as numbers ("sleep 2s" before "sleep 10s").

export interface Placeable { id: string; name?: string; startTime: number }

const names = new Intl.Collator('ko', { numeric: true, sensitivity: 'base' });

/** Groups are runs of tasks that started less than `gap` ms after the one before; groups keep time order. */
export function orderBatch<T extends Placeable>(tasks: T[], gap = 1000): T[] {
  const byTime = [...tasks].sort((a, b) => a.startTime - b.startTime || a.id.localeCompare(b.id));
  const groups: T[][] = [];
  for (const t of byTime) {
    const g = groups.at(-1);
    if (g && t.startTime - g[g.length - 1].startTime < gap) g.push(t);
    else groups.push([t]);
  }
  return groups.flatMap((g) => g.sort((a, b) => names.compare(a.name ?? '', b.name ?? '') || a.startTime - b.startTime || a.id.localeCompare(b.id)));
}

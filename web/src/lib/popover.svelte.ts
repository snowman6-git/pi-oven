// Small popovers (the model and permission choices, the + menu, the header's "more"): each open one is a surface the
// phone's back button should close first, like a dialog or the context menu. App.svelte counts them for its history
// entries and closes the newest on back.
import { untrack } from 'svelte';

const open = $state<{ list: (() => void)[] }>({ list: [] });

/** Call from an `$effect` while the popover is shown; returns the cleanup that takes it off the list. Untracked: push
    and splice read the list too, and an effect that read what it writes would run itself again forever. */
export function trackPopover(close: () => void): () => void {
  untrack(() => open.list.push(close));
  return () => untrack(() => {
    const i = open.list.indexOf(close);
    if (i >= 0) open.list.splice(i, 1);
  });
}

export const popovers = {
  get count() { return open.list.length; },
  /** Close the newest one; false when none is open. */
  closeTop(): boolean {
    const close = open.list.at(-1);
    if (!close) return false;
    close();
    return true;
  },
};

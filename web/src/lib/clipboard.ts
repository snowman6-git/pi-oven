/**
 * Copy text. navigator.clipboard only exists in secure contexts (https or localhost), so over a
 * plain-http LAN address it is undefined; fall back to a hidden textarea and execCommand.
 */
export async function copyText(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch { /* permission denied: try the fallback */ }

  const ta = document.createElement('textarea');
  ta.value = text;
  ta.setAttribute('readonly', '');
  Object.assign(ta.style, { position: 'fixed', top: '0', left: '0', opacity: '0', pointerEvents: 'none' });
  const keep = document.activeElement as HTMLElement | null;
  document.body.append(ta);
  ta.select();
  ta.setSelectionRange(0, text.length);
  let ok = false;
  try { ok = document.execCommand('copy'); } catch { /* blocked */ }
  ta.remove();
  keep?.focus?.();
  return ok;
}

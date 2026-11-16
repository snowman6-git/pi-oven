// The phone app shows the server's page in its only window, and an Android WebView cannot open a second one: a link
// with target=_blank (every link in a reply) and window.open did nothing at all. In the app (window.__piOvenPhone, set
// by the app) such links become plain navigation instead; the app catches any that leave the server and hands them to
// the phone's browser, so the page itself never goes away.

const outside = (href: string) => {
  try {
    const u = new URL(href, location.href);
    return (u.protocol === 'http:' || u.protocol === 'https:') && u.origin !== location.origin;
  } catch {
    return false;
  }
};

export function phoneLinks() {
  if (!(window as unknown as { __piOvenPhone?: boolean }).__piOvenPhone) return;
  document.addEventListener(
    'click',
    (e) => {
      const a = (e.target as Element | null)?.closest?.('a[href]') as HTMLAnchorElement | null;
      if (!a || !outside(a.href)) return;
      e.preventDefault();
      location.href = a.href;
    },
    true,
  );
  const open = window.open.bind(window);
  window.open = ((url?: string | URL, ...rest: unknown[]) => {
    const href = url == null ? '' : String(url);
    if (href && outside(href)) {
      location.href = href;
      return null;
    }
    return open(url, ...(rest as [string?, string?]));
  }) as typeof window.open;
}

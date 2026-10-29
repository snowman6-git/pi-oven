// The Maplestory font (Nexon) is bundled under Nexon's terms (see web/src/assets/fonts/MAPLESTORY-LICENSE.txt and
// ASSETS_LICENSE.md). It is still looked up rather than imported, so a build without the file works too: the app then
// uses the system font and hides the choice.

const found = import.meta.glob('../assets/fonts/Maplestory-Light.woff2', { query: '?url', import: 'default', eager: true }) as Record<string, string>;
const url = Object.values(found)[0];

export const hasMaple = Boolean(url);

if (url) {
  const style = document.createElement('style');
  style.textContent = `@font-face { font-family: 'NxMaple'; src: url('${url}') format('woff2'); font-weight: 300 400; font-style: normal; font-display: swap; }`;
  document.head.append(style);
}

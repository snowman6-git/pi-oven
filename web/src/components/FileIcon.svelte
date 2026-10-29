<script lang="ts">
  import { kindOf, type Glyph } from '../lib/filetypes';

  // A file's icon: the page outline, and inside it a small drawing for the kind of file, in that family's tint.
  let { name, size = 15 }: { name: string; size?: number } = $props();
  const kind = $derived(kindOf(name));

  const INNER: Record<Glyph, string> = {
    file: '',
    braces: '<path d="M10.6 11.4c-1.2 0-1.6.5-1.6 1.6v.6c0 .7-.5 1-1.1 1.1.6.1 1.1.4 1.1 1.1v.6c0 1.1.4 1.6 1.6 1.6M13.4 11.4c1.2 0 1.6.5 1.6 1.6v.6c0 .7.5 1 1.1 1.1-.6.1-1.1.4-1.1 1.1v.6c0 1.1-.4 1.6-1.6 1.6"/>',
    tag: '<path d="M10.4 12l-2.2 2.6 2.2 2.6M13.6 12l2.2 2.6-2.2 2.6"/>',
    code: '<path d="M10.2 12.2l-2 2.4 2 2.4M13.8 12.2l2 2.4-2 2.4M12.9 11.6l-1.8 6"/>',
    hash: '<path d="M10.6 11.6l-.8 6M14.4 11.6l-.8 6M8.9 13.4h6.7M8.4 15.9h6.7"/>',
    lines: '<path d="M8.5 12.6h7M8.5 15h7M8.5 17.4h4"/>',
    gear: '<path d="M8.5 13.2h1.2M12.6 13.2h3M8.5 16.8h3.4M14.8 16.8h.8"/><circle cx="11.1" cy="13.2" r="1.2"/><circle cx="13.4" cy="16.8" r="1.2"/>',
    terminal: '<path d="M8.6 12.6l2.6 2.1-2.6 2.1M12.6 17.2h3.2"/>',
    image: '<path d="M8.4 17.6l2.3-2.8 1.7 1.9 1.5-1.7 1.8 2.6z"/><circle cx="10.2" cy="12.6" r=".8"/>',
    database: '<ellipse cx="12" cy="12.4" rx="3.5" ry="1.3"/><path d="M8.5 12.4v5c0 .7 1.6 1.3 3.5 1.3s3.5-.6 3.5-1.3v-5M8.5 14.9c0 .7 1.6 1.3 3.5 1.3s3.5-.6 3.5-1.3"/>',
    lock: '<rect x="9" y="14.3" width="6" height="4" rx=".8"/><path d="M10.4 14.3v-1a1.6 1.6 0 0 1 3.2 0v1"/>',
    branch: '<circle cx="10" cy="12.3" r="1"/><circle cx="10" cy="17.4" r="1"/><circle cx="14.3" cy="14.4" r="1"/><path d="M10 13.3v3.1M13.4 14.7c-2.2.4-3.4 1-3.4 2"/>',
    package: '<path d="M8.5 13.2l3.5-1.8 3.5 1.8v4l-3.5 1.8-3.5-1.8zM8.5 13.2L12 15l3.5-1.8M12 15v4"/>',
    binary: '<rect x="8.6" y="12.2" width="2.2" height="4.4" rx="1.1"/><path d="M14 12.2v4.4M13 13h1M12.9 16.6h2.2"/>',
    archive: '<path d="M12 11.4v7.2M12 12.6h1.5M10.5 14h1.5M12 15.4h1.5M10.5 16.8h1.5"/>',
    media: '<path d="M10 12.2v5.4l4.8-2.7z"/>',
    font: '<path d="M8.8 17.6l3.2-6.2 3.2 6.2M10.1 15.6h3.8"/>',
    test: '<path d="M8.8 15l2.1 2.1 4.3-4.4"/>',
  };
</script>

<svg class="fi" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" data-kind={kind.glyph} data-tint={kind.tint}>
  <path d="M7 3h7l5 5v11a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2zM14 3v5h5" />
  <g class="in" style:stroke="var(--ft-{kind.tint})">{@html INNER[kind.glyph]}</g>
</svg>

<style>
  .in {
    stroke-width: 1.9;
  }
</style>

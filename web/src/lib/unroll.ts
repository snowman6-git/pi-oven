// The plan cards in the info tab: a plan folded to one line shows the split ring (LimitRing); opened, each half of
// the ring flies to its row and unrolls into that row's bar (right half → the 5-hour bar, left half → the weekly
// one). Folding runs it back. The lines are drawn on a page-wide SVG while they move, clipped to the card's live
// outline every frame so they never show outside it, and the real bars take over when they land.
//
// Only here, and only for these few hundred milliseconds, is anything redrawn per frame (points of two polylines);
// the rest is transform, opacity and a short height animation, as elsewhere (see lib/sheet.ts).

import { bezier, spring } from './sheet';

export const easeOut = bezier(0.2, 0.8, 0.2, 1);
export const easeIn = bezier(0.4, 0, 1, 1);
export const easeInOut = bezier(0.4, 0, 0.2, 1);

export const reducedMotion = () => typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

/** Height keyframes from `from` to `to` along the spring, its overshoot kept to `cap` px, sampled 240 times a second. */
export function springHeights(from: number, to: number, ms: number, cap: number): Keyframe[] {
  const n = Math.max(2, Math.ceil((ms / 1000) * 240));
  const out: Keyframe[] = [];
  for (let i = 0; i <= n; i++) {
    const t = spring(i / n);
    const h = t <= 1 ? from + (to - from) * t : to + Math.sign(to - from) * Math.min(cap, Math.abs(to - from) * (t - 1));
    out.push({ offset: i / n, height: `${h}px` });
  }
  return out;
}

type Pts = { pts: [number, number][]; w: number };
const N = 40;
/** How much of each half the ring's groove hides at either end (LimitRing's END, as a fraction). */
const END = 0.055;

/** One half of the ring on screen, from its bottom (where its fill starts) to its top. `side`: 'short' is the right
    half (5 hours), 'long' the left (a week). Drawn in LimitRing's 20×20 box: centre 10,10, radius 7, stroke 2.4. */
export function arcPoints(ring: Element, side: 'short' | 'long'): Pts {
  const r = ring.getBoundingClientRect();
  const sc = r.width / 20;
  const pts: [number, number][] = [];
  for (let i = 0; i <= N; i++) {
    const u = END + (1 - 2 * END) * (i / N);
    const a = side === 'short' ? Math.PI / 2 - Math.PI * u : Math.PI / 2 + Math.PI * u;
    pts.push([r.left + (10 + 7 * Math.cos(a)) * sc, r.top + (10 + 7 * Math.sin(a)) * sc]);
  }
  return { pts, w: 2.4 * sc };
}

/** A row's bar, left to right along its middle. */
export function barPoints(meter: Element): Pts {
  const r = meter.getBoundingClientRect();
  const y = r.top + r.height / 2;
  const pts: [number, number][] = [];
  for (let i = 0; i <= N; i++) pts.push([r.left + (r.width * i) / N, y]);
  return { pts, w: r.height };
}

/** `pos` 0 = the ring's half, 1 = a short line at the bar's start (as long as the half), 2 = the whole bar. */
function shape(arc: Pts, bar: Pts, pos: number): Pts {
  const [x0, y] = bar.pts[0];
  const W = bar.pts[N][0] - x0;
  const L = Math.min(W, Math.PI * 7 * (arc.w / 2.4) * (1 - 2 * END));
  const len = pos <= 1 ? L : L + (W - L) * (pos - 1);
  const flat = arc.pts.map((_, i) => [x0 + (len * i) / N, y] as [number, number]);
  if (pos >= 1) return { pts: flat, w: bar.w };
  return {
    pts: arc.pts.map(([x, y0], i) => [x + (flat[i][0] - x) * pos, y0 + (flat[i][1] - y0) * pos]),
    w: arc.w + (bar.w - arc.w) * Math.min(1, Math.max(0, pos)),
  };
}

export type Item = { arc: Pts; bar: Pts; left: number };
export type Step = { ms: number; from: number; to: number; ease: (t: number) => number; stagger: number };

/** Opening: fly to the bar's start straightening into a short line (spring), then stretch right (ease-out). */
export const UNROLL: Step[] = [
  { ms: 360, from: 0, to: 1, ease: spring, stagger: 50 },
  { ms: 340, from: 1, to: 2, ease: easeOut, stagger: 0 },
];
/** Folding: shrink back to the short line, then fly home to the ring. */
export const ROLL_UP: Step[] = [
  { ms: 110, from: 2, to: 1, ease: easeIn, stagger: 0 },
  { ms: 190, from: 1, to: 0, ease: easeInOut, stagger: 0 },
];

const NS = 'http://www.w3.org/2000/svg';
let stage: SVGSVGElement | null = null;
function stageEl(): SVGSVGElement {
  if (stage?.isConnected) return stage;
  stage = document.createElementNS(NS, 'svg');
  stage.setAttribute('aria-hidden', 'true');
  stage.setAttribute('class', 'unroll-stage');
  document.body.appendChild(stage);
  return stage;
}

/** Draw the lines between ring and bars along `steps`; never outside `box` (radius `round` px). */
export function play(items: Item[], steps: Step[], box: Element, round = 10): Promise<void> {
  return new Promise((done) => {
    const st = stageEl();
    const lines = items.map(() => {
      const track = document.createElementNS(NS, 'polyline');
      const fill = document.createElementNS(NS, 'polyline');
      track.setAttribute('class', 'trk');
      fill.setAttribute('class', 'arc');
      st.append(track, fill);
      return { track, fill };
    });
    const total = steps.reduce((t, s) => t + s.ms + s.stagger * (items.length - 1), 0);
    const str = (a: [number, number][]) => a.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(' ');
    const t0 = performance.now();
    const frame = (now: number) => {
      const el = now - t0;
      const r = box.getBoundingClientRect();
      st.style.clipPath = `inset(${r.top}px ${innerWidth - r.right}px ${innerHeight - r.bottom}px ${r.left}px round ${round}px)`;
      items.forEach(({ arc, bar, left }, j) => {
        let t = el;
        let pos = steps[0].from;
        for (const s of steps) {
          const span = s.ms + s.stagger * (items.length - 1);
          if (t < span) {
            const q = Math.min(1, Math.max(0, (t - s.stagger * j) / s.ms));
            pos = s.from + (s.to - s.from) * s.ease(q);
            break;
          }
          t -= span;
          pos = s.to;
        }
        const sh = shape(arc, bar, pos);
        const cut = Math.round(N * Math.max(0, Math.min(1, left)));
        lines[j].track.setAttribute('points', str(sh.pts));
        lines[j].fill.setAttribute('points', str(sh.pts.slice(0, cut + 1)));
        lines[j].track.style.strokeWidth = lines[j].fill.style.strokeWidth = String(sh.w);
      });
      if (el < total) requestAnimationFrame(frame);
      else {
        for (const l of lines) {
          l.track.remove();
          l.fill.remove();
        }
        st.style.clipPath = '';
        done();
      }
    };
    requestAnimationFrame(frame);
  });
}

import { tick } from 'svelte';
import { MediaQuery } from 'svelte/reactivity';

export function trackFeed(
  list: () => HTMLElement | undefined,
  scope: () => string | null,
  posts: () => ReadonlyArray<{ id: string }>
) {
  const order = $derived(JSON.stringify(posts().map(post => post.id)));
  const reducedMotion = new MediaQuery('(prefers-reduced-motion: reduce)');
  let previousScope: string | null = null;
  let previousOrder = '';
  $effect(() => {
    const element = list();
    if (element && reducedMotion.current) stopFeedAnimations(element);
  });
  $effect.pre(() => {
    const nextScope = scope();
    const changed = nextScope !== null && nextScope === previousScope && order !== previousOrder;
    previousScope = nextScope;
    previousOrder = order;
    const element = list();
    if (!changed || !element) return;
    const apply = measureFeed(element);
    let current = true;
    void tick().then(() => { if (current) apply(reducedMotion.current); });
    return () => { current = false; };
  });
}

function stopFeedAnimations(list: HTMLElement) {
  for (const animation of list.getAnimations({subtree: true})) {
    if (animation.id === 'feed-update') animation.cancel();
  }
}

// Measure before Svelte reconciles the keyed posts, then move the surviving rows
// from their previous screen positions. The actual DOM stays in reading order.
function measureFeed(list: HTMLElement) {
  const rows = () => Array.from(list.querySelectorAll<HTMLElement>(':scope > article[data-post-id]'));
  const before = new Map(rows().map(row => [row, row.getBoundingClientRect()]));
  const header = document.querySelector('.app-header')?.getBoundingClientRect().bottom ?? 0;
  const reading = window.scrollY > 0 && list.getBoundingClientRect().top <= header;
  const focused = list.contains(document.activeElement) ? document.activeElement as HTMLElement : null;
  const focusedIndex = focused ? [...before.keys()].findIndex(row => row.contains(focused)) : -1;

  return (reducedMotion: boolean) => {
    if (!list.isConnected || !list.getClientRects().length) return;
    const current = rows();
    // An interrupted update starts from the last painted position.
    stopFeedAnimations(list);
    if (focused && document.activeElement === document.body) {
      const neighbor = current[Math.min(focusedIndex, current.length - 1)];
      const target = focused.isConnected ? focused : neighbor?.querySelector<HTMLElement>('.read-post')
        ?? list.querySelector<HTMLElement>('.empty h2, :scope > p');
      if (target) {
        if (!target.matches('button, a[href], [tabindex]')) target.tabIndex = -1;
        target.focus({preventScroll: true});
      }
    }
    if (reading) {
      const anchor = [...before].find(([row, rect]) =>
        row.isConnected && rect.bottom > header && rect.top < window.innerHeight);
      if (anchor) {
        const [row, previous] = anchor;
        const shift = row.getBoundingClientRect().top - previous.top;
        if (Math.abs(shift) > 1) window.scrollBy({top: shift, behavior: 'instant'});
      }
    }
    // A complete snapshot replacing a cold or expired cache is a new feed.
    const replacement = current.length > 1 && !current.some(row => before.has(row));
    if (replacement || reducedMotion) return;

    const visible = (rect: DOMRect) => rect.bottom > header && rect.top < window.innerHeight;
    const positions = current.map(row => ({row, after:row.getBoundingClientRect()}));
    for (const {row, after} of positions) {
      const old = before.get(row);
      if (!visible(after) && !(old && visible(old))) continue;
      const delta = old ? old.top - after.top : -8;
      if (old && Math.abs(delta) < 1) continue;
      // A rank change across many screens shouldn't fly an article across the reader.
      const distance = Math.max(-window.innerHeight / 2, Math.min(window.innerHeight / 2, delta));
      row.animate([
        { transform: `translateY(${distance}px)`, opacity: old ? 1 : 0 },
        { transform: 'translateY(0)', opacity: 1 }
      ], { id: 'feed-update', duration: 210, easing: 'cubic-bezier(.2,.75,.25,1)' });
    }
  };
}

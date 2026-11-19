// Moving between ordered screens should leave navigation, scroll and focus in place.
export function pageMotion(panel: HTMLElement, position: number | null) {
  const reduced = matchMedia('(prefers-reduced-motion: reduce)');
  let animation: Animation | undefined;
  function stop() { animation?.cancel(); }
  reduced.addEventListener('change', stop);
  return {
    update(next: number | null) {
      const previous = position;
      position = next;
      if (previous === next) return;
      stop();
      if (next === null || previous === null || reduced.matches || !panel.getClientRects().length) return;
      const distance = next > previous ? 18 : -18;
      animation = panel.animate([
        { transform: `translateX(${distance}px)` },
        { transform: 'translateX(0)' }
      ], { duration: 205, easing: 'cubic-bezier(.2,.75,.25,1)' });
    },
    destroy() {
      stop();
      reduced.removeEventListener('change', stop);
    }
  };
}

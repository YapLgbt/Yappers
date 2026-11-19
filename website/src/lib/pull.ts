// One gesture recognizer for the page and the scrollable Preferences dialog.
export function pullToRefresh(ready: () => boolean, refresh: () => Promise<void>) {
  return (element: HTMLElement) => {
    const listeners = new AbortController();
    const feedback = document.createElement('div');
    feedback.className = 'pull-feedback';
    feedback.role = 'status';
    feedback.hidden = true;
    element.prepend(feedback);
    let start: { x: number; y: number } | null = null;
    let distance = 0;
    const offset = () => element.closest('dialog') ? element.scrollTop : element.scrollTop + window.scrollY;
    function cancel() {
      start = null; distance = 0; feedback.hidden = true; feedback.removeAttribute('id');
    }
    element.addEventListener('touchstart', event => {
      cancel();
      if (!ready() || offset() > 0 || event.touches.length !== 1 ||
          (event.target as Element).closest('button,a,input,select,textarea,summary,label')) return;
      if (((event.target as Element).closest<HTMLElement>('[data-scroll-history]')?.scrollTop ?? 0) > 0) return;
      const touch = event.touches[0];
      start = { x: touch.clientX, y: touch.clientY };
    }, { passive: true, signal: listeners.signal });
    element.addEventListener('touchmove', event => {
      if (!start) return;
      if (event.touches.length !== 1 || offset() > 0) return cancel();
      const touch = event.touches[0];
      distance = touch.clientY - start.y;
      if (distance < 0 || Math.abs(touch.clientX - start.x) > Math.max(distance,12)) return cancel();
      feedback.hidden = distance < 12;
      if (!feedback.hidden) {
        event.preventDefault();
        feedback.id = 'pull-feedback';
        feedback.textContent = distance >= 80 ? 'Release to refresh' : 'Pull to refresh';
      }
    }, { passive: false, signal: listeners.signal });
    element.addEventListener('touchcancel', cancel, { signal: listeners.signal });
    element.addEventListener('touchend', async () => {
      const trigger = !!start && distance >= 80;
      cancel();
      if (trigger) {
        feedback.textContent = 'Refreshing…'; feedback.hidden = false;
        try { await refresh(); } finally { cancel(); }
      }
    }, { signal: listeners.signal });
    return { destroy() { listeners.abort(); feedback.remove(); } };
  };
}

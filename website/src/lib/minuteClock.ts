import { readable } from 'svelte/store';

// Feed and thread timestamps share one clock while posts are on screen.
export const minuteClock = readable(Date.now(), set => {
  if (typeof window === 'undefined') return;
  const update = () => set(Date.now());
  update();
  const timer = window.setInterval(update, 60_000);
  document.addEventListener('visibilitychange', update);
  return () => {
    window.clearInterval(timer);
    document.removeEventListener('visibilitychange', update);
  };
});

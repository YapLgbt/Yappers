// Keep the reader's keyboard position when a status resolves while its controls have focus.
export function deliveryDisclosure(panel: HTMLDetailsElement, returnFocus: () => void) {
  let heldFocus = false;
  function trackFocus(event: FocusEvent) {
    heldFocus = event.target instanceof Node && panel.contains(event.target);
  }
  function closeWithEscape(event: KeyboardEvent) {
    if (event.key !== 'Escape' || !panel.open) return;
    event.preventDefault();
    panel.open = false;
    panel.querySelector('summary')?.focus({ preventScroll: true });
  }
  document.addEventListener('focusin', trackFocus);
  panel.addEventListener('keydown', closeWithEscape);
  return {
    destroy() {
      panel.removeEventListener('keydown', closeWithEscape);
      document.removeEventListener('focusin', trackFocus);
      if (heldFocus) queueMicrotask(() => {
        if (document.activeElement === document.body) returnFocus();
      });
    }
  };
}

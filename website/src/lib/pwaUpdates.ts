export function watchPwaUpdates(canReload: () => boolean) {
  if (import.meta.env.DEV || !('serviceWorker' in navigator)) return () => {};

  const workers = navigator.serviceWorker;
  const build = document.querySelector<HTMLMetaElement>('meta[name="yap-build"]')?.content;
  const listeners = new AbortController();
  const options = { signal: listeners.signal };
  let registration: ServiceWorkerRegistration | undefined;
  let checking = false;
  let retry: number | undefined;
  let reloading = false;

  function reloadWhenReady() {
    window.clearTimeout(retry);
    if (reloading) return;
    if (!canReload() || document.querySelector('dialog[open]')) {
      retry = window.setTimeout(reloadWhenReady, 1000);
      return;
    }
    reloading = true;
    location.reload();
  }

  function askBuild() {
    workers.controller?.postMessage({type: 'yap-get-build'});
  }
  workers.addEventListener('message', event => {
    if (event.source === workers.controller && event.data?.type === 'yap-build') {
      if (event.data.build !== build) reloadWhenReady();
      else window.clearTimeout(retry);
    }
  }, options);
  workers.addEventListener('controllerchange', askBuild, options);

  async function register() {
    const scope = {scope: '/app/', updateViaCache: 'none'} as const;
    const current = await workers.register('/app/background-sw.js', {...scope, type: 'module'})
      .catch(() => workers.register('/app/sw.js', scope));
    const periodic = (current as ServiceWorkerRegistration & {
      periodicSync?: {register: (tag: string, options: {minInterval: number}) => Promise<void>}
    }).periodicSync;
    if (periodic) {
      void navigator.permissions.query({name:'periodic-background-sync' as PermissionName})
        .then(permission => permission.state === 'granted'
          ? periodic.register('yap-refresh', {minInterval: 24 * 60 * 60 * 1000}) : undefined)
        .catch(() => {});
    }
    return current;
  }
  async function check() {
    if (document.hidden) return;
    askBuild();
    if (!navigator.onLine || checking) return;
    checking = true;
    try {
      registration ??= await register();
      if (!listeners.signal.aborted && registration.active) await registration.update();
    }
    catch { /* Retry when connectivity returns or on the next check. */ }
    finally { checking = false; }
  }
  window.addEventListener('online', check, options);
  window.addEventListener('pageshow', check, options);
  document.addEventListener('visibilitychange', check, options);
  const timer = window.setInterval(check, 60_000);
  void check();
  return () => {
    listeners.abort();
    window.clearInterval(timer);
    window.clearTimeout(retry);
  };
}

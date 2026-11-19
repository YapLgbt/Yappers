import { defineConfig } from 'vite';
import { sveltekit } from '@sveltejs/kit/vite';
import adapter from '@sveltejs/adapter-static';

export default defineConfig({
  build: { rolldownOptions: { external: ['/app/network.js', '/app/recovery.js', '/app/flag-print.js'] } },
  plugins: [sveltekit({
    adapter: adapter({ pages: 'dist' }),
    paths: { relative: false },
    prerender: { crawl: false, entries: ['*'] },
    serviceWorker: { register: false }
  })]
});

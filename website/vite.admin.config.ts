import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import adapter from '@sveltejs/adapter-static';

export default defineConfig({
  plugins: [tailwindcss(), sveltekit({
    adapter: adapter({ pages: 'dist-admin' }),
    files: { routes: 'src/admin', assets: 'src/admin/static' },
    outDir: '.svelte-kit-admin',
    paths: { relative: false },
    prerender: { crawl: false, entries: ['/'] },
    serviceWorker: { register: false },
    csp: {
      mode: 'hash',
      directives: {
        'default-src': ['none'],
        'script-src': ['self'],
        'style-src': ['self'],
        // SvelteKit sets this fixed inline style on its screen-reader route announcer.
        'style-src-attr': ['unsafe-hashes', 'sha256-S8qMpvofolR8Mpjy4kQvEm7m1q8clzU4dfDH0AmvZjo='],
        'font-src': ['self'],
        'connect-src': ['self'],
      },
    },
  })],
});

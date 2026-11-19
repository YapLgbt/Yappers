// The host allows same-origin scripts and WASM, including when launched offline.
// Keep SvelteKit's bootstrap in its original DOM position as an external classic script.
import { createHash } from 'node:crypto';
import { readFile, readdir, writeFile } from 'node:fs/promises';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse } from 'parse5';

const dist = fileURLToPath(new URL('../dist/', import.meta.url));
function* descendants(node) {
  yield node;
  for (const child of node.childNodes ?? []) yield* descendants(child);
}
async function externalizeBootstrap(shell) {
  let html=await readFile(shell,'utf8');
  const inline = [...descendants(parse(html, { sourceCodeLocationInfo: true }))].filter(node =>
    node.tagName === 'script' && !node.attrs.some(attr => attr.name === 'src' || attr.name === 'type')
  );
  if (inline.length !== 1) throw new Error('Expected one classic bootstrap script in the app shell.');
  const { startTag, endTag } = inline[0].sourceCodeLocation;
  const code = html.slice(startTag.endOffset, endTag.startOffset);
  const digest = bytes => createHash('sha256').update(bytes).digest('hex').slice(0,20);
  const startup = 'start.' + digest(code) + '.js';
  await writeFile(join(dist, 'app', startup), code);
  html = html.slice(0,startTag.startOffset) + '<script src="/app/' + startup + '"></script>' + html.slice(endTag.endOffset);
  await writeFile(shell, html);
}
await Promise.all(['app/index.html','app/demo/index.html'].map(path=>externalizeBootstrap(join(dist,path))));

// Distribute the notices alongside the components in the static release.
const materialNotices = await Promise.all(['LICENSE', 'NOTICE'].map(name =>
  readFile(new URL('../node_modules/material/' + name, import.meta.url), 'utf8')
));
await writeFile(join(dist, 'app', 'material-notices.txt'), materialNotices.join('\n'));

const kitPackages = ['@skeletonlabs/skeleton', '@skeletonlabs/skeleton-common', '@skeletonlabs/skeleton-svelte', 'tailwindcss'];
const kitNotices = await Promise.all(kitPackages.map(async name => {
  const license = await readFile(new URL('../node_modules/' + name + '/LICENSE', import.meta.url), 'utf8');
  return name + '\n' + license;
}));
await writeFile(join(dist, 'app', 'skeleton-notices.txt'), kitNotices.join('\n'));

async function tree(path) {
  const entries = await readdir(path, { withFileTypes: true });
  const files = await Promise.all(entries.map(entry => {
    const full = join(path, entry.name);
    return entry.isDirectory() ? tree(full) : [relative(dist,full)];
  }));
  return files.flat();
}
const files = [...await tree(join(dist,'app')), ...await tree(join(dist,'_app/immutable')),
  'assets/yap-wordmark.svg', 'assets/icon.svg', 'assets/silali-banerjee-rainbow-preview.jpg']
  .filter(path => path !== 'app/sw.js' && path !== 'app/background-sw.js' && !path.endsWith('.txt')).sort();
const template = await readFile(new URL('../sw-template.js',import.meta.url),'utf8');
const background=await readFile(new URL('../background-sync.js',import.meta.url),'utf8');
const hash = createHash('sha256').update(template).update(background);
for (const path of files) hash.update(path).update(await readFile(join(dist,path)));
const cached=template
  .replace('__YAP_BUILD__',hash.digest('hex').slice(0,20))
  .replace('/* offline files */ []',JSON.stringify(files.map(path => '/' + path)));
await writeFile(join(dist,'app/sw.js'),cached);
await writeFile(join(dist,'app/background-sw.js'),background+'\n'+cached);

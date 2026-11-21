import { readFile, writeFile } from 'node:fs/promises';

const kitPackages = ['@skeletonlabs/skeleton', '@skeletonlabs/skeleton-common', '@skeletonlabs/skeleton-svelte', 'tailwindcss'];

export async function writeUiNotices(file, includeFont = false) {
  const notices = await Promise.all(kitPackages.map(async (name) => {
    const license = await readFile(new URL(`../node_modules/${name}/LICENSE`, import.meta.url), 'utf8');
    return `${name}\n${license}`;
  }));
  if (includeFont) notices.push(await readFile(new URL('../static/app/fonts/OFL.txt', import.meta.url), 'utf8'));
  await writeFile(file, notices.join('\n'));
}

if (import.meta.main) await writeUiNotices(process.argv[2], true);

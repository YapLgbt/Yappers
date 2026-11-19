const normalize = (s: string) =>
  s
    .normalize('NFD')
    .replace(/\p{Mark}/gu, '')
    .toLowerCase();
let directory: Promise<{ rows: string[]; labels: string[] }> | undefined;
async function load() {
  const response = await fetch('/app/towns.tsv.gz');
  if (!response.ok) throw new Error();
  const data = await response.blob();
  // Some hosts serve the file with Content-Encoding: gzip, so fetch already decoded it.
  const header = new Uint8Array(await data.slice(0, 2).arrayBuffer());
  const text = header[0] === 0x1f && header[1] === 0x8b
    ? await new Response(data.stream().pipeThrough(new DecompressionStream('gzip'))).text()
    : await data.text();
  const rows = text.trimEnd().split('\n');
  const labels = rows.map((row) => normalize(row.split('\t', 3).filter(Boolean).join(', ')));
  return { rows, labels };
}
self.onmessage = async ({ data: { generation, query, center } }) => {
  try {
    const { rows, labels } = await (directory ??= load());
    if (center) {
      const rad=Math.PI/18000;
      let best=Math.sin(30 / 7917.52)**2, label=center.label;
      for (const row of rows) {
        const [city,region,country,lat,lon]=row.split('\t');
        const dy=(Number(lat)-center.lat100)*rad;
        if(Math.abs(dy)>0.008) continue;
        const h=Math.sin(dy/2)**2+Math.cos(center.lat100*rad)*Math.cos(Number(lat)*rad)*Math.sin((Number(lon)-center.lon100)*rad/2)**2;
        if(h<best){best=h;label=[city,region,country].filter(Boolean).join(', ');}
      }
      self.postMessage({generation,area:{...center,label}});
      return;
    }
    const term = normalize(query.trim()),
      matches = [];
    for (let i = 0; term.length >= 2 && i < rows.length && matches.length < 12; i++) {
      if (labels[i].startsWith(term)) {
        const [city, region, country, lat, lon] = rows[i].split('\t');
        matches.push({
          area: {
            label: [city, region, country].filter(Boolean).join(', '),
            lat100: Number(lat),
            lon100: Number(lon),
          },
        });
      }
    }
    self.postMessage({ generation, matches });
  } catch {
    directory = undefined;
    self.postMessage({ generation, matches: [], error: true });
  }
};

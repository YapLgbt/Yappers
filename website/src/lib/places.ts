// The worker keeps the full town directory off the rendering thread.
import type { Area } from './community';

export interface TownMatch {
  area: Area;
}
export function searchTowns(onResult: (matches: TownMatch[], error: boolean) => void) {
  const worker = new Worker(new URL('./towns-worker.ts', import.meta.url), { type: 'module' });
  let generation = 0;
  worker.onmessage = ({ data }) => {
    if (data.generation === generation) onResult(data.matches, Boolean(data.error));
  };
  worker.onerror = () => onResult([], true);
  return {
    search(query: string) {
      worker.postMessage({ query, generation: ++generation });
    },
    close() {
      worker.terminate();
    },
  };
}

export function labelLocation(center:Area):Promise<Area> {
  return new Promise(resolve=>{
    const worker = new Worker(new URL('./towns-worker.ts',import.meta.url),{type:'module'});
    const finish = (area:Area) => { clearTimeout(timeout); worker.terminate(); resolve(area); };
    const timeout = setTimeout(()=>finish(center),6000);
    worker.onmessage = ({data})=>finish(data.area ?? center);
    worker.onerror = ()=>finish(center);
    worker.postMessage({center});
  });
}

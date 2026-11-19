import { printEdge } from './print-edges.js';

const motion = matchMedia('(prefers-reduced-motion: reduce)');
function read(key) { try { return localStorage.getItem(key); } catch { return null; } }
function save(key, value) { try { localStorage.setItem(key, value); } catch {} }

// Indexes match the sRGB stripe definitions in the shared fabric shader.
const flags = ['Rainbow', 'Transgender', 'Sunset Lesbian', 'Bisexual', 'Nonbinary',
  'Pansexual', 'Asexual', 'Aromantic', 'Genderfluid'];
const rotationKey = 'yap-estuary-rotation';
const sessionKey = 'yap-estuary-visit';

function drawFlag() {
  let state;
  try { state = JSON.parse(read(rotationKey)); } catch {}
  let remaining = Array.isArray(state?.remaining)
    ? [...new Set(state.remaining.filter(i => Number.isInteger(i) && flags[i]))] : [];
  if (!remaining.length) remaining = flags.map((_, i) => i);
  const choices = remaining.length > 1 ? remaining.filter(i => i !== state?.last) : remaining;
  const pick = choices[Math.floor(Math.random() * choices.length)];
  save(rotationKey, JSON.stringify({last: pick, remaining: remaining.filter(i => i !== pick)}));
  try { sessionStorage.setItem(sessionKey, String(pick)); } catch {}
  return pick;
}

async function visitFlag(figure) {
  // Follow the homepage artwork into onboarding within the same tab.
  if (figure.classList.contains('welcome-art') && document.referrer &&
      new URL(document.referrer).origin === location.origin &&
      new URL(document.referrer).pathname === '/' &&
      performance.getEntriesByType('navigation')[0]?.type === 'navigate') {
    try {
      const saved = sessionStorage.getItem(sessionKey);
      if (saved !== null && flags[Number(saved)]) return Number(saved);
    } catch {}
  }
  // Simultaneous tabs draw from the same collection in sequence.
  if (navigator.locks) {
    try { return await navigator.locks.request(rotationKey, drawFlag); } catch {}
  }
  return drawFlag();
}

async function prepare(figure) {
  const index = await visitFlag(figure);
  const name = flags[index];
  const image = figure.querySelector('img');
  if (index) image.src = new URL('./estuary/' + name.toLowerCase().replace(' ', '-') + '.webp', import.meta.url);
  const label = figure.querySelector('[data-flag-name]');
  if (label) {
    label.textContent = name + ' Estuary';
    image.alt = 'Tidal channels winding through the stripes of the ' + name + ' pride flag.';
  }
  return index;
}

async function mount(figure, palette) {
  const canvas = figure.querySelector('canvas');
  const silhouette = figure.querySelector('#estuary-edge path');
  const gl = canvas.getContext('webgl', {alpha: false, antialias: false, depth: false});
  if (!gl) return; // The printed cover is already in the document.
  const source = await fetch(new URL('./fabric.glsl', import.meta.url));
  if (!source.ok) return;
  const fragment = await source.text();
  function compile(type, code) {
    const shader = gl.createShader(type);
    gl.shaderSource(shader, code);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw Error(gl.getShaderInfoLog(shader));
    return shader;
  }
  const program = gl.createProgram();
  gl.attachShader(program, compile(gl.VERTEX_SHADER,
    'attribute vec2 vertex; void main() { gl_Position = vec4(vertex, 0., 1.); }'));
  gl.attachShader(program, compile(gl.FRAGMENT_SHADER, 'precision highp float;\n' + fragment +
    '\nuniform float visitPalette;\nvoid main() { vec2 p = vec2(gl_FragCoord.x, extent.y-gl_FragCoord.y); gl_FragColor=vec4(fabricWithPalette(p, visitPalette),1.); }'));
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw Error(gl.getProgramInfoLog(program));
  gl.useProgram(program);
  const buffer = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1,-1, 3,-1, -1,3]), gl.STATIC_DRAW);
  const vertex = gl.getAttribLocation(program, 'vertex');
  gl.enableVertexAttribArray(vertex);
  gl.vertexAttribPointer(vertex, 2, gl.FLOAT, false, 0, 0);
  const uniform = name => gl.getUniformLocation(program, name);
  const extent = uniform('extent'), phase = uniform('phase'), edition = uniform('edition');
  gl.uniform1f(uniform('visitPalette'), palette);
  const controls = figure.querySelector('figcaption');
  const pause = controls.querySelector('[data-pause]');
  let userPaused = !!pause && read('yap-fabric-paused') === 'true';
  let reduced = motion.matches;
  let playWithReducedMotion = false;
  function mayPlay() {
    return !userPaused && (!reduced || playWithReducedMotion);
  }
  let visible = false, frame = 0, previous = 0, elapsed = 0;
  function draw() {
    const ratio = Math.min(devicePixelRatio, 2, 1440 / canvas.clientWidth);
    const width = Math.max(1, Math.round(canvas.clientWidth * ratio));
    const height = Math.max(1, Math.round(canvas.clientHeight * ratio));
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    if (silhouette) {
      silhouette.setAttribute('d', printEdge('pool', elapsed));
    }
    gl.viewport(0, 0, width, height);
    gl.uniform2f(extent, width, height);
    gl.uniform1f(phase, elapsed);
    gl.uniform1f(edition, 4655);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }
  function tick(time) {
    if (!mayPlay() || !visible || document.hidden) return;
    const dt = previous ? Math.min(64, time - previous) : 0;
    elapsed = (elapsed + dt * .00038) % (2 * Math.PI);
    previous = time;
    draw();
    frame = requestAnimationFrame(tick);
  }
  function update() {
    cancelAnimationFrame(frame);
    previous = 0;
    if (pause) pause.textContent = mayPlay() ? 'Pause motion' : 'Play motion';
    draw();
    if (mayPlay() && visible && !document.hidden) frame = requestAnimationFrame(tick);
  }
  if (pause) pause.onclick = () => {
    userPaused = mayPlay();
    playWithReducedMotion = reduced && !userPaused;
    save('yap-fabric-paused', String(userPaused));
    update();
  };
  const lifetime = new AbortController();
  motion.addEventListener('change', event => {
    reduced = event.matches;
    playWithReducedMotion = false;
    update();
  }, {signal: lifetime.signal});
  document.addEventListener('visibilitychange', update, {signal: lifetime.signal});
  const visibility = new IntersectionObserver(([entry]) => { visible = entry.isIntersecting; update(); });
  visibility.observe(canvas);
  const sizing = new ResizeObserver(draw);
  sizing.observe(canvas);
  canvas.addEventListener('webglcontextlost', event => {
    event.preventDefault();
    lifetime.abort();
    visibility.disconnect();
    sizing.disconnect();
    figure.querySelector('img').removeAttribute('aria-hidden');
    visible = false;
    cancelAnimationFrame(frame);
    canvas.hidden = true;
    if (pause) pause.hidden = true;
  }, {once:true});
  canvas.addEventListener('webglcontextrestored', () => start(figure, palette), {once:true});
  canvas.hidden = controls.hidden = false;
  if (pause) pause.hidden = false;
  update();
}

function start(figure, selection) {
  Promise.resolve(selection).then(palette => mount(figure, palette)).catch(error => console.warn('Showing the still flag print.', error));
}

// Returning PWA users go straight to the feed without initializing WebGL.
const selections = new WeakMap();
const firstView = new IntersectionObserver(entries => {
  for (const entry of entries) if (entry.isIntersecting) {
    firstView.unobserve(entry.target);
    start(entry.target, selections.get(entry.target) ?? prepare(entry.target));
  }
});
for (const figure of document.querySelectorAll('.flag-print')) {
  if (figure.classList.contains('hero-art')) selections.set(figure, prepare(figure));
  firstView.observe(figure);
}

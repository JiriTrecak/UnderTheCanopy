// Captures the old game's reference stage with standalone effects played at the focus,
// the same way `canopy shot --effect` places them (8 m apart, `age` ticks old, travelling
// layers from 6 m west), for side-by-side reviews (docs/captures/README.md, m3-effects).
// Needs the old repository's dev server on 127.0.0.1:5173 (`npx vite` there) and Chrome.
// Usage:
//   node tools/old-effects/capture.mjs <out.png> <map> <x> <z> <zoom> <effect[,age[,radius]]>...
import { spawn } from 'node:child_process';
import { writeFileSync, mkdirSync } from 'node:fs';

const [out, map, x, z, zoom, ...effects] = process.argv.slice(2);
const port = 9333;
const dir = new URL("../../target/old-effects-chrome/", import.meta.url).pathname;
mkdirSync(dir, { recursive: true });
const chrome = spawn('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', [
  '--headless=new', `--remote-debugging-port=${port}`, `--user-data-dir=${dir}`,
  '--window-size=1920,1080', '--enable-gpu', '--ignore-gpu-blocklist', '--use-angle=metal',
  'about:blank',
], { stdio: 'ignore' });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let target;
for (let i = 0; i < 50 && !target; i++) {
  try {
    target = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: 'PUT' })).json();
  } catch { await sleep(200); }
}
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener('open', r, { once: true }));
let id = 0; const pending = new Map();
ws.addEventListener('message', (m) => { const d = JSON.parse(m.data); if (pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } });
const send = (method, params = {}) => new Promise((r) => { const n = ++id; pending.set(n, r); ws.send(JSON.stringify({ id: n, method, params })); });
const evaluate = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (r.result?.exceptionDetails) throw new Error(JSON.stringify(r.result.exceptionDetails).slice(0, 2000));
  return r.result?.result?.value;
};
await send('Emulation.setDeviceMetricsOverride', { width: 1920, height: 1080, deviceScaleFactor: 1, mobile: false });
await send('Page.navigate', { url: `http://127.0.0.1:5173/reference-stage.html?map=${map}&x=${x}&z=${z}&zoom=${zoom}&yaw=0&pitch=56&hour=12` });
for (let i = 0; i < 600; i++) {
  await sleep(500);
  if (await evaluate("document.body?.dataset?.ready === 'true'").catch(() => false)) break;
}
const specs = effects.map((e) => { const [id, age = '10', radius] = e.split(','); return { id, age: Number(age), radius: radius ? Number(radius) : undefined }; });
const png = await evaluate(`(async () => {
  const { Renderer } = await import('/src/render/renderer/renderer.ts');
  const present = Renderer.prototype.present;
  Renderer.prototype.present = function (...a) { window.__r = this; return present.apply(this, a); };
  for (let i = 0; i < 100 && !window.__r; i++) await new Promise((r) => setTimeout(r, 50));
  const r = window.__r;
  const { EffectPlayer } = await import('/src/render/abilities/effectPlayer.ts');
  const { coreEffects } = await import('/src/content/effects/library.ts');
  const player = new EffectPlayer();
  r.scene.add(player.root);
  const specs = ${JSON.stringify(specs)};
  const T = 0, n = specs.length, fx = ${Number(x)}, fz = ${Number(z)};
  specs.forEach((s, i) => {
    const effect = coreEffects.find((e) => e.id === s.id);
    if (!effect) throw new Error('no effect ' + s.id);
    const ex = fx + (i - (n - 1) / 2) * 8, h = r.height.walkSample(ex, fz);
    player.play(effect, { tick: T - s.age, seed: 1 + i, target: { x: ex, y: fz, height: h }, source: { x: ex - 6, y: fz, height: h + 1 }, radius: s.radius });
  });
  player.update(T);
  await player.ready();
  player.update(T);
  window.__player = player;
  const canvas = r.capture(1920, 16 / 9, 12);
  return canvas.toDataURL('image/png').split(',')[1];
})()`);
writeFileSync(out, Buffer.from(png, 'base64'));
console.log('wrote', out);
ws.close(); chrome.kill();

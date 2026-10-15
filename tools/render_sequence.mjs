// Load a scene into `lunaris serve`, advance it, and save a PNG at each requested tick. Also prints the state hash per frame.
// usage: node tools/render_sequence.mjs <scene.json> <ticks,comma,separated> [width] [height] [outdir] [--eye=x,y,z --target=x,y,z]
import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import readline from 'node:readline';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(here, '..');
const positional = process.argv.slice(2).filter((a) => !a.startsWith('--'));
const flags = Object.fromEntries(process.argv.slice(2).filter((a) => a.startsWith('--')).map((a) => a.slice(2).split('=')));
const [scene, tickList, w = '480', h = '270', outdir = path.join(root, 'target', 'renders')] = positional;
const vec = (v) => (v ? v.split(',').map(Number) : undefined);
if (!scene || !tickList) {
	console.error('usage: node tools/render_sequence.mjs <scene.json> <ticks,...> [width] [height] [outdir]');
	process.exit(2);
}
mkdirSync(outdir, { recursive: true });
const exe = path.join(root, 'target', 'release', 'lunaris.exe');
const child = spawn(exe, ['serve', '--root', root], { stdio: ['pipe', 'pipe', 'inherit'] });
const rl = readline.createInterface({ input: child.stdout });
const waiting = new Map();
rl.on('line', (line) => { const m = JSON.parse(line); const f = waiting.get(m.id); if (f) { waiting.delete(m.id); f(m); } });
let id = 1;
const call = (name, args = {}) => new Promise((resolve, reject) => {
	const i = id++;
	waiting.set(i, (m) => (m.error ? reject(new Error(JSON.stringify(m.error))) : resolve(m.result)));
	child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: i, method: 'tools/call', params: { name, arguments: args } }) + '\n');
});
const text = (r) => JSON.parse(r.content.find((c) => c.type === 'text').text);

const base = path.basename(scene).replace(/\..*$/, '');
const loaded = text(await call('load_scene', { path: scene }));
console.log(`loaded ${scene}: ${loaded.entities} entities`);
let now = 0;
for (const t of tickList.split(',').map(Number)) {
	if (t > now) await call('step', { ticks: t - now });
	now = t;
	const r = await call('render', { width: Number(w), height: Number(h), supersample: 2, eye: vec(flags.eye), target: vec(flags.target) });
	const info = text(r);
	const png = Buffer.from(r.content.find((c) => c.type === 'image').data, 'base64');
	const file = path.join(outdir, `${base}_${String(t).padStart(4, '0')}.png`);
	writeFileSync(file, png);
	const sim = text(await call('hash'));
	console.log(`tick ${t}: ${file}  frame ${info.hash}  state ${sim.hash}  visible ${info.visible.map((v) => v.name ?? v.uid).join(', ')}`);
}
child.stdin.end();
child.kill();

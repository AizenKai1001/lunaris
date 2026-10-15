// End-to-end check of `lunaris serve`: starts the real binary and talks MCP to it over stdio, the way an AI client does.
// usage: node tools/mcp_smoke.mjs [path-to-lunaris.exe]
import { spawn } from 'node:child_process';
import { mkdtempSync, existsSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import readline from 'node:readline';

const here = path.dirname(fileURLToPath(import.meta.url));
const exe = process.argv[2] || path.join(here, '..', 'target', 'release', 'lunaris.exe');
if (!existsSync(exe)) {
	console.error('binary not found:', exe, '\nbuild it with: cargo build --release -p lunaris');
	process.exit(2);
}
const root = mkdtempSync(path.join(tmpdir(), 'lunaris-smoke-'));
const child = spawn(exe, ['serve', '--seed', '7', '--root', root], { stdio: ['pipe', 'pipe', 'pipe'] });
let stderr = '';
child.stderr.on('data', (d) => (stderr += d));
const rl = readline.createInterface({ input: child.stdout });
const waiting = new Map();
rl.on('line', (line) => {
	const msg = JSON.parse(line);
	const w = waiting.get(msg.id);
	if (w) { waiting.delete(msg.id); w(msg); }
});

let nextId = 1;
const rpc = (method, params) =>
	new Promise((resolve, reject) => {
		const id = nextId++;
		const timer = setTimeout(() => reject(new Error(`timeout waiting for ${method}`)), 15000);
		waiting.set(id, (m) => { clearTimeout(timer); resolve(m); });
		child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
	});
const notify = (method, params) => child.stdin.write(JSON.stringify({ jsonrpc: '2.0', method, params }) + '\n');
const tool = async (name, args = {}) => {
	const r = await rpc('tools/call', { name, arguments: args });
	const text = r.result.content[0].text;
	return { error: r.result.isError, body: JSON.parse(text) };
};

let failed = 0;
const check = (name, ok, detail = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  (' + detail + ')' : ''}`); if (!ok) failed++; };

try {
	const init = await rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'smoke', version: '0' } });
	check('initialize', init.result.serverInfo.name === 'lunaris', `protocol ${init.result.protocolVersion}`);
	notify('notifications/initialized');
	const list = await rpc('tools/list', {});
	check('tools/list', list.result.tools.length >= 20, `${list.result.tools.length} tools`);
	const advertised = new Set(list.result.tools.map((t) => t.name));
	const needed = ['define_system', 'remove_system', 'enable_system', 'systems', 'get_system', 'render', 'raycast', 'step'];
	check('graph-system tools are discoverable', needed.every((n) => advertised.has(n)), needed.filter((n) => !advertised.has(n)).join(', ') || 'all listed');

	const d = (await tool('describe')).body;
	check('describe', d.engine === 'lunaris' && d.entities === 0, `components: ${d.components.join(', ')}`);

	await tool('define_component', { name: 'Health', doc: 'hit points', fields: [{ name: 'hp', type: 'f32', default: 100 }, { name: 'team', type: 'str' }] });
	const sp = (await tool('spawn', { count: 1000, components: { Transform: {}, Velocity: { lin: [1, 0, 0] }, Wander: { strength: 2 }, Health: { team: 'red' } } })).body;
	check('spawn 1000', sp.count === 1000 && sp.first_uid === 1);
	await tool('set', { uid: 7, component: 'Health', value: { hp: 12 } });
	const q = (await tool('query', { where: [{ component: 'Health', field: 'hp', op: '<', value: 50 }], select: ['Health'] })).body;
	check('query by field value', q.total === 1 && q.entities[0].uid === 7);

	const err = await tool('add_component', { uid: 1, component: 'Helth' });
	check('typo gets a suggestion', err.error && /did you mean `Health`/.test(err.body.hint), err.body.hint?.slice(0, 60));

	await tool('snapshot', { name: 'start' });
	const a = (await tool('step', { ticks: 300 })).body;
	await tool('restore', { name: 'start' });
	const b = (await tool('step', { ticks: 300 })).body;
	check('restore + step reproduces the same hash', a.hash === b.hash, a.hash);

	await tool('restore', { name: 'start' });
	await tool('record_start');
	await tool('step', { ticks: 100 });
	await tool('despawn', { uid: 3 });
	await tool('set', { uid: 4, component: 'Velocity', value: { lin: [0, 5, 0] } });
	await tool('step', { ticks: 100 });
	const stop = (await tool('record_stop', { path: 'runs/smoke.recording.json' })).body;
	const rep = (await tool('replay', { path: 'runs/smoke.recording.json' })).body;
	check('recording replays to the same hash', rep.matches === true, `${stop.entries} commands, hash ${rep.got}`);
	check('recording file written inside the workspace', existsSync(path.join(root, 'runs', 'smoke.recording.json')) && JSON.parse(readFileSync(path.join(root, 'runs', 'smoke.recording.json'), 'utf8')).lunaris_recording === 1);

	const esc = await tool('save_scene', { path: '../outside.json' });
	check('paths cannot leave the workspace', esc.error && esc.body.code === 'bad_path');

	await tool('save_scene', { path: 'scenes/smoke.lunaris.json' });
	const hBefore = (await tool('hash')).body.hash;
	await tool('reset');
	await tool('define_component', { name: 'Health', fields: [{ name: 'hp', type: 'f32', default: 100 }, { name: 'team', type: 'str' }] });
	const loaded = (await tool('load_scene', { path: 'scenes/smoke.lunaris.json' })).body;
	check('scene round-trips through a text file', loaded.entities === 999, `${loaded.entities} entities`);
	void hBefore;

	// the agent can see: render returns an MCP image content block that really is a PNG
	await tool('reset');
	await tool('spawn', { components: { Name: { value: 'Eye' }, Transform: { pos: [0, 1, 6] }, Camera: {} } });
	await tool('spawn', { components: { Name: { value: 'Ball' }, Transform: {}, MeshRenderer: { mesh: 'sphere', color: [0.9, 0.3, 0.2, 1] } } });
	const shot = await rpc('tools/call', { name: 'render', arguments: { width: 320, height: 180 } });
	const parts = shot.result.content;
	const png = Buffer.from(parts[0].data, 'base64');
	check('render returns an image content block', parts[0].type === 'image' && png.subarray(1, 4).toString() === 'PNG', `${png.length} bytes`);
	const info = JSON.parse(parts[1].text);
	check('render lists what is visible', info.visible.length === 1 && info.visible[0].name === 'Ball', `${info.visible[0]?.pixels} px, hash ${info.hash}`);
	const again = JSON.parse((await rpc('tools/call', { name: 'render', arguments: { width: 320, height: 180, image: false } })).result.content[0].text);
	check('same world renders to the same pixels', again.hash === info.hash);

	// behaviour as data: define a graph system, run it, read the result
	await tool('reset');
	await tool('spawn', { components: { Transform: { pos: [0, 10, 0] }, Velocity: {} } });
	const badGraph = await tool('define_system', { name: 'g', each: ['Transform', 'Velocity'], do: [{ set: 'Velocity.lin[1]', to: { subb: [1, 2] } }] });
	check('a bad graph is rejected with a path and hint', badGraph.error && badGraph.body.path === 'do[0].to.subb' && /sub/.test(badGraph.body.hint));
	await tool('define_system', { name: 'gravity', each: ['Transform', 'Velocity'], do: [{ set: 'Velocity.lin[1]', to: { sub: [{ get: 'Velocity.lin[1]' }, { mul: [9.8, { dt: true }] }] } }] });
	await tool('step', { ticks: 60 });
	const fell = (await tool('get', { uid: 1, components: ['Velocity'] })).body.components.Velocity.lin[1];
	check('graph system ran (one second of gravity)', Math.abs(fell + 9.8) < 0.01, `vy = ${fell.toFixed(3)}`);
	const sysList = (await tool('systems')).body;
	check('systems lists it with node count', sysList.graph[0].name === 'gravity' && sysList.graph[0].nodes >= 5);

	// the world pushes back, and the agent can ask what touched what
	await tool('reset');
	await tool('spawn', { components: { Name: { value: 'Floor' }, Transform: { pos: [0, -0.5, 0] }, Collider: { shape: 1, size: [20, 1, 20] } } });
	await tool('spawn', { components: { Name: { value: 'Ball' }, Transform: { pos: [0, 3, 0] }, Velocity: {}, Collider: { shape: 0 }, RigidBody: { restitution: 0.2 } } });
	await tool('step', { ticks: 240 });
	const ballY = (await tool('get', { uid: 2, components: ['Transform'] })).body.components.Transform.pos[1];
	check('physics: the ball comes to rest on the floor', Math.abs(ballY - 0.5) < 0.03, `y = ${ballY.toFixed(3)}`);
	const touching = (await tool('contacts')).body;
	check('contacts name the pair', touching.contacts[0].a_name === 'Floor' && touching.contacts[0].b_name === 'Ball');
	const ray = (await tool('raycast', { origin: [0, 5, 0], direction: [0, -1, 0] })).body;
	check('raycast finds the ball first', ray.hit && ray.name === 'Ball' && Math.abs(ray.distance - 4.0) < 0.05, `distance ${ray.distance?.toFixed(3)}`);

	const bad = await rpc('lunaris.step', { ticks: 'many' });
	check('bad parameter gives a JSON-RPC error with data', bad.error && bad.error.data.code === 'type_mismatch');
} catch (e) {
	console.log('ERROR', e.message);
	failed++;
}
child.stdin.end();
await new Promise((r) => setTimeout(r, 200));
child.kill();
console.log(failed ? `\n${failed} check(s) FAILED\n${stderr}` : '\nall checks passed');
process.exit(failed ? 1 : 0);

// End-to-end check of the browser editor: starts `lunaris edit`, drives the real page in headless Chrome over the DevTools
// protocol (Node 22+, no packages), and checks what a person would do: select by clicking the view, edit a field, play,
// edit graph systems (menus, text, Apply, engine errors), read the tool-call log.
//
//   node tools/editor_smoke.mjs            (needs target/release/lunaris.exe and Chrome or Edge)
//   CHROME=path/to/chrome.exe node tools/editor_smoke.mjs
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const exe = join(root, 'target', 'release', process.platform === 'win32' ? 'lunaris.exe' : 'lunaris');
const chromePath = [process.env.CHROME, 'C:/Program Files/Google/Chrome/Application/chrome.exe', 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', '/usr/bin/google-chrome', '/usr/bin/chromium'].find((p) => p && existsSync(p));
if (!existsSync(exe)) throw new Error(`build first: cargo build --release -p lunaris (missing ${exe})`);
if (!chromePath) throw new Error('no Chrome or Edge found; set CHROME=path');

let failures = 0;
const check = (name, ok, detail = '') => {
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  (' + detail + ')' : ''}`);
  if (!ok) failures++;
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// --- the editor server
const server = spawn(exe, ['edit', 'examples/bouncing.lunaris.json', '--port', '0'], { cwd: root });
let serverOut = '';
server.stdout.on('data', (d) => (serverOut += d));
server.stderr.on('data', (d) => (serverOut += d));
for (let i = 0; i < 100 && !/Lunaris editor: (\S+)/.test(serverOut); i++) await sleep(100);
const url = (serverOut.match(/Lunaris editor: (\S+)/) || [])[1];
if (!url) throw new Error('the editor did not start: ' + serverOut);

// --- headless Chrome with a DevTools port
const debugPort = 9300 + Math.floor(Math.random() * 500);
const profile = mkdtempSync(join(tmpdir(), 'lunaris-editor-smoke-'));
const chrome = spawn(chromePath, ['--headless=new', '--disable-gpu', `--remote-debugging-port=${debugPort}`, `--user-data-dir=${profile}`, '--window-size=1500,900', 'about:blank'], { stdio: 'ignore' });

function cleanup() {
  try { chrome.kill(); } catch {}
  try { server.kill(); } catch {}
  setTimeout(() => { try { rmSync(profile, { recursive: true, force: true }); } catch {} }, 1500);
}

try {
  let target;
  for (let i = 0; i < 100 && !target; i++) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${debugPort}/json`)).json();
      target = list.find((t) => t.type === 'page');
    } catch {}
    if (!target) await sleep(100);
  }
  if (!target) throw new Error('Chrome did not expose a page');
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
  let id = 0;
  const pending = new Map();
  ws.onmessage = (m) => { const j = JSON.parse(m.data); if (j.id && pending.has(j.id)) { pending.get(j.id)(j); pending.delete(j.id); } };
  const cdp = (method, params = {}) => new Promise((res) => { const i = ++id; pending.set(i, res); ws.send(JSON.stringify({ id: i, method, params })); });
  const evaluate = async (expr) => {
    const r = await cdp('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
    if (r.result.exceptionDetails) throw new Error('page error: ' + JSON.stringify(r.result.exceptionDetails.exception || r.result.exceptionDetails));
    return r.result.result.value;
  };
  await cdp('Page.enable');
  await cdp('Emulation.setDeviceMetricsOverride', { width: 1500, height: 900, deviceScaleFactor: 1, mobile: false });
  await cdp('Page.navigate', { url });
  for (let i = 0; i < 100; i++) { if (await evaluate('typeof S !== "undefined" && S.frame && S.frame.hash ? 1 : 0')) break; await sleep(100); }

  const r = await evaluate(`(async () => {
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    const out = {};
    const errors = [];
    window.addEventListener('error', (e) => errors.push(e.message));
    let syncCount = 0;
    const origSync = syncExternal;
    syncExternal = async () => { syncCount++; return origSync(); };
    out.entityRows = document.querySelectorAll('#ents .ent').length;
    out.rendered = !!S.frame.hash && document.querySelector('#frame').src.startsWith('data:image/png');
    out.scenePath = document.querySelector('#scenePath').value;

    // 1. pick by clicking the cube in the view
    const cube = S.frame.visible.find((v) => v.name === 'Cube');
    const vp = document.querySelector('#viewport').getBoundingClientRect();
    const cx = vp.left + ((cube.bbox[0] + cube.bbox[2]) / 2 / S.frame.w) * vp.width;
    const cy = vp.top + ((cube.bbox[1] + cube.bbox[3]) / 2 / S.frame.h) * vp.height;
    document.querySelector('#viewport').dispatchEvent(new MouseEvent('mousedown', { clientX: cx, clientY: cy, button: 0, bubbles: true }));
    window.dispatchEvent(new MouseEvent('mouseup', { clientX: cx, clientY: cy, button: 0, bubbles: true }));
    await sleep(500);
    out.pickedUid = S.sel;

    // 1b. drag the selected cube across the floor with the mouse: it moves in a horizontal plane and the whole drag is one undo step
    const posOf = async () => (await tool('get', { uid: 12, components: ['Transform'] }, { quiet: true })).components.Transform.pos;
    const dragBefore = await posOf();
    const vis = S.frame.visible.find((v) => v.uid === 12);
    const r0 = document.querySelector('#viewport').getBoundingClientRect();
    const gx = r0.left + (((vis.bbox[0] + vis.bbox[2]) / 2) / S.frame.w) * r0.width;
    const gy = r0.top + (((vis.bbox[1] + vis.bbox[3]) / 2) / S.frame.h) * r0.height;
    document.querySelector('#viewport').dispatchEvent(new MouseEvent('mousedown', { clientX: gx, clientY: gy, button: 0, bubbles: true }));
    await sleep(150);
    for (let k = 1; k <= 6; k++) {
      window.dispatchEvent(new MouseEvent('mousemove', { clientX: gx + 14 * k, clientY: gy + 3 * k, bubbles: true }));
      await sleep(90);
    }
    window.dispatchEvent(new MouseEvent('mouseup', { clientX: gx + 84, clientY: gy + 18, button: 0, bubbles: true }));
    await sleep(300);
    const dragAfter = await posOf();
    out.dragDx = dragAfter[0] - dragBefore[0];
    out.dragDz = dragAfter[2] - dragBefore[2];
    out.dragDy = dragAfter[1] - dragBefore[1];
    out.stillSelected = S.sel;
    await tool('undo');
    const dragBack = await posOf();
    out.dragUndone = Math.abs(dragBack[0] - dragBefore[0]) < 1e-4 && Math.abs(dragBack[2] - dragBefore[2]) < 1e-4;
    S.dirty = true;

    // 2. edit a field in the inspector
    const inp = document.querySelector('input[data-c="Transform"][data-f="pos"][data-i="1"]');
    inp.value = '4';
    inp.dispatchEvent(new Event('change', { bubbles: true }));
    await sleep(500);
    const g = await tool('get', { uid: S.sel }, { quiet: true });
    out.posY = g.components.Transform.pos[1];

    // 3. play for a second
    const t0 = S.tick;
    document.querySelector('#bPlay').click();
    await sleep(1000);
    document.querySelector('#bPlay').click();
    await sleep(200);
    out.ticksPlayed = S.tick - t0;

    // 4. graph systems: a bad edit shows the engine's error with a path
    document.querySelector('[data-sys="gravity"]').click();
    await sleep(400);
    document.querySelector('#sysjson').value = document.querySelector('#sysjson').value.replace('"sub"', '"subb"');
    document.querySelector('#bApply').click();
    await sleep(400);
    out.badEdit = document.querySelector('#syserr').textContent;
    document.querySelector('#bRevert').click();
    await sleep(200);

    // 5. change a constant in the graph and apply it
    window.prompt = () => '12.5';
    const c = [...document.querySelectorAll('#gview .node.const')].find((n) => n.textContent.includes('9.8'));
    const p = JSON.parse(c.getAttribute('data-path'));
    editConst(p);
    document.querySelector('#bApply').click();
    await sleep(400);
    out.constApplied = JSON.stringify((await tool('get_system', { name: 'gravity' }, { quiet: true })).def).includes('12.5');
    await tool('undo');
    out.constUndone = JSON.stringify((await tool('get_system', { name: 'gravity' }, { quiet: true })).def).includes('9.8');

    // 6. build a system from menus
    document.querySelector('#bSysNew').click();
    await sleep(300);
    const rc = (el) => { const b = el.getBoundingClientRect(); el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: b.left + 10, clientY: b.top + 10, button: 2 })); };
    const pick = async (label) => { const it = [...document.querySelectorAll('#ctx .mi')].find((x) => x.textContent === label); if (!it) throw new Error('no menu item ' + label); it.click(); await sleep(120); };
    rc(document.querySelector('#gview .node.event'));
    await pick('Add statement at the end…'); await pick('If');
    rc(document.querySelector('#gview .node.ctrl'));
    await pick('Add statement to "then"…'); await pick('Set a field');
    const leaf = [...document.querySelectorAll('#gview .node.const')].find((n) => JSON.parse(n.getAttribute('data-path')).join('.') === 'do.1.then.0.to');
    rc(leaf);
    await pick('mul');
    document.querySelector('#bApply').click();
    await sleep(500);
    out.menuApplyErr = document.querySelector('#syserr').textContent;
    const made = await tool('get_system', { name: 'my_system' }, { quiet: true });
    out.menuSystem = JSON.stringify(made.def.do);
    document.querySelector('#bGUndo').click();

    // 7. play a real game from the keyboard: the page forwards keys as input tool calls while playing
    await tool('load_scene', { path: 'examples/orb_dash.lunaris.json' });
    S.sel = null;
    await refreshAll(true);
    await sleep(300);
    const x0 = (await tool('resources', {}, { quiet: true })).resources['player.x'] || 0;
    document.querySelector('#bPlay').click();
    await sleep(300);
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'd', bubbles: true }));
    await sleep(800);
    const heldDuring = (await tool('resources', {}, { quiet: true })).resources['input.d'];
    window.dispatchEvent(new KeyboardEvent('keyup', { key: 'd', bubbles: true }));
    await sleep(200);
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await sleep(300);
    const after = (await tool('resources', {}, { quiet: true })).resources;
    out.playing = S.playing;
    out.keyHeld = heldDuring;
    out.keyReleased = after['input.d'];
    out.movedX = (after['player.x'] || 0) - x0;
    out.hud = S.frame.visible.filter((v) => v.name === 'Score' || v.name === 'Time').length;
    out.soundNames = Object.keys(SFX).sort().join(',');
    handleEvents([{ name: 'coin' }, { name: 'coin' }]);
    out.eventCount = S.eventCount;

    // 7b. another client (an AI agent, a script) edits the world without going through this page: the page notices and refreshes
    const rowsBefore = document.querySelectorAll('#ents .ent').length;
    await (await fetch('/rpc', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Lunaris-Token': TOKEN }, body: JSON.stringify({ jsonrpc: '2.0', id: 9001, method: 'lunaris.spawn', params: { components: { Name: { value: 'Visitor' }, Transform: {} } } }) })).json();
    for (let k = 0; k < 20 && ![...document.querySelectorAll('#ents .nm')].some((n) => n.textContent === 'Visitor'); k++) await sleep(150);
    out.visitorListed = [...document.querySelectorAll('#ents .nm')].some((n) => n.textContent === 'Visitor');
    out.visitorRows = document.querySelectorAll('#ents .ent').length - rowsBefore;
    out.visitorStatus = document.querySelector('#status').textContent;
    await sleep(1200);
    out.syncCount = syncCount;

    // 7c. the + menu adds a kind of entity; Attach... parents it without moving it; the list nests it; the parent drags it along
    const countBefore = S.total;
    document.querySelector('#bNew').click();
    await sleep(150);
    const sphereItem = [...document.querySelectorAll('#ctx .mi')].find((x) => x.textContent === 'Sphere');
    out.menuKinds = [...document.querySelectorAll('#ctx .mi')].map((x) => x.textContent).join(',');
    sphereItem.click();
    await sleep(600);
    const childUid = S.sel;
    out.newCount = S.total - countBefore;
    const posBefore = (await tool('get', { uid: childUid, components: ['Transform'] }, { quiet: true })).components.Transform.pos;
    const visitor = S.ents.find((e) => e.name === 'Visitor');
    window.prompt = () => String(visitor.uid);
    document.querySelector('#bAttach').click();
    await sleep(800);
    const attached = (await tool('get', { uid: childUid, components: ['Transform', 'Parent'] }, { quiet: true })).components;
    out.attachedTo = attached.Parent && attached.Parent.target;
    out.visitorUid = visitor.uid;
    out.attachJump = Math.max(...attached.Transform.pos.map((v, i) => Math.abs(v - posBefore[i])));
    out.treeDepth = (S.ents.find((e) => e.uid === childUid) || {}).depth;
    out.indentedRow = !!document.querySelector('.ent[data-uid="' + childUid + '"] .nm[data-depth="1"]');
    await tool('set', { uid: visitor.uid, component: 'Transform', value: { pos: [5, 0, 0] } });
    const moved = (await tool('get', { uid: childUid, components: ['Transform'] }, { quiet: true })).components.Transform.pos;
    out.followDx = moved[0] - posBefore[0];
    window.prompt = () => '0';
    document.querySelector('#bAttach').click();
    await sleep(800);
    const detached = (await tool('get', { uid: childUid }, { quiet: true })).components;
    out.detached = !detached.Parent && !detached.LocalTransform;
    out.stayedPut = Math.abs(detached.Transform.pos[0] - moved[0]) < 1e-4;

    // 7d. the Assets tab lists the workspace files; clicking a model sets it on the selected mesh
    document.querySelector('[data-tab="assets"]').click();
    await sleep(500);
    out.assetRows = document.querySelectorAll('#assetlist .asset').length;
    out.assetKinds = [...new Set([...document.querySelectorAll('#assetlist .asset')].map((r) => r.dataset.kind))].sort().join(',');
    const modelRow = [...document.querySelectorAll('#assetlist .asset')].find((r) => r.dataset.kind === 'model');
    const meshHolder = (await tool('query', { with: ['MeshRenderer'], limit: 1 }, { quiet: true })).entities[0].uid;
    select(meshHolder);
    await sleep(300);
    modelRow.click();
    await sleep(600);
    out.modelSet = (await tool('get', { uid: meshHolder, components: ['MeshRenderer'] }, { quiet: true })).components.MeshRenderer.mesh === modelRow.dataset.path;
    out.modelPath = modelRow.dataset.path;
    document.querySelector('[data-tab="systems"]').click();

    // 8. the tool-call log shows what the page did
    document.querySelector('[data-tab="tools"]').click();
    await sleep(200);
    out.logText = document.querySelector('#log').textContent;
    out.errors = errors;
    return out;
  })()`);

  check('page loads and lists the 6 entities', r.entityRows === 6, `${r.entityRows} rows`);
  check('the viewport shows a rendered frame (PNG from the engine)', r.rendered);
  check('the scene path box shows the loaded scene', r.scenePath === 'examples/bouncing.lunaris.json', r.scenePath);
  check('clicking the cube in the view selects it', r.pickedUid === 12, `uid ${r.pickedUid}`);
  check('dragging the selection moves it in the floor plane', Math.abs(r.dragDx) + Math.abs(r.dragDz) > 0.3 && Math.abs(r.dragDy) < 1e-4 && r.stillSelected === 12, `moved dx ${Number(r.dragDx).toFixed(2)}, dz ${Number(r.dragDz).toFixed(2)}`);
  check('the whole drag is a single undo step', r.dragUndone);
  check('editing a field in the inspector changes the entity', r.posY === 4, `pos.y = ${r.posY}`);
  check('Play steps the simulation at about 60 ticks per second', r.ticksPlayed >= 25 && r.ticksPlayed <= 90, `${r.ticksPlayed} ticks in 1 s`);
  check('a bad graph edit is refused with a path and a hint', /do\[0\]\.to\.subb/.test(r.badEdit) && /hint:/.test(r.badEdit), r.badEdit.split('\n')[0]);
  check('changing a constant in the graph and applying it reaches the engine', r.constApplied);
  check('that edit is undoable like any other', r.constUndone);
  check('a system built from the menus is accepted', r.menuApplyErr === '' && /"mul":\[0,1\]/.test(r.menuSystem), r.menuSystem);
  check('keys go to the game while playing (input.d held, then released)', r.keyHeld === 1 && r.keyReleased === 0, `held ${r.keyHeld}, released ${r.keyReleased}`);
  check('the ball moved right and Escape paused the game', r.movedX > 1 && r.playing === false, `moved ${Number(r.movedX).toFixed(2)}`);
  check('the HUD text is part of the rendered view', r.hud === 2, `${r.hud} HUD entities visible`);
  check('Sound entities are loaded as effects and game events are handled', r.soundNames === 'coin,lose,win' && r.eventCount >= 2, `${r.soundNames}, ${r.eventCount} events`);
  check('the + button offers kinds of entity and adds the one you pick', /Cube.*Sphere.*light.*Camera.*Text/.test(r.menuKinds) && r.newCount === 1, `${r.newCount} new; menu: ${r.menuKinds}`);
  check('Attach... parents an entity without moving it', r.attachedTo === r.visitorUid && r.attachJump < 1e-4, `parent ${r.attachedTo}, jump ${r.attachJump}`);
  check('the entity list nests an attached entity under its parent', r.treeDepth === 1 && r.indentedRow);
  check('moving the parent drags the attached entity along', Math.abs(r.followDx - 5) < 1e-3, `dx ${Number(r.followDx).toFixed(3)}`);
  check('Attach with 0 detaches and leaves it where it is', r.detached && r.stayedPut);
  check('the Assets tab lists the workspace files by kind', r.assetRows > 10 && /model/.test(r.assetKinds) && /scene/.test(r.assetKinds) && /texture/.test(r.assetKinds), `${r.assetRows} files; kinds ${r.assetKinds}`);
  check('clicking a model asset puts it on the selected mesh', r.modelSet, r.modelPath);
  check('an edit made by another client shows up on the page by itself', r.visitorListed && r.visitorRows === 1, `${r.visitorRows} new row(s); status: ${r.visitorStatus}`);
  check('the page refreshes only for that outside edit, not for its own calls', r.syncCount === 1, `${r.syncCount} refresh(es)`);
  check('the tool-call log lists the calls the page made', /define_system/.test(r.logText) && /get_system/.test(r.logText) && /spawn|set/.test(r.logText));
  check('no script errors in the page', r.errors.length === 0, r.errors.join('; '));
  ws.close();
} catch (e) {
  failures++;
  console.log('FAIL  ' + (e.stack || e));
} finally {
  cleanup();
}
console.log(failures ? `\n${failures} check(s) failed` : '\nall checks passed');
process.exit(failures ? 1 : 0);

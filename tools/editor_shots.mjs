// Takes screenshots of the browser editor in headless Chrome (Node 22+, no packages) for docs/gallery.
//
//   node tools/editor_shots.mjs        (needs target/release/lunaris.exe and Chrome or Edge)
//
// editor-hierarchy.png: the Clockwork scene with attached entities nested in the list and a moon selected.
// editor-new-menu.png: the + menu of entity kinds.
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const exe = join(root, 'target', 'release', process.platform === 'win32' ? 'lunaris.exe' : 'lunaris');
const chromePath = [process.env.CHROME, 'C:/Program Files/Google/Chrome/Application/chrome.exe', 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', '/usr/bin/google-chrome', '/usr/bin/chromium'].find((p) => p && existsSync(p));
if (!existsSync(exe)) throw new Error(`build first (missing ${exe})`);
if (!chromePath) throw new Error('no Chrome or Edge found; set CHROME=path');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const server = spawn(exe, ['edit', 'examples/clockwork.lunaris.json', '--port', '0'], { cwd: root });
let out = '';
server.stdout.on('data', (d) => (out += d));
for (let i = 0; i < 100 && !/Lunaris editor: (\S+)/.test(out); i++) await sleep(100);
const url = (out.match(/Lunaris editor: (\S+)/) || [])[1];
if (!url) throw new Error('the editor did not start: ' + out);

const debugPort = 9800 + Math.floor(Math.random() * 400);
const profile = mkdtempSync(join(tmpdir(), 'lunaris-shots-'));
const chrome = spawn(chromePath, ['--headless=new', '--disable-gpu', `--remote-debugging-port=${debugPort}`, `--user-data-dir=${profile}`, '--window-size=1500,900', 'about:blank'], { stdio: 'ignore' });
try {
  let target;
  for (let i = 0; i < 100 && !target; i++) {
    try { target = (await (await fetch(`http://127.0.0.1:${debugPort}/json`)).json()).find((t) => t.type === 'page'); } catch {}
    if (!target) await sleep(100);
  }
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
  let id = 0;
  const pending = new Map();
  ws.onmessage = (m) => { const j = JSON.parse(m.data); if (j.id && pending.has(j.id)) { pending.get(j.id)(j); pending.delete(j.id); } };
  const cdp = (method, params = {}) => new Promise((res) => { const i = ++id; pending.set(i, res); ws.send(JSON.stringify({ id: i, method, params })); });
  const evaluate = async (expr) => (await cdp('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true })).result.result.value;
  const shot = async (name) => {
    const r = await cdp('Page.captureScreenshot', { format: 'png' });
    writeFileSync(join(root, 'docs', 'gallery', name), Buffer.from(r.result.data, 'base64'));
    console.log('wrote docs/gallery/' + name);
  };
  await cdp('Page.enable');
  await cdp('Emulation.setDeviceMetricsOverride', { width: 1500, height: 900, deviceScaleFactor: 1, mobile: false });
  await cdp('Page.navigate', { url });
  for (let i = 0; i < 100; i++) { if (await evaluate('typeof S !== "undefined" && S.frame && S.frame.hash ? 1 : 0')) break; await sleep(100); }
  // let the planets move a little, select a moon, put the camera on the scene's own view
  await evaluate(`(async () => { S.view.mode = 'scene'; document.querySelector('#vmode').value = 'scene'; await tool('step', { ticks: 400 }); await refreshAll(true); const moon = S.ents.find((e) => e.name === 'Moon'); select(moon.uid); S.dirty = true; })()`);
  await sleep(900);
  await shot('editor-hierarchy.png');
  await evaluate(`document.querySelector('#bNew').click()`);
  await sleep(300);
  await shot('editor-new-menu.png');
  ws.close();
} finally {
  try { chrome.kill(); } catch {}
  try { server.kill(); } catch {}
  setTimeout(() => { try { rmSync(profile, { recursive: true, force: true }); } catch {} }, 1500);
}

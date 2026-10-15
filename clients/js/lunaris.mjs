// Drive a Lunaris engine from JavaScript (Node 18+). No packages.
//
//   import { Engine } from './lunaris.mjs';
//   const eng = await Engine.start();                       // starts `lunaris serve` and talks JSON-RPC over its pipes
//   await eng.define_component({ name: 'Health', fields: [{ name: 'hp', type: 'f32', default: 100 }] });
//   await eng.spawn({ count: 3, components: { Transform: {}, Health: {} } });
//   const frame = await eng.render({ width: 320, height: 180 });   // frame.png is a Buffer with the PNG
//   await eng.close();
//
//
// Every tool is a method (`eng.tools` lists them). A refused call rejects with a LunarisError carrying `code`, `path`, `hint`.
import { spawn } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { createInterface } from 'node:readline';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export class LunarisError extends Error {
  constructor(data) {
    super(data.hint ? `${data.message} (hint: ${data.hint})` : data.message);
    this.name = 'LunarisError';
    this.code = data.code || '';
    this.path = data.path || '';
    this.hint = data.hint || '';
    this.data = data;
  }
}

function findExe(exe) {
  if (exe) return exe;
  if (process.env.LUNARIS_EXE) return process.env.LUNARIS_EXE;
  const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
  for (const name of ['lunaris.exe', 'lunaris']) {
    const p = join(repo, 'target', 'release', name);
    if (existsSync(p)) return p;
  }
  throw new Error('lunaris executable not found: build it (cargo build --release -p lunaris) or set LUNARIS_EXE');
}

export class Engine {
  static async start({ exe, root = '.', seed = 1, threads } = {}) {
    const args = ['serve', '--root', String(root), '--seed', String(seed)];
    if (threads) args.push('--threads', String(threads));
    return Engine.#withTools(new Engine(spawn(findExe(exe), args, { stdio: ['pipe', 'pipe', 'ignore'] })));
  }

  static async #withTools(e) {
    e.tools = Object.fromEntries((await e.rpc('tools/list', {})).tools.map((t) => [t.name, t]));
    // every tool is available as a method: eng.spawn({...})
    return new Proxy(e, {
      get: (target, prop) => (prop in target || typeof prop !== 'string' ? target[prop] : prop in target.tools ? (args = {}) => target.call(prop, args) : undefined),
    });
  }

  constructor(proc) {
    this.proc = proc;
    this.tools = {};
    this.nextId = 1;
    this.waiting = new Map();
    if (!proc) return;
    createInterface({ input: proc.stdout }).on('line', (line) => {
      if (!line.trim()) return;
      const reply = JSON.parse(line);
      const w = this.waiting.get(reply.id);
      if (!w) return;
      this.waiting.delete(reply.id);
      if (reply.error) w.reject(new LunarisError({ message: reply.error.message, ...(reply.error.data || {}) }));
      else w.resolve(reply.result);
    });
    proc.on('exit', () => {
      for (const w of this.waiting.values()) w.reject(new Error('the lunaris process exited'));
      this.waiting.clear();
    });
  }

  rpc(method, params) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.waiting.set(id, { resolve, reject });
      this.proc.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
    });
  }

  async call(tool, args = {}) {
    const result = await this.rpc('lunaris.' + tool, args);
    if (result && result.image && result.image.data) {
      const { data, ...image } = result.image;
      return { ...result, image, png: Buffer.from(data, 'base64') };
    }
    return result;
  }

  async close() {
    if (!this.proc) return;
    this.proc.stdin.end();
    await new Promise((r) => { this.proc.once('exit', r); setTimeout(() => { this.proc.kill(); r(); }, 3000); });
  }
}

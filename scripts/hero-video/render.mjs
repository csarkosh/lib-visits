// Renders scripts/hero-video/hero.html frame by frame into an H.264 clip: headless Chrome at
// 1440 by 800 (two pixels a point: 2880 by 1600), render(t) for each frame, a screenshot of each
// piped into ffmpeg. Every frame is drawn for its exact time, so the clip is the same every run
// and never drops a frame. scripts/hero-video.sh runs it; Node 22 (its WebSocket), Chrome and
// ffmpeg are all it needs.
//
//   node scripts/hero-video/render.mjs <out.mp4> [--poster out.jpg] [--fps 30] [--preview]
//
// --preview draws a quarter of the size at 15 fps, to look in seconds.
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const flag = (name) => { const i = args.indexOf(name); return i < 0 ? null : args.splice(i, 2)[1]; };
const preview = args.includes('--preview') && args.splice(args.indexOf('--preview'), 1);
const poster = flag('--poster');
const fps = Number(flag('--fps') ?? (preview ? 15 : 30));
const out = args[0];
if (!out) { console.error('usage: render.mjs <out.mp4> [--poster out.jpg] [--fps 30] [--preview]'); process.exit(2); }
const chrome = process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const scale = preview ? 0.5 : 2;
const [W, H] = [1440, 800];

const profile = mkdtempSync(path.join(tmpdir(), 'hero-video-'));
const browser = spawn(chrome, ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, '--hide-scrollbars', '--allow-file-access-from-files', '--force-color-profile=srgb', 'about:blank'], { stdio: 'ignore' });
// Chrome is ended and waited for before its profile goes, or it is still writing into it.
const done = async (code) => {
  if (browser.exitCode === null) {
    const gone = new Promise((r) => browser.once('exit', r));
    try { browser.kill(); } catch {}
    await Promise.race([gone, new Promise((r) => setTimeout(r, 5000))]);
  }
  try { rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }); } catch {}
  process.exit(code);
};

// Chrome names its debugging port in the profile once it listens.
const portFile = path.join(profile, 'DevToolsActivePort');
for (let i = 0; i < 100 && !existsSync(portFile); i++) await new Promise((r) => setTimeout(r, 100));
if (!existsSync(portFile)) { console.error('render.mjs: Chrome did not start'); await done(1); }
const port = readFileSync(portFile, 'utf8').split('\n')[0];
const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
const page = targets.find((t) => t.type === 'page');
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
let id = 0;
const waiting = new Map();
const events = [];
ws.onmessage = (m) => {
  const msg = JSON.parse(m.data);
  if (msg.id && waiting.has(msg.id)) { const { r, j } = waiting.get(msg.id); waiting.delete(msg.id); msg.error ? j(new Error(msg.error.message)) : r(msg.result); }
  else if (msg.method) events.push(msg.method);
};
const send = (method, params = {}) => new Promise((r, j) => { const n = ++id; waiting.set(n, { r, j }); ws.send(JSON.stringify({ id: n, method, params })); });
const evaluate = async (expression) => {
  const res = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (res.exceptionDetails) throw new Error(res.exceptionDetails.exception?.description ?? res.exceptionDetails.text);
  return res.result.value;
};

await send('Page.enable');
await send('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: scale, mobile: false });
await send('Emulation.setDefaultBackgroundColorOverride', { color: { r: 7, g: 8, b: 11, a: 1 } });
await send('Page.navigate', { url: pathToFileURL(path.join(here, 'hero.html')).href });
for (let i = 0; i < 100 && !(await evaluate("document.body && document.body.dataset.ready === '1'").catch(() => false)); i++) await new Promise((r) => setTimeout(r, 100));
const duration = await evaluate('window.duration');
const n = Math.round(duration * fps);

const ffmpeg = spawn('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', '-f', 'image2pipe', '-c:v', 'png', '-framerate', String(fps), '-i', '-',
  '-vf', 'setparams=color_primaries=bt709:color_trc=iec61966-2-1:colorspace=bt709:range=tv,format=yuv420p',
  '-c:v', 'libx264', '-preset', preview ? 'veryfast' : 'slow', '-crf', '19', '-profile:v', 'high', '-movflags', '+faststart', '-an', out], { stdio: ['pipe', 'inherit', 'inherit'] });
let lastPng = null;
for (let i = 0; i < n; i++) {
  const t = i / fps;
  await evaluate(`(render(${t}), new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))))`);
  const shot = await send('Page.captureScreenshot', { format: 'png', fromSurface: true, captureBeyondViewport: false });
  lastPng = Buffer.from(shot.data, 'base64');
  if (!ffmpeg.stdin.write(lastPng)) await new Promise((r) => ffmpeg.stdin.once('drain', r));
  if (i % 30 === 0) process.stderr.write(`render: frame ${i} of ${n}\n`);
}
ffmpeg.stdin.end();
const code = await new Promise((r) => ffmpeg.on('close', r));
if (code !== 0) { console.error('render.mjs: ffmpeg failed'); await done(1); }
if (poster && lastPng) {
  const jpg = spawn('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', '-f', 'png_pipe', '-i', '-', '-q:v', '2', poster], { stdio: ['pipe', 'inherit', 'inherit'] });
  jpg.stdin.end(lastPng);
  await new Promise((r) => jpg.on('close', r));
}
console.log(`${out}: ${W * scale}x${H * scale}, ${n} frames at ${fps} fps (${duration} s)`);
ws.close();
await done(0);

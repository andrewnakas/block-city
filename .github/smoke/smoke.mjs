// Loads the deployed web build with a mode preset and checks it boots with those modes.
import { chromium } from 'playwright';

const url = process.env.URL;
const want = process.env.WANT; // e.g. "SKATE, PORTALS"
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
const logs = [];
page.on('console', (m) => logs.push(`[${m.type()}] ${m.text()}`));
page.on('response', (r) => { if (r.status() >= 400) logs.push(`[http ${r.status()}] ${r.url()}`); });
page.on('pageerror', (e) => logs.push(`[pageerror] ${e.message}`));
await page.goto(url, { timeout: 120000 });
let mounted = true;
try {
  await page.waitForFunction(() => !document.getElementById('loading'), null, { timeout: 240000 });
} catch { mounted = false; }
await page.waitForTimeout(8000);
// Software WebGL can keep the main thread busy; the screenshot is a nice-to-have.
try { await page.screenshot({ path: 'smoke.png', timeout: 60000, animations: 'allow', caret: 'initial' }); } catch (e) { logs.push(`[smoke] screenshot skipped: ${e.message.split('\n')[0]}`); }
await browser.close().catch(() => {});

const text = logs.join('\n');
console.log(text.split('\n').filter((l) => !/wgpu|naga/i.test(l)).slice(0, 80).join('\n'));
const fail = [];
if (!mounted) fail.push('first frame never drawn');
if (!text.includes(`start modes: ${want}`)) fail.push(`missing "start modes: ${want}"`);
const wantWorld = process.env.WANT_WORLD || 'City';
if (!text.includes(`world: ${wantWorld}`)) fail.push(`missing "world: ${wantWorld}"`);
if (/panicked|RuntimeError|unreachable/.test(text)) fail.push('wasm panic in console');
if (/\[http 4\d\d\]/.test(text)) fail.push('failed requests (see [http] lines)');
if (fail.length) { console.error('SMOKE FAIL: ' + fail.join('; ')); process.exit(1); }
console.log('SMOKE OK');

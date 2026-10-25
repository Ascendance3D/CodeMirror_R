const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

function resolvePlaywright() {
  for (const name of ['playwright', '@playwright/test']) {
    try { return require(name); } catch (e) {}
  }
  const cached = execSync(
    'ls -d ~/.npm-global/lib/node_modules/@playwright/cli/node_modules/playwright ' +
    '~/.npm/_npx/*/node_modules/playwright 2>/dev/null || true',
    { encoding: 'utf8', shell: '/bin/bash' }).trim().split('\n').filter(Boolean);
  for (const p of cached) {
    try { return require(p); } catch (e) {}
  }
  throw new Error('playwright not found');
}

function installedChromium() {
  const found = execSync(
    'ls -d ~/.cache/ms-playwright/chromium-*/chrome-linux*/chrome 2>/dev/null | sort -V | tail -1 || true',
    { encoding: 'utf8', shell: '/bin/bash' }).trim();
  return found || null;
}

(async () => {
  const { chromium } = resolvePlaywright();
  const targetDir = __dirname;
  const qaDir = path.join(targetDir, 'qa-renders');
  if (!fs.existsSync(qaDir)) fs.mkdirSync(qaDir, { recursive: true });

  const url = 'http://127.0.0.1:8099/tooling/x_ite-preview/preview.html?file=../../item-categories/antiques/chronos-orrery-v2/chronos-orrery-v2.wrl';
  
  const opts = { headless: true, args: ['--use-gl=swiftshader', '--enable-unsafe-swiftshader'] };
  const exe = installedChromium();
  if (exe) opts.executablePath = exe;

  const browser = await chromium.launch(opts);
  const page = await browser.newPage({ viewport: { width: 1000, height: 800 } });
  
  await page.goto(url, { waitUntil: 'networkidle' });
  await page.waitForFunction(() => {
    const c = document.querySelector('X3DCanvas');
    return !!(c && c.browser && c.browser.currentScene && c.browser.currentScene.rootNodes.length);
  }, null, { timeout: 30000 });

  await page.waitForTimeout(1500);

  // 1. Front 3/4
  await page.screenshot({ path: path.join(qaDir, 'front-34.png') });
  console.log('Captured front-34.png');

  await browser.close();
})().catch(e => { console.error('QA Render failed:', e); process.exit(1); });

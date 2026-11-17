/* Prints an HTML file to PDF with headless Chrome (page numbers in the footer). No packages needed.
   node print.cjs <in.html> <out.pdf> */
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const {spawn} = require('node:child_process');
const sleep = ms => new Promise(r => setTimeout(r, ms));

class CDP {
  constructor(ws) {
    this.ws = ws; this.next = 1; this.pending = new Map();
    ws.addEventListener('message', e => {
      const d = JSON.parse(e.data);
      if (!d.id) return;
      const p = this.pending.get(d.id); this.pending.delete(d.id);
      if (p) d.error ? p.reject(Error(JSON.stringify(d.error))) : p.resolve(d.result);
    });
  }
  static async connect(url) {
    const ws = new WebSocket(url);
    await new Promise((res, rej) => { ws.addEventListener('open', res, {once: true}); ws.addEventListener('error', rej, {once: true}); });
    return new CDP(ws);
  }
  send(method, params = {}) {
    const id = this.next++;
    return new Promise((res, rej) => { this.pending.set(id, {resolve: res, reject: rej}); this.ws.send(JSON.stringify({id, method, params})); });
  }
}

(async () => {
  const [input, output] = process.argv.slice(2).map(p => path.resolve(p));
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'oos-manual-'));
  const portFile = path.join(profile, 'DevToolsActivePort');
  const chrome = process.env.OOS_CHROME || 'C:/Program Files/Google/Chrome/Application/chrome.exe';
  const child = spawn(chrome, ['--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check', '--disable-extensions',
    '--disable-background-networking', '--remote-debugging-port=0', '--user-data-dir=' + profile, 'about:blank'], {windowsHide: true, stdio: 'ignore'});
  let browser, page;
  try {
    for (let n = 0; n < 150 && !fs.existsSync(portFile); n++) await sleep(100);
    const [port, wsPath] = fs.readFileSync(portFile, 'utf8').trim().split(/\r?\n/);
    browser = await CDP.connect(`ws://127.0.0.1:${port}${wsPath}`);
    const target = await browser.send('Target.createTarget', {url: 'about:blank'});
    const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
    page = await CDP.connect(targets.find(t => t.id === target.targetId).webSocketDebuggerUrl);
    await page.send('Page.enable');
    await page.send('Page.navigate', {url: 'file:///' + input.replaceAll('\\', '/')});
    for (let n = 0; n < 100; n++) {
      await sleep(100);
      const r = await page.send('Runtime.evaluate', {expression: 'document.readyState==="complete" && document.fonts.status==="loaded"', returnByValue: true});
      if (r.result.value) break;
    }
    const foot = `<div style="width:100%;font:7.5pt Arial,sans-serif;color:#555;padding:0 16mm;display:flex;justify-content:space-between">
      <span>OOS Plus Programming Manual</span><span><span class="pageNumber"></span></span></div>`;
    const pdf = await page.send('Page.printToPDF', {
      printBackground: true, preferCSSPageSize: true, displayHeaderFooter: true,
      headerTemplate: '<div></div>', footerTemplate: foot, generateDocumentOutline: false,
    });
    fs.writeFileSync(output, Buffer.from(pdf.data, 'base64'));
  } finally {
    if (browser) { await browser.send('Browser.close').catch(() => {}); browser.ws.close(); }
    child.kill();
    await sleep(300);
    fs.rmSync(profile, {recursive: true, force: true, maxRetries: 5});
  }
})().catch(e => { console.error(e); process.exit(1); });

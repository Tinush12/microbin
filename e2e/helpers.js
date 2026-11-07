const { spawn, execFileSync } = require('node:child_process');
const fs = require('node:fs');
const net = require('node:net');
const os = require('node:os');
const path = require('node:path');
const base = require('@playwright/test');

const DEFAULT_BINARY = path.join(__dirname, '..', 'target', 'release', 'microbin');
const START_TIMEOUT_MS = 10_000;
const START_POLL_MS = 50;

function freePort() {
  return new Promise((resolve, reject) => {
    const probe = net.createServer();
    probe.on('error', reject);
    probe.listen(0, '127.0.0.1', () => {
      const { port } = probe.address();
      probe.close(() => resolve(port));
    });
  });
}

async function waitUntilUp(url) {
  const deadline = Date.now() + START_TIMEOUT_MS;
  while (Date.now() < deadline) {
    try {
      if ((await fetch(url)).ok) return;
    } catch (_notUpYet) {
      // connection refused until the server has bound its port
    }
    await new Promise((resolve) => setTimeout(resolve, START_POLL_MS));
  }
  throw new Error(`server did not start at ${url}`);
}

async function postEntries(url, form) {
  const response = await fetch(`${url}/api/entries`, { method: 'POST', body: form });
  if (response.status !== 201) throw new Error(`seed failed: ${response.status} ${await response.text()}`);
  return (await response.json()).entries;
}

class Server {
  constructor(url, dataDir) {
    this.url = url;
    this.dataDir = dataDir;
  }

  async addText(text) {
    const form = new FormData();
    form.append('text', text);
    return (await postEntries(this.url, form))[0];
  }

  async addTexts(count, prefix = 'entry') {
    const added = [];
    for (let i = 0; i < count; i += 1) added.push(await this.addText(`${prefix} ${i}`));
    return added;
  }

  async addFile(name, content = 'data') {
    const form = new FormData();
    form.append('file', new Blob([content]), name);
    return (await postEntries(this.url, form))[0];
  }

  async remove(id) {
    await fetch(`${this.url}/api/entries/${id}`, { method: 'DELETE' });
  }

  async list() {
    return (await (await fetch(`${this.url}/api/entries`)).json()).entries;
  }

  setExpires(id, secs) {
    const db = path.join(this.dataDir, 'feed.sqlite');
    execFileSync('sqlite3', [db, `UPDATE entries SET expires=${Number(secs)} WHERE id=${Number(id)}`]);
  }
}

const test = base.test.extend({
  server: async ({}, use) => {
    const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'microbin-e2e-'));
    const port = await freePort();
    const binary = process.env.MICROBIN_BIN || DEFAULT_BINARY;
    const child = spawn(binary, ['--data-dir', dataDir, '--port', String(port), '--bind', '127.0.0.1'], {
      stdio: 'ignore',
    });
    const url = `http://127.0.0.1:${port}`;
    try {
      await waitUntilUp(`${url}/api/entries`);
      await use(new Server(url, dataDir));
    } finally {
      child.kill();
      fs.rmSync(dataDir, { recursive: true, force: true });
    }
  },
});

module.exports = { test, expect: base.expect };

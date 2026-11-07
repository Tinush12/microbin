const os = require('node:os');
const path = require('node:path');
const { defineConfig } = require('@playwright/test');

module.exports = defineConfig({
  testDir: __dirname,
  outputDir: path.join(os.tmpdir(), 'microbin-e2e-results'),
  fullyParallel: true,
  reporter: 'line',
  timeout: 30_000,
  use: { viewport: { width: 1100, height: 700 } },
});

const { test, expect } = require('./helpers');

const LIVE_TIMEOUT = { timeout: 3_000 };

async function setHidden(page, hidden) {
  await page.evaluate((isHidden) => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => (isHidden ? 'hidden' : 'visible') });
    document.dispatchEvent(new Event('visibilitychange'));
  }, hidden);
}

function action(row, name) {
  return row.getByRole('button', { name, exact: true });
}

test('an entry sent in one tab appears in another within 3 s', async ({ browser, server }) => {
  const a = await (await browser.newContext()).newPage();
  const b = await (await browser.newContext()).newPage();
  await a.goto(server.url);
  await b.goto(server.url);
  await a.locator('#input').fill('from tab a');
  await a.locator('#input').press('Enter');
  await expect(b.locator('.entry')).toContainText('from tab a', LIVE_TIMEOUT);
  await expect(b.locator('#count')).toHaveText('1', LIVE_TIMEOUT);
  await expect(a.locator('.entry')).toHaveCount(1);
});

test('a deleted entry disappears from the other tab', async ({ browser, server }) => {
  const entry = await server.addText('short lived');
  const a = await (await browser.newContext()).newPage();
  const b = await (await browser.newContext()).newPage();
  await a.goto(server.url);
  await b.goto(server.url);
  const row = a.locator(`#e-${entry.id}`);
  await row.hover();
  await action(row, 'Delete').click();
  await action(row, 'Yes').click();
  await expect(row).toHaveCount(0);
  await expect(b.locator(`#e-${entry.id}`)).toHaveCount(0, LIVE_TIMEOUT);
  await expect(a.locator('#count')).toHaveText('0', LIVE_TIMEOUT);
});

test('declining the delete keeps the entry', async ({ page, server }) => {
  const entry = await server.addText('keep');
  await page.goto(server.url);
  const row = page.locator(`#e-${entry.id}`);
  await row.hover();
  await action(row, 'Delete').click();
  await action(row, 'No').click();
  await expect(action(row, 'Delete')).toBeVisible();
  expect((await server.list()).length).toBe(1);
});

test('catching up after the tab was hidden adds no duplicates', async ({ page, server }) => {
  const first = await server.addText('first');
  await page.goto(server.url);
  await expect(page.locator('.entry')).toHaveCount(1);
  await setHidden(page, true);
  const survivor = await server.addText('survivor');
  const gone = await server.addText('gone');
  await server.remove(gone.id);
  await page.waitForTimeout(2_500);
  await expect(page.locator(`#e-${survivor.id}`)).toHaveCount(0);
  await setHidden(page, false);
  await expect(page.locator(`#e-${survivor.id}`)).toHaveCount(1, LIVE_TIMEOUT);
  await expect(page.locator(`#e-${gone.id}`)).toHaveCount(0);
  await expect(page.locator(`#e-${first.id}`)).toHaveCount(1);
  await expect(page.locator('.entry')).toHaveCount(2);
});

test('expire sets a marker, and never clears it', async ({ page, server }) => {
  const entry = await server.addText('x');
  await page.goto(server.url);
  const row = page.locator(`#e-${entry.id}`);
  await row.hover();
  await action(row, 'Expire').click();
  await action(row, '1 hour').click();
  await expect(row.locator('.meta')).toContainText(/expires in (59m|1h)/);
  await row.hover();
  await action(row, 'Expire').click();
  await action(row, 'Never').click();
  await expect(row.locator('.expiry-marker')).toHaveCount(0);
});

test('an expired entry leaves the open feed', async ({ page, server }) => {
  const entry = await server.addText('x');
  await page.goto(server.url);
  await expect(page.locator(`#e-${entry.id}`)).toHaveCount(1);
  server.setExpires(entry.id, Math.floor(Date.now() / 1000) - 1);
  await expect(page.locator(`#e-${entry.id}`)).toHaveCount(0, LIVE_TIMEOUT);
});

test('copy puts the raw markdown on the clipboard', async ({ page, context, server }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: server.url });
  const raw = '# Title\n**bold** and `code`';
  const entry = await server.addText(raw);
  await page.goto(server.url);
  const row = page.locator(`#e-${entry.id}`);
  await row.hover();
  await action(row, 'Copy').click();
  await expect(action(row, 'Copied')).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(raw);
  await expect(action(row, 'Copy')).toBeVisible({ timeout: 3_000 });
});

test('copy works without the clipboard api, as on plain http', async ({ page, server }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', { value: undefined });
    window.copied = [];
    document.execCommand = (command) => {
      if (command === 'copy') window.copied.push(document.activeElement.value);
      return true;
    };
  });
  const entry = await server.addText('raw *text*');
  await page.goto(server.url);
  const row = page.locator(`#e-${entry.id}`);
  await row.hover();
  await action(row, 'Copy').click();
  await expect(action(row, 'Copied')).toBeVisible();
  expect(await page.evaluate(() => window.copied)).toEqual(['raw *text*']);
});

test('each kind offers its own actions', async ({ page, server }) => {
  const text = await server.addText('plain');
  const link = await server.addText('https://example.com/a');
  const file = await server.addFile('a.txt');
  await page.goto(server.url);
  const names = async (id) => {
    const row = page.locator(`#e-${id}`);
    await row.hover();
    return row.locator('.actions button').allTextContents();
  };
  expect(await names(text.id)).toEqual(['Copy', 'QR', 'Expire', 'Delete']);
  expect(await names(link.id)).toEqual(['Copy', 'Open', 'QR', 'Short link', 'Expire', 'Delete']);
  expect(await names(file.id)).toEqual(['Download', 'Open', 'QR', 'Expire', 'Delete']);
});

test('short link copies the /u address', async ({ page, context, server }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: server.url });
  const link = await server.addText('https://example.com/a');
  await page.goto(server.url);
  const row = page.locator(`#e-${link.id}`);
  await row.hover();
  await action(row, 'Short link').click();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(`${server.url}/u/${link.id}`);
});

test('download fetches the file as an attachment', async ({ page, server }) => {
  const file = await server.addFile('a.txt', 'hello');
  await page.goto(server.url);
  const row = page.locator(`#e-${file.id}`);
  await row.hover();
  const download = page.waitForEvent('download');
  await action(row, 'Download').click();
  expect((await download).suggestedFilename()).toBe('a.txt');
});

test('keyboard focus reveals the actions', async ({ page, server }) => {
  const entry = await server.addText('x');
  await page.goto(server.url);
  const row = page.locator(`#e-${entry.id}`);
  await expect(row.locator('.actions')).toBeHidden();
  await row.focus();
  await expect(row.locator('.actions')).toBeVisible();
});

test('on a phone a tap toggles the actions', async ({ browser, server }) => {
  const entry = await server.addText('x');
  const context = await browser.newContext({ hasTouch: true, isMobile: true, viewport: { width: 390, height: 800 } });
  const page = await context.newPage();
  await page.goto(server.url);
  const row = page.locator(`#e-${entry.id}`);
  await row.locator('.body').tap();
  await expect(row).toHaveClass(/open/);
  await expect(action(row, 'Copy')).toBeVisible();
  await row.locator('.body').tap();
  await expect(row).not.toHaveClass(/open/);
  await context.close();
});

test('three failed polls show the offline marker until one succeeds', async ({ page, server }) => {
  await page.goto(server.url);
  await expect(page.locator('#offline')).toBeHidden();
  await page.route('**/api/changes**', (route) => route.abort());
  await expect(page.locator('#offline')).toBeVisible({ timeout: 8_000 });
  await page.unroute('**/api/changes**');
  await expect(page.locator('#offline')).toBeHidden({ timeout: 3_000 });
});

test('a live update leaves an open image row alone', async ({ page, server }) => {
  const file = await server.addFile('a.png');
  const other = await server.addText('other');
  await page.goto(server.url);
  await page.locator(`#e-${file.id}`).evaluate((el) => { el.dataset.mark = 'same-element'; });
  // Re-reported through the change log with nothing new about it.
  await page.evaluate(async () => {
    const { entries } = await (await fetch('/api/entries')).json();
    window.Feed.upsert(entries);
  });
  await expect(page.locator(`#e-${file.id}`)).toHaveAttribute('data-mark', 'same-element');
  await expect(page.locator(`#e-${other.id}`)).toHaveCount(1);
});

test('a poll that never answers still ends in the offline marker, then recovers', async ({ page, server }) => {
  test.setTimeout(60_000);
  await page.goto(server.url);
  await expect(page.locator('#offline')).toBeHidden();
  await page.route('**/api/changes**', () => { /* never answered, as on a dead network path */ });
  await expect(page.locator('#offline')).toBeVisible({ timeout: 35_000 });
  await page.unroute('**/api/changes**');
  const entry = await server.addText('after the outage');
  await expect(page.locator(`#e-${entry.id}`)).toHaveCount(1, { timeout: 10_000 });
  await expect(page.locator('#offline')).toBeHidden();
});

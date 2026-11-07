const { test, expect } = require('./helpers');

const atBottom = (feed) => feed.evaluate((el) => el.scrollHeight - el.scrollTop - el.clientHeight < 5);

test('opens at the newest entry with day dividers', async ({ page, server }) => {
  await server.addTexts(3);
  await page.goto(server.url);
  await expect(page.locator('.entry')).toHaveCount(3);
  await expect(page.locator('.entry').last()).toContainText('entry 2');
  await expect(page.locator('.day')).toHaveCount(1);
  await expect(page.locator('.day').first()).toHaveText('Today');
  expect(await atBottom(page.locator('#feed'))).toBe(true);
});

test('a long feed opens scrolled to the bottom', async ({ page, server }) => {
  await server.addTexts(60);
  await page.goto(server.url);
  await expect(page.locator('.entry')).toHaveCount(50);
  await expect(page.locator('.entry').last()).toBeInViewport();
  expect(await atBottom(page.locator('#feed'))).toBe(true);
});

test('scrolling up loads older entries in batches', async ({ page, server }) => {
  await server.addTexts(120);
  await page.goto(server.url);
  const feed = page.locator('#feed');
  await expect(page.locator('.entry')).toHaveCount(50);
  const oldest = page.locator('.entry').first();
  const oldestId = await oldest.getAttribute('id');
  await feed.evaluate((el) => { el.scrollTop = 0; });
  await expect(page.locator('.entry')).toHaveCount(100);
  // The row that was on top keeps its place on screen instead of jumping away.
  const row = page.locator(`#${oldestId}`);
  await expect(row).toBeInViewport();
  const top = await row.evaluate((el) => el.getBoundingClientRect().top);
  const feedTop = await feed.evaluate((el) => el.getBoundingClientRect().top);
  expect(top - feedTop).toBeLessThan(300);
  const ids = await page.locator('.entry').evaluateAll((els) => els.map((el) => Number(el.dataset.id)));
  expect(ids).toEqual([...ids].sort((a, b) => a - b));
  expect(new Set(ids).size).toBe(ids.length);
});

test('markdown renders and code is highlighted', async ({ page, server }) => {
  await server.addText('# Todo\n```bash\nls\n```');
  await page.goto(server.url);
  await expect(page.locator('.entry h1')).toHaveText('Todo');
  await expect(page.locator('.entry code.hljs')).toHaveCount(1);
});

test('entry body is the sanitized server html', async ({ page, server }) => {
  const dialogs = [];
  page.on('dialog', (dialog) => { dialogs.push(dialog.message()); dialog.dismiss(); });
  await server.addText('<img src=x onerror="alert(1)"> <script>alert(2)</script> done');
  await page.goto(server.url);
  await expect(page.locator('.entry')).toHaveCount(1);
  await page.waitForTimeout(200);
  expect(dialogs).toEqual([]);
});

test('phone width puts time above the entry', async ({ page, server }) => {
  await server.addText('hello');
  await page.setViewportSize({ width: 390, height: 800 });
  await page.goto(server.url);
  const meta = await page.locator('.entry .meta').boundingBox();
  const body = await page.locator('.entry .body').boundingBox();
  expect(meta.y).toBeLessThan(body.y);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  expect(overflow).toBeLessThanOrEqual(0);
});

test('an unreachable server shows an error instead of an empty feed', async ({ page, server }) => {
  await page.route('**/api/entries', (route) => route.abort());
  await page.goto(server.url);
  await expect(page.locator('#feed .load-error')).toBeVisible();
});

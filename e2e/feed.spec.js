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

// Draws a real PNG in the browser, so the bytes are what a screenshot tool would produce.
async function realPng(page, name) {
  return page.evaluateHandle(async (fileName) => {
    const canvas = document.createElement('canvas');
    canvas.width = 40;
    canvas.height = 30;
    const context = canvas.getContext('2d');
    context.fillStyle = '#c00';
    context.fillRect(0, 0, 40, 30);
    const blob = await new Promise((resolve) => canvas.toBlob(resolve, 'image/png'));
    return new File([blob], fileName, { type: 'image/png', lastModified: Date.now() });
  }, name);
}

test('a pasted image shows in the strip and decodes in the feed', async ({ page, server }) => {
  await page.goto(server.url);
  const file = await realPng(page, 'image.png');
  await page.evaluate((png) => {
    const transfer = new DataTransfer();
    transfer.items.add(png);
    document.dispatchEvent(new ClipboardEvent('paste', { clipboardData: transfer, bubbles: true, cancelable: true }));
  }, file);
  const decoded = (locator) => locator.evaluate((img) => img.decode().then(() => img.naturalWidth));
  expect(await decoded(page.locator('.chip img'))).toBe(40);
  await page.locator('#send').click();
  const shown = page.locator('.entry[data-kind="image"] img.shot');
  await expect(shown).toBeVisible();
  expect(await decoded(shown)).toBe(40);
  expect((await shown.boundingBox()).height).toBeGreaterThan(0);
});

const CONTRAST_SCRIPT = (root) => {
  const channels = (color) => color.match(/[\d.]+/g).slice(0, 3).map(Number);
  const luminance = (color) => {
    const [r, g, b] = channels(color).map((value) => {
      const c = value / 255;
      return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const ratio = (a, b) => {
    const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x);
    return (high + 0.05) / (low + 0.05);
  };
  const background = getComputedStyle(root.closest('pre')).backgroundColor;
  const tokens = [root, ...root.querySelectorAll('span')];
  return Math.min(...tokens.map((token) => ratio(getComputedStyle(token).color, background)));
};

for (const colorScheme of ['light', 'dark']) {
  test(`highlighted code is readable in ${colorScheme} mode`, async ({ browser, server }) => {
    await server.addText('```bash\n# comment\nfor f in *.png; do echo "$f" 42; done\n```');
    await server.addText('```rust\n/// doc\nfn main() { let x: u32 = 1; println!("{x}"); }\n```');
    await server.addText('```html\n<a href="x">y</a>\n```');
    const context = await browser.newContext({ colorScheme });
    const page = await context.newPage();
    await page.goto(server.url);
    await expect(page.locator('code.hljs')).toHaveCount(3);
    for (const code of await page.locator('code.hljs').all()) {
      expect(await code.evaluate(CONTRAST_SCRIPT)).toBeGreaterThanOrEqual(4.5);
    }
    await context.close();
  });
}

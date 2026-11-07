const { test, expect } = require('./helpers');

async function openSearch(page, query) {
  await page.locator('#search-toggle').click();
  await page.locator('#search-input').fill(query);
}

test('search matches text, link and file name', async ({ page, server }) => {
  await server.addText('Jellyfin notes');
  await server.addText('https://example.com/unique-path');
  await server.addFile('NAS-backup.tar.gz');
  await server.addTexts(3, 'noise');
  await page.goto(server.url);
  await expect(page.locator('#search-input')).toBeHidden();
  await openSearch(page, 'jellyfin');
  await expect(page.locator('#results .entry')).toHaveCount(1);
  await expect(page.locator('#results .entry')).toContainText('Jellyfin notes');
  await page.locator('#search-input').fill('unique-path');
  await expect(page.locator('#results .entry')).toHaveCount(1);
  await expect(page.locator('#results .entry')).toHaveAttribute('data-kind', 'link');
  await page.locator('#search-input').fill('nas-BACKUP');
  await expect(page.locator('#results .entry')).toHaveCount(1);
  await expect(page.locator('#results .entry')).toHaveAttribute('data-kind', 'file');
});

test('results are newest first and show the date', async ({ page, server }) => {
  const added = await server.addTexts(3, 'match');
  await page.goto(server.url);
  await openSearch(page, 'match');
  await expect(page.locator('#results .entry')).toHaveCount(3);
  const ids = await page.locator('#results .entry').evaluateAll((els) => els.map((el) => Number(el.dataset.id)));
  expect(ids).toEqual(added.map((entry) => entry.id).reverse());
  await expect(page.locator('#results .entry .result-date').first()).toHaveText('Today');
});

test('no match says so, and a blank query goes back to the feed', async ({ page, server }) => {
  await server.addText('hello');
  await page.goto(server.url);
  await openSearch(page, 'zzz');
  await expect(page.locator('#results')).toContainText('No matches');
  await page.locator('#search-input').fill('   ');
  await expect(page.locator('#results')).toBeHidden();
  await expect(page.locator('#feed .entry')).toBeVisible();
});

test('a slow old answer does not replace a newer one', async ({ page, server }) => {
  await server.addText('alpha');
  await server.addText('beta');
  await page.goto(server.url);
  await page.route('**/api/search?q=alpha', async (route) => {
    await new Promise((resolve) => setTimeout(resolve, 800));
    await route.continue();
  });
  await openSearch(page, 'alpha');
  await page.waitForTimeout(350);
  await page.locator('#search-input').fill('beta');
  await expect(page.locator('#results .entry')).toContainText('beta');
  await page.waitForTimeout(800);
  await expect(page.locator('#results .entry')).toContainText('beta');
});

test('Escape closes the search and the feed is where it was', async ({ page, server }) => {
  await server.addTexts(60);
  await page.goto(server.url);
  const feed = page.locator('#feed');
  await expect(page.locator('#feed .entry')).toHaveCount(50);
  await feed.evaluate((el) => { el.scrollTop = el.scrollHeight - el.clientHeight - 400; });
  const before = await feed.evaluate((el) => el.scrollTop);
  await openSearch(page, 'entry 5');
  await expect(page.locator('#results .entry').first()).toBeVisible();
  await page.locator('#search-input').press('Escape');
  await expect(page.locator('#results')).toBeHidden();
  await expect(page.locator('#search-input')).toBeHidden();
  expect(await feed.evaluate((el) => el.scrollTop)).toBe(before);
});

test('clicking a result jumps into the timeline', async ({ page, server }) => {
  const added = await server.addTexts(120);
  const target = added[57];
  await page.goto(server.url);
  await openSearch(page, 'entry 57');
  await page.locator('#results .entry').first().click();
  await expect(page).toHaveURL(new RegExp(`around=${target.id}$`));
  const row = page.locator(`#feed #e-${target.id}`);
  await expect(row).toBeInViewport();
  await expect(row).toHaveClass(/highlight/);
  await expect(page.locator(`#feed #e-${added[56].id}`)).toHaveCount(1);
  await expect(page.locator(`#feed #e-${added[58].id}`)).toHaveCount(1);
});

test('live entries are not appended while viewing the middle', async ({ page, server }) => {
  const added = await server.addTexts(120);
  await page.goto(`${server.url}/?around=${added[57].id}`);
  await expect(page.locator(`#feed #e-${added[57].id}`)).toBeInViewport();
  const fresh = await server.addText('brand new');
  await page.waitForTimeout(3_000);
  await expect(page.locator(`#e-${fresh.id}`)).toHaveCount(0);
  await expect(page.locator('#count')).toHaveText('121');
  await expect(async () => {
    await page.locator('#feed').evaluate((el) => { el.scrollTop = el.scrollHeight; });
    await expect(page.locator(`#e-${fresh.id}`)).toHaveCount(1, { timeout: 200 });
  }).toPass({ timeout: 10_000 });
  await expect(page.locator('#feed .entry').last()).toContainText('brand new');
  const ids = await page.locator('#feed .entry').evaluateAll((els) => els.map((el) => Number(el.dataset.id)));
  expect(ids).toEqual([...ids].sort((a, b) => a - b));
  expect(new Set(ids).size).toBe(ids.length);
});

test('sending while viewing the middle goes to the newest entries', async ({ page, server }) => {
  const added = await server.addTexts(120);
  await page.goto(`${server.url}/?around=${added[57].id}`);
  await expect(page.locator(`#feed #e-${added[57].id}`)).toBeInViewport();
  await page.locator('#input').fill('sent from the middle');
  await page.locator('#input').press('Enter');
  await expect(page).toHaveURL(`${server.url}/`);
  await expect(page.locator('#feed .entry').last()).toContainText('sent from the middle');
  await expect(page.locator('#feed .entry').last()).toBeInViewport();
});

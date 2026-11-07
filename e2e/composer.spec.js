const { test, expect } = require('./helpers');

const PNG_BASE64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==';

function pngFileSource() {
  return `(() => {
    const bytes = Uint8Array.from(atob('${PNG_BASE64}'), (c) => c.charCodeAt(0));
    return (name) => new File([bytes], name, { type: 'image/png', lastModified: 1000 });
  })()`;
}

// A browser that exposes pasted files through clipboardData.items only (files stays empty).
async function pasteThroughItemsOnly(page, count) {
  await page.evaluate(([source, total]) => {
    const makePng = eval(source);
    const items = Array.from({ length: total }, () => ({
      kind: 'file',
      type: 'image/png',
      getAsFile: () => makePng('image.png'),
    }));
    const event = new Event('paste', { bubbles: true, cancelable: true });
    Object.defineProperty(event, 'clipboardData', {
      value: { files: [], items, getData: () => '' },
    });
    document.dispatchEvent(event);
  }, [pngFileSource(), count]);
}

async function pasteDataTransfer(page, setup) {
  await page.evaluate(([source, body]) => {
    const makePng = eval(source);
    const transfer = new DataTransfer();
    new Function('transfer', 'makePng', body)(transfer, makePng);
    document.dispatchEvent(new ClipboardEvent('paste', { clipboardData: transfer, bubbles: true, cancelable: true }));
  }, [pngFileSource(), setup]);
}

test('pasting two screenshots stages both with unique names', async ({ page, server }) => {
  await page.goto(server.url);
  await pasteThroughItemsOnly(page, 2);
  await expect(page.locator('.chip img')).toHaveCount(2);
  await page.locator('#send').click();
  await expect(page.locator('.entry[data-kind="image"]')).toHaveCount(2);
  const names = (await server.list()).map((entry) => entry.file_name);
  expect(new Set(names).size).toBe(2);
  names.forEach((name) => expect(name).toMatch(/^screenshot-\d+\.png$/));
});

test('a file exposed through both files and items is staged once', async ({ page, server }) => {
  await page.goto(server.url);
  await pasteDataTransfer(page, "transfer.items.add(makePng('photo.png'));");
  await expect(page.locator('.chip')).toHaveCount(1);
});

test('screenshots pasted one after another all stay staged', async ({ page, server }) => {
  await page.goto(server.url);
  await pasteThroughItemsOnly(page, 1);
  await pasteThroughItemsOnly(page, 1);
  await expect(page.locator('.chip')).toHaveCount(2);
});

test('pasting plain text types into the box, even with focus elsewhere', async ({ page, server }) => {
  await page.goto(server.url);
  await page.locator('#input').evaluate((el) => el.blur());
  await pasteDataTransfer(page, "transfer.setData('text/plain', 'pasted words');");
  await expect(page.locator('#input')).toHaveValue('pasted words');
  await expect(page.locator('.chip')).toHaveCount(0);
  await expect(page.locator('#send')).toBeEnabled();
});

test('dropping a file shows the overlay and stages it', async ({ page, server }) => {
  await page.goto(server.url);
  await expect(page.locator('#dropzone')).toBeHidden();
  const transfer = await page.evaluateHandle(() => {
    const dt = new DataTransfer();
    dt.items.add(new File(['x'], 'notes.txt', { type: 'text/plain' }));
    return dt;
  });
  await page.dispatchEvent('body', 'dragenter', { dataTransfer: transfer });
  await expect(page.locator('#dropzone')).toBeVisible();
  await page.dispatchEvent('body', 'drop', { dataTransfer: transfer });
  await expect(page.locator('#dropzone')).toBeHidden();
  await expect(page.locator('.chip')).toHaveCount(1);
  await expect(page.locator('.chip')).toContainText('notes.txt');
});

test('text plus file become two entries, text first', async ({ page, server }) => {
  await page.goto(server.url);
  await page.locator('#input').fill('caption words');
  await page.locator('#picker').setInputFiles({ name: 'a.bin', mimeType: 'application/octet-stream', buffer: Buffer.from('x') });
  await expect(page.locator('.chip')).toHaveCount(1);
  await page.locator('#input').press('Enter');
  await expect(page.locator('.entry')).toHaveCount(2);
  await expect(page.locator('.entry').nth(0)).toHaveAttribute('data-kind', 'text');
  await expect(page.locator('.entry').nth(1)).toHaveAttribute('data-kind', 'file');
  await expect(page.locator('#input')).toHaveValue('');
  await expect(page.locator('.chip')).toHaveCount(0);
  await expect(page.locator('#send')).toBeDisabled();
});

test('the same picked file can be removed and added again', async ({ page, server }) => {
  await page.goto(server.url);
  const file = { name: 'a.txt', mimeType: 'text/plain', buffer: Buffer.from('x') };
  await page.locator('#picker').setInputFiles(file);
  await expect(page.locator('.chip')).toHaveCount(1);
  await page.locator('.chip .x').click();
  await expect(page.locator('.chip')).toHaveCount(0);
  await expect(page.locator('#send')).toBeDisabled();
  await page.locator('#picker').setInputFiles(file);
  await expect(page.locator('.chip')).toHaveCount(1);
});

test('failed send keeps the text and files', async ({ page, server }) => {
  await page.goto(server.url);
  await page.route('**/api/entries', (route) => {
    if (route.request().method() !== 'POST') return route.fallback();
    return route.fulfill({ status: 413, contentType: 'application/json', body: '{"error":"big.bin is larger than 1 MB"}' });
  });
  await page.locator('#input').fill('keep me');
  await page.locator('#picker').setInputFiles({ name: 'big.bin', mimeType: 'application/octet-stream', buffer: Buffer.from('x') });
  await page.locator('#send').click();
  await expect(page.locator('#composer-error')).toContainText('larger than');
  await expect(page.locator('#input')).toHaveValue('keep me');
  await expect(page.locator('.chip')).toHaveCount(1);
  await expect(page.locator('.entry')).toHaveCount(0);
  await expect(page.locator('#send')).toBeEnabled();
});

test('a server that is down shows an error and keeps everything', async ({ page, server }) => {
  await page.goto(server.url);
  await page.route('**/api/entries', (route) => {
    if (route.request().method() !== 'POST') return route.fallback();
    return route.abort();
  });
  await page.locator('#input').fill('keep me');
  await page.locator('#send').click();
  await expect(page.locator('#composer-error')).toContainText('Could not reach the server');
  await expect(page.locator('#input')).toHaveValue('keep me');
});

test('send is disabled while a send is running, so it cannot double post', async ({ page, server }) => {
  await page.goto(server.url);
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  await page.route('**/api/entries', async (route) => {
    if (route.request().method() !== 'POST') return route.fallback();
    await gate;
    return route.continue();
  });
  await page.locator('#input').fill('once');
  await page.locator('#input').press('Enter');
  await expect(page.locator('#send')).toBeDisabled();
  await page.locator('#input').press('Enter');
  release();
  await expect(page.locator('.entry')).toHaveCount(1);
  await page.waitForTimeout(200);
  expect((await server.list()).length).toBe(1);
});

test('Shift+Enter adds a new line on desktop', async ({ page, server }) => {
  await page.goto(server.url);
  await page.locator('#input').fill('a');
  await page.locator('#input').press('Shift+Enter');
  await expect(page.locator('.entry')).toHaveCount(0);
  await expect(page.locator('#input')).toHaveValue('a\n');
});

test('on a touch device Enter adds a new line', async ({ browser, server }) => {
  const context = await browser.newContext({ hasTouch: true, isMobile: true, viewport: { width: 390, height: 800 } });
  const page = await context.newPage();
  await page.goto(server.url);
  await page.locator('#input').fill('a');
  await page.locator('#input').press('Enter');
  await expect(page.locator('#input')).toHaveValue('a\n');
  expect((await server.list()).length).toBe(0);
  await page.locator('#send').click();
  await expect(page.locator('.entry')).toHaveCount(1);
  await context.close();
});

test('the text box grows with its content, up to a limit', async ({ page, server }) => {
  await page.goto(server.url);
  const input = page.locator('#input');
  const small = (await input.boundingBox()).height;
  await input.fill('line\n'.repeat(3));
  const grown = (await input.boundingBox()).height;
  expect(grown).toBeGreaterThan(small);
  await input.fill('line\n'.repeat(100));
  expect((await input.boundingBox()).height).toBeLessThanOrEqual(160);
  await input.fill('');
  expect((await input.boundingBox()).height).toBe(small);
});

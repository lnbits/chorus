import { test, expect } from '@playwright/test';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
const username = process.env.CHORUS_E2E_USERNAME || 'admin';
const password = process.env.CHORUS_E2E_PASSWORD || 'browser-test-password';

test('public interface, theme, and mobile navigation', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.goto('/');
  await expect(
    page.getByRole('heading', { name: 'Run the same contract. Agree on the same result.' })
  ).toBeVisible();
  await page.screenshot({ path: '/tmp/chorus-home-desktop.png', fullPage: true });
  await page.getByRole('button', { name: 'Light', exact: true }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await page.getByRole('button', { name: 'Dark', exact: true }).click();
  await page.getByRole('link', { name: 'Explore federations', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Federations', exact: true })).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/');
  await expect(
    page.getByRole('heading', { name: 'Run the same contract. Agree on the same result.' })
  ).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(
    true
  );
  await page.screenshot({ path: '/tmp/chorus-home-mobile.png', fullPage: true });
  expect(errors).toEqual([]);
});

test('real node: login, create, execute, export proof and independently verify', async ({
  page
}, testInfo) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.setViewportSize({ width: 1500, height: 1000 });
  await page.goto('/login');
  await page.getByLabel('USERNAME', { exact: true }).fill(username);
  await page.getByLabel('PASSWORD', { exact: true }).fill(password);
  await page.getByRole('button', { name: 'Login', exact: true }).click();
  await expect(
    page.getByRole('button', { name: 'Create federation', exact: true }).first()
  ).toBeVisible();
  await page.getByRole('button', { name: 'Create federation', exact: true }).first().click();
  const dialog = page.getByRole('dialog');
  const name = `Execution test ${Date.now()}`;
  await dialog.getByLabel('NAME', { exact: true }).fill(name);
  await dialog
    .getByLabel('DESCRIPTION', { exact: true })
    .fill('Browser test of a real deterministic echo contract.');
  await dialog.getByRole('spinbutton').fill('1');
  await dialog.getByLabel('WASM CONTRACT').setInputFiles('../../examples/echo.wasm');
  await dialog.getByRole('button', { name: 'Create federation', exact: true }).click();
  await expect(page.getByRole('heading', { name, exact: true })).toBeVisible();
  const fid = page.url().split('/').at(-1)!;
  await page.screenshot({ path: '/tmp/chorus-federation-desktop.png', fullPage: true });
  await page.getByRole('button', { name: 'Submit job', exact: false }).first().click();
  await page.getByLabel('COMMAND / INPUT').fill('Independent browser execution');
  await page.getByRole('button', { name: 'Submit for execution' }).click();
  await expect(page.getByText('finalized', { exact: true })).toBeVisible({ timeout: 25000 });
  const jobs = await (await page.request.get(`/api/v1/federations/${fid}/jobs`)).json();
  const job = jobs[0];
  expect(job.agreeing).toBe(1);
  expect(job.has_proof).toBe(true);
  const proof = await (await page.request.get(`/api/v1/jobs/${job.id}/proof`)).json();
  const file = join(testInfo.outputDir, 'result.json');
  await mkdir(testInfo.outputDir, { recursive: true });
  await writeFile(file, JSON.stringify(proof));
  const result = execFileSync(
    '../../target/debug/fedwasm',
    ['verify', file, '--genesis-hash', proof.invitation.genesis_hash, '--rerun'],
    { encoding: 'utf8' }
  );
  expect(result).toContain('independent execution matches');
  expect(
    Buffer.from(proof.output_hex, 'hex').includes(Buffer.from('Independent browser execution'))
  ).toBe(true);
  await page.getByRole('button', { name: 'Details', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Job verification data' })).toBeVisible();
  await page.getByRole('button', { name: 'Close dialog' }).click();
  await page.getByRole('button', { name: 'Governance', exact: true }).click();
  await page.getByRole('button', { name: 'Create proposal', exact: true }).click();
  await page.getByRole('dialog').getByRole('spinbutton').fill('1');
  await page
    .getByRole('dialog')
    .getByRole('button', { name: 'Create proposal', exact: true })
    .click();
  await page.getByRole('button', { name: 'Approve this proposal' }).click();
  await expect(page.getByText('Accepted', { exact: true })).toBeVisible();
  await page.goto('/settings');
  await expect(page.getByRole('heading', { name: 'Keys stay on your node.' })).toBeVisible();
  await page.screenshot({ path: '/tmp/chorus-settings-desktop.png', fullPage: true });
  // Authenticated cookies alone cannot authorize writes.
  const csrf = await page.request.post('/api/v1/settings/relays', { data: [] });
  expect(csrf.ok()).toBe(false);
  const response = await (await page.request.get('/api/v1/settings')).text();
  expect(response).not.toContain('nsec');
  await page.getByRole('button', { name: 'Logout', exact: true }).click();
  await expect(page.getByRole('link', { name: 'Login', exact: false })).toBeVisible();
  expect(errors).toEqual([]);
});

test('published bootstrap is external under the strict node CSP', async ({ request }) => {
  const res = await request.get('/');
  expect(res.headers()['content-security-policy']).not.toContain(
    "script-src 'self' 'unsafe-inline'"
  );
  const html = await res.text();
  expect(html).toContain('<script src="/_app/bootstrap-');
  expect(html).not.toMatch(/<script>/);
  const wasm = await readFile('../../examples/echo.wasm');
  expect(wasm.subarray(0, 4)).toEqual(Buffer.from([0, 97, 115, 109]));
});

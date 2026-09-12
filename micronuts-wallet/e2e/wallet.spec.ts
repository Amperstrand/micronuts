import { test, expect, type Page } from "@playwright/test";

// E2E for the wasm wallet (local dist/ or the deployed GitHub Pages build).
//
// Slint renders to a canvas — there is no DOM to assert on. Pattern (same
// as the gm65 playground): the wasm build mirrors semantic wallet state
// onto `window.__micronuts` (page, balance, mint, flags), and input is
// driven by canvas-geometry clicks. The canvas is fixed 480x800 CSS.

const WALLET_URL =
  process.env.WALLET_URL ?? "https://amperstrand.github.io/micronuts/";

type WalletState = {
  page: string;
  balance: string;
  mint: string;
  connected: boolean;
  hasActiveMint: boolean;
  historyLen: number;
};

async function state(page: Page): Promise<WalletState> {
  return page.evaluate(() => (window as any).__micronuts);
}

async function open(page: Page): Promise<void> {
  // Fresh Pages deploys can 404 briefly after the deploy step reports
  // success — poll until the wasm mirror exists (the wallet's own
  // readiness signal; there is no DOM to wait for).
  await page.goto(WALLET_URL, { waitUntil: "domcontentloaded" });
  await expect
    .poll(() => state(page).then((s) => s?.page ?? ""), { timeout: 30_000 })
    .not.toBe("");
}

async function clickCanvas(page: Page, fx: number, fy: number): Promise<void> {
  const box = await page.locator("#canvas").boundingBox();
  expect(box).toBeTruthy();
  await page.mouse.click(box.x + box.width * fx, box.y + box.height * fy);
}

test("boots to home with the demo mint and a formatted balance", async ({
  page,
}) => {
  await open(page);
  const s = await state(page);
  expect(s.hasActiveMint).toBe(true);
  expect(s.page).toBe("home");
  // Amount format contract: thousands-separated + " sats".
  expect(s.balance).toMatch(/^(••••••|[0-9,]+ sats)$/);
});

test("bottom navigation: Home / Activity / Settings tabs switch pages", async ({
  page,
}) => {
  await open(page);

  // Tab centers: thirds of the 92px bottom bar.
  await clickCanvas(page, 0.5, 0.945); // Activity
  await expect.poll(() => state(page).then((s) => s.page)).toBe("activity");

  await clickCanvas(page, 0.833, 0.945); // Settings
  await expect.poll(() => state(page).then((s) => s.page)).toBe("settings");

  await clickCanvas(page, 0.167, 0.945); // Home
  await expect.poll(() => state(page).then((s) => s.page)).toBe("home");
});

test("settings navigates to mints and back with the header affordance", async ({
  page,
}) => {
  await open(page);
  await clickCanvas(page, 0.833, 0.945); // Settings tab
  await expect.poll(() => state(page).then((s) => s.page)).toBe("settings");

  // First settings row = Mints (title + 72px rows from the top padding).
  await clickCanvas(page, 0.5, 0.125);
  await expect.poll(() => state(page).then((s) => s.page)).toBe("mints");

  // BackHeader (‹) sits at the top-left.
  await clickCanvas(page, 0.06, 0.04);
  await expect.poll(() => state(page).then((s) => s.page)).toBe("settings");
});

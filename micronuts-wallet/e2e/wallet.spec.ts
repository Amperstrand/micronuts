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

test("receive ecash: typing a token auto-inspects into a review state", async ({
  page,
}) => {
  await open(page);

  // Home → Receive (left primary button under the balance; center ≈ y264
  // from the layout: pad20+28+chip20+balance66+status18+spacers → 230..298).
  await clickCanvas(page, 0.3, 0.33);
  await expect.poll(() => state(page).then((s) => s.page)).toBe("receive");

  // Focus the paste field and type a syntactically valid token from a
  // mint the wallet is not connected to — the embedded demo mint is.
  // A genuinely decodable V4 token from an in-process demo mint
  // (examples/print_token) — foreign to the wallet's embedded mint, so
  // inspection must land on the foreign-mint review wording.
  await clickCanvas(page, 0.5, 0.22);
  await page.keyboard.type(
    "cashuBpGFtdGh0dHBzOi8vbWludC5leGFtcGxlYXVjc2F0YWRjZTJlYXSBomFpSAAi4CWGd5PRYXCDpGFhEGFzeEA5ZDQ0MDc2ZTA0Y2Q1Yzc1OTIzNTQyMmZhNmNkNjkzZDhjMmViNGYwNzM3NDhjODIyNjhmMWY2NzI4NTNjYjNiYWNYIQOL4r5EO99Ar-WZVzHDjgjXTwlwzOxMk-qgdLxrwfLRoWFko2FlWCCQqXMzTHeGcZx65daAxrdMhsS1jJDPVhCGDkZdj9DVKGFzWCAU8nuXZgkYk6FCDBeGZhwXxeiaLJLkZpAPfU0HZnPlOmFyWCBYIFBKGJpoh3-1A23dK4Syask2Zn7Utf-wNTveekmxwqRhYQRhc3hANTQ4NmI5MDQ5OGY2OTg0ZDQwZTQzYzY2OTc0ZTdhZTQzZjY3MWEzNGY5OWRiM2E1MjU2OWUyNzA3MTk4NWMzOGFjWCEC8xJ040ab8opGfzJ7UbUshlYkKpk7jKyiKBipnwNHaRRhZKNhZVggD8cKpOCOk3KZZnBvSCaDvT1gCynVUH_iHtH5ecDoaW1hc1gg0pyVHH8_j3BFrxpBBDuusYKeLyBkFIR84FC5OfYYUKJhclggcyXhidi1Gj0-1vlUQvdmdrqaJKSHxVkt9qp2sjG5_u2kYWEBYXN4QGZlY2Y4ZGZkNDBhNzk3OWIyOWQyNmQzODdiOWY3Nzg5YmExMGQwZWY1OWZiYWZjZGI4NGE1YmNiOGU4NDczMTlhY1ghAnJZaoV4OtzKP0HI_7MrUGNRnlGlnHODS1EaujKREYQ1YWSjYWVYIHNdg5QlxdMT7ijHCcItRWQbJ_2fabr1O4XwC7YIodquYXNYIOWA42LcH5MmfI0vzg7fGLultXyQzHTwr1BSVRVdarJHYXJYIBcOwwmgbf9ar7lpZIIWYDfkurxeSCx3DJt7csNdxjg7",
    { delay: 5 },
  );

  // The review machinery runs by itself: state must leave "input" and
  // land on the foreign-mint failure wording (no auto-spend possible —
  // there is no confirmation to press).
  await expect
    .poll(() => state(page).then((s) => s.receiveState), { timeout: 15_000 })
    .toBe("failed");
  const s = await state(page);
  expect(s.receiveLine).toContain("different mint");
});

test("send ecash with empty balance fails honestly, not silently", async ({
  page,
}) => {
  await open(page);

  // Home → Send (right primary button).
  await clickCanvas(page, 0.7, 0.33);
  await expect.poll(() => state(page).then((s) => s.page)).toBe("send");

  // Amount field → 21 → Create token (CI-calibrated geometry shared
  // with money.spec.ts). Zero balance must land in the honest failed
  // state.
  await clickCanvas(page, 0.5, 0.175);
  await page.keyboard.type("21", { delay: 20 });
  await clickCanvas(page, 0.5, 0.3);
  await expect
    .poll(() => state(page).then((s) => s.sendState), { timeout: 15_000 })
    .toBe("failed");
  const s = await state(page);
  expect(s.pendingSends).toBe(0);
});

import { test, expect, type Page } from "@playwright/test";

// The money cycle — every flow the wallet exists for, driven end to end
// against the embedded demo mint (auto-paying, so fully deterministic).
// Geometry calibrated against the 480x800 canvas in the 560x900 viewport
// (whole canvas visible — no scrolling needed, unlike smaller viewports).

const WALLET_URL =
  process.env.WALLET_URL ?? "https://amperstrand.github.io/micronuts/";

type WalletState = {
  page: string;
  balance: string;
  receiveState: string;
  receiveLine: string;
  tokenOut: string;
  invoiceState: string;
  meltPreimage: string;
  historyLen: number;
};

async function state(page: Page): Promise<WalletState> {
  return page.evaluate(() => (window as any).__micronuts);
}

async function open(page: Page): Promise<void> {
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

async function expectBalance(page: Page, text: string): Promise<void> {
  await expect
    .poll(() => state(page).then((s) => s.balance), { timeout: 20_000 })
    .toBe(text);
}

// Every test gets a fresh in-memory wallet (balance 0): top up first.
// The embedded mint auto-pays, so this is fully deterministic.
async function mint(page: Page, amount: number): Promise<void> {
  await clickCanvas(page, HOME_RECEIVE.fx, HOME_RECEIVE.fy);
  await expect.poll(() => state(page).then((s) => s.page)).toBe("receive");
  await clickCanvas(page, TAB_PILL_RIGHT.fx, TAB_PILL_RIGHT.fy); // Lightning
  await clickCanvas(page, 0.5, 0.19); // amount field
  await page.keyboard.type(String(amount), { delay: 20 });
  await clickCanvas(page, 0.5, 0.24); // Create invoice
  await expect
    .poll(() => state(page).then((s) => s.invoiceState), { timeout: 15_000 })
    .toBe("PAID");
  await clickCanvas(page, 0.5, 0.78); // Mint ecash
  await expectBalance(page, `${amount} sats`);
  await clickCanvas(page, BOTTOM_NAV_HOME.fx, BOTTOM_NAV_HOME.fy);
  await expect.poll(() => state(page).then((s) => s.page)).toBe("home");
}

// Home action row: Receive left, Send right (calibrated under the
// balance block).
const HOME_RECEIVE = { fx: 0.3, fy: 0.345 };
const HOME_SEND = { fx: 0.7, fy: 0.345 };
const TAB_PILL_RIGHT = { fx: 0.65, fy: 0.115 };
const BOTTOM_NAV_HOME = { fx: 0.167, fy: 0.945 };

test("mints ecash from a paid invoice (receive → lightning → mint)", async ({
  page,
}) => {
  await open(page);
  await clickCanvas(page, HOME_RECEIVE.fx, HOME_RECEIVE.fy);
  await expect.poll(() => state(page).then((s) => s.page)).toBe("receive");

  await clickCanvas(page, TAB_PILL_RIGHT.fx, TAB_PILL_RIGHT.fy); // Lightning
  await clickCanvas(page, 0.5, 0.19); // amount field
  await page.keyboard.type("100", { delay: 20 });
  await clickCanvas(page, 0.5, 0.24); // Create invoice

  // The embedded mint auto-pays; the mirror exposes the quote state.
  await expect
    .poll(() => state(page).then((s) => s.invoiceState), { timeout: 15_000 })
    .toBe("PAID");

  await clickCanvas(page, 0.5, 0.78); // Mint ecash
  await expectBalance(page, "100 sats");
});

test("send → receive round trip returns to the starting balance", async ({
  page,
}) => {
  await open(page);
  await mint(page, 100);

  // Send 21 sats of ecash.
  await clickCanvas(page, HOME_SEND.fx, HOME_SEND.fy);
  await expect.poll(() => state(page).then((s) => s.page)).toBe("send");
  await clickCanvas(page, 0.5, 0.175); // amount field (ecash tab default)
  await page.keyboard.type("21", { delay: 20 });
  await clickCanvas(page, 0.5, 0.3); // Create token
  await expect
    .poll(() => state(page).then((s) => s.tokenOut.length), { timeout: 15_000 })
    .toBeGreaterThan(20);
  await expectBalance(page, "79 sats");
  const token = (await state(page)).tokenOut;

  // Receive it back: paste → auto-inspect → confirm.
  await clickCanvas(page, BOTTOM_NAV_HOME.fx, BOTTOM_NAV_HOME.fy);
  await clickCanvas(page, HOME_RECEIVE.fx, HOME_RECEIVE.fy);
  await expect.poll(() => state(page).then((s) => s.page)).toBe("receive");
  await clickCanvas(page, 0.5, 0.22); // paste field (ecash tab default)
  await page.keyboard.type(token, { delay: 0 });
  await expect
    .poll(() => state(page).then((s) => s.receiveState), { timeout: 30_000 })
    .toBe("review");
  await expect
    .poll(() => state(page).then((s) => s.receiveLine))
    .toContain("21 sats");

  await clickCanvas(page, 0.5, 0.51); // Receive (confirm, in review card)
  await expect
    .poll(() => state(page).then((s) => s.receiveState), { timeout: 30_000 })
    .toBe("received");
  await expectBalance(page, "100 sats");
});

test("melts ecash to pay an invoice (send → lightning → pay)", async ({
  page,
}) => {
  await open(page);
  await mint(page, 100);

  await clickCanvas(page, HOME_SEND.fx, HOME_SEND.fy);
  await expect.poll(() => state(page).then((s) => s.page)).toBe("send");
  await clickCanvas(page, TAB_PILL_RIGHT.fx, TAB_PILL_RIGHT.fy); // Lightning
  await clickCanvas(page, 0.5, 218 / 800); // invoice box
  await page.keyboard.type("lnbcdemo30sat1demo", { delay: 10 });
  await clickCanvas(page, 0.5, 302 / 800); // Get quote

  await clickCanvas(page, 0.5, 373 / 800); // Pay invoice
  await expect
    .poll(() => state(page).then((s) => s.meltPreimage), { timeout: 20_000 })
    .not.toBe("");
  await expectBalance(page, "70 sats");
});

test("console contract: qr self-test ok and no unexpected errors", async ({
  page,
}) => {
  const logs: string[] = [];
  const errors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push(msg.text());
    logs.push(msg.text());
  });
  const pageErrors: string[] = [];
  page.on("pageerror", (err) => pageErrors.push(String(err)));

  await open(page);
  await expect
    .poll(() => logs.some((line) => line.includes("qr self-test: ok")), {
      timeout: 15_000,
    })
    .toBe(true);

  // Known-benign noise: winit unwinds the event loop through a JS
  // exception by design; the favicon 404 is a missing static asset.
  const benign = (text: string) =>
    text.includes("Using exceptions for control flow") ||
    text.includes("Failed to load resource") ||
    text.includes("favicon");
  const unexpected = errors.filter((text) => !benign(text));
  expect(unexpected).toEqual([]);
  expect(pageErrors.filter((text) => !benign(text))).toEqual([]);
});

import { test, expect, type Page } from "@playwright/test";

// The money cycle — every flow the wallet exists for, driven end to end
// against the embedded demo mint (auto-paying, fully deterministic).
//
// Driving model: `window.__micronutsAct(name, arg)` invokes the exact
// WalletLogic callbacks the on-screen buttons fire (installed by the
// wasm build alongside the state mirror). Coordinate clicking was the
// previous approach — brittle (layout drift = silent misfires) and
// unfixable via element geometry because Slint-on-wasm exposes no DOM
// and its absolute-position doesn't propagate into the binding graph.
// Pointer plumbing itself stays covered by the nav/boot pointer tests
// in wallet.spec.ts.

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
  meltQuoteInfo: string;
  historyLen: number;
};

async function state(page: Page): Promise<WalletState> {
  return page.evaluate(() => (window as any).__micronuts);
}

async function act(page: Page, name: string, arg = ""): Promise<string> {
  return page.evaluate(
    ([n, a]) => (window as any).__micronutsAct(n, a),
    [name, arg] as const,
  );
}

async function open(page: Page): Promise<void> {
  await page.goto(WALLET_URL, { waitUntil: "domcontentloaded" });
  await expect
    .poll(() => state(page).then((s) => s?.page ?? ""), { timeout: 30_000 })
    .not.toBe("");
}

async function expectBalance(page: Page, text: string): Promise<void> {
  await expect
    .poll(() => state(page).then((s) => s.balance), { timeout: 20_000 })
    .toBe(text);
}

test("mints ecash from a paid invoice (receive → lightning → mint)", async ({
  page,
}) => {
  await open(page);
  await act(page, "navigate", "receive");
  await expect.poll(() => state(page).then((s) => s.page)).toBe("receive");

  await act(page, "mint-invoice", "100");
  // The embedded mint auto-pays; issuance is automatic on PAID.
  await expectBalance(page, "100 sats");
  expect((await state(page)).historyLen).toBeGreaterThanOrEqual(1);
});

test("send → receive round trip returns to the starting balance", async ({
  page,
}) => {
  await open(page);
  await act(page, "navigate", "receive");
  await act(page, "mint-invoice", "100");
  await expectBalance(page, "100 sats");

  await act(page, "navigate", "send");
  await act(page, "send-token", "21 demo");
  await expect
    .poll(() => state(page).then((s) => s.tokenOut.length), { timeout: 15_000 })
    .toBeGreaterThan(20);
  await expectBalance(page, "79 sats");
  const token = (await state(page)).tokenOut;

  // Paste → auto-inspect → confirm, exactly like the Ecash tab.
  await act(page, "navigate", "receive");
  await act(page, "token-edited", token);
  await expect
    .poll(() => state(page).then((s) => s.receiveState), { timeout: 30_000 })
    .toBe("review");
  await expect
    .poll(() => state(page).then((s) => s.receiveLine))
    .toContain("21 sats");

  await act(page, "receive-token", token);
  await expect
    .poll(() => state(page).then((s) => s.receiveState), { timeout: 30_000 })
    .toBe("received");
  await expectBalance(page, "100 sats");
});

test("melts ecash to pay an invoice (send → lightning → pay)", async ({
  page,
}) => {
  await open(page);
  await act(page, "navigate", "receive");
  await act(page, "mint-invoice", "100");
  await expectBalance(page, "100 sats");

  await act(page, "navigate", "send");
  await act(page, "invoice-edited", "lnbcdemo30sat1demo");
  // Quote acquisition is automatic (wallet machinery): wait for the
  // review card, then confirm.
  await expect
    .poll(() => state(page).then((s) => s.meltQuoteInfo), { timeout: 15_000 })
    .toContain("30 sat");

  await act(page, "melt-confirm");
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

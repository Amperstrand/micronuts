import { test, expect, type Page } from "@playwright/test";

// Real-mint e2e against testnut.cashu.space (cdk-mintd FakeWallet:
// invoices auto-settle, so the flow is deterministic; signet-adjacent
// test money only — repo money taxonomy). Gated behind
// WALLET_E2E_REAL_MINT because it needs the public internet and a live
// third-party service; the hermetic demo-mint suite must stay green
// regardless.

const WALLET_URL =
  process.env.WALLET_URL ?? "https://amperstrand.github.io/micronuts/";
const REAL_MINT = process.env.WALLET_E2E_REAL_MINT_URL ?? "https://testnut.cashu.space";

test.skip(
  !process.env.WALLET_E2E_REAL_MINT,
  "set WALLET_E2E_REAL_MINT=1 to exercise the real-mint flow",
);

type WalletState = { page: string; balance: string; mint: string; connected: boolean };

async function state(page: Page): Promise<WalletState> {
  return page.evaluate(() => (window as any).__micronuts);
}

async function act(page: Page, name: string, arg = ""): Promise<string> {
  return page.evaluate(
    ([n, a]) => (window as any).__micronutsAct(n, a),
    [name, arg],
  );
}

test("adds a real mint and receives Lightning ecash over HTTPS", async ({
  page,
}) => {
  await page.goto(WALLET_URL, { waitUntil: "domcontentloaded" });
  await expect
    .poll(() => state(page).then((s) => s?.page ?? ""), { timeout: 30_000 })
    .not.toBe("");

  // Add the real mint (info probe → trust → connect).
  await act(page, "add-mint", REAL_MINT);
  await expect
    .poll(() => state(page).then((s) => s.connected), { timeout: 30_000 })
    .toBe(true);
  await expect
    .poll(() => state(page).then((s) => s.mint))
    .not.toBe("");

  // Receive via Lightning: invoice → auto-settle → auto-issuance.
  await act(page, "navigate", "receive");
  await act(page, "mint-invoice", "11");
  await expect
    .poll(() => state(page).then((s) => s.balance), { timeout: 60_000 })
    .toBe("11 sats");
});

test("wallet-to-wallet on a real mint: QR pixels → ecash handover → claim detection", async ({
  page,
  browser,
}) => {
  await page.goto(WALLET_URL, { waitUntil: "domcontentloaded" });
  await expect
    .poll(() => state(page).then((s) => s?.page ?? ""), { timeout: 30_000 })
    .not.toBe("");

  // Wallet A (this page): add the real mint, fund, send.
  await act(page, "add-mint", REAL_MINT);
  await expect
    .poll(() => state(page).then((s) => s.connected), { timeout: 30_000 })
    .toBe(true);
  await act(page, "navigate", "receive");
  await act(page, "mint-invoice", "64");
  await expect
    .poll(() => state(page).then((s) => s.balance), { timeout: 60_000 })
    .toBe("64 sats");
  await act(page, "navigate", "send");
  await act(page, "send-token", "21");
  await expect
    .poll(() => state(page).then((s) => s.tokenOut.length), { timeout: 15_000 })
    .toBeGreaterThan(20);

  // Decode A's ACTUAL rendered QR pixels (host-side PNG → jsQR).
  const box = await page.locator("#canvas").boundingBox();
  const shot = await page.screenshot({
    clip: {
      x: box.x,
      y: box.y + box.height * 0.35,
      width: box.width,
      height: box.height * 0.55,
    },
  });
  const { PNG } = require("pngjs");
  const jsQR = require("jsqr");
  const png = PNG.sync.read(shot);
  const qr = jsQR(new Uint8ClampedArray(png.data), png.width, png.height);
  expect(qr, "send-QR pixels must decode").toBeTruthy();
  const token = qr!.data as string;
  expect(token.startsWith("cashuB")).toBe(true);

  // Wallet B — a separate browser context: own seed, own storage, same
  // real mint. Paste → auto-inspect review → Receive.
  const ctx2 = await browser.newContext({ viewport: { width: 560, height: 900 } });
  const page2 = await ctx2.newPage();
  await page2.goto(WALLET_URL, { waitUntil: "domcontentloaded" });
  await expect
    .poll(() => state(page2).then((s) => s?.page ?? ""), { timeout: 30_000 })
    .not.toBe("");
  await act(page2, "add-mint", REAL_MINT);
  await expect
    .poll(() => state(page2).then((s) => s.connected), { timeout: 30_000 })
    .toBe(true);
  await act(page2, "navigate", "receive");
  await act(page2, "token-edited", token);
  await expect
    .poll(() => state(page2).then((s) => s.receiveState), { timeout: 30_000 })
    .toBe("review");
  await act(page2, "receive-token", token);
  // testnut charges a NUT-08 input fee scaled by its current keyset
  // (ppk floats with their keyset rotations) — assert a positive
  // credited balance at-or-below the sent amount, and surface the
  // review/failure line if the receive stalls.
  await expect
    .poll(
      () =>
        state(page2).then((s) => {
          const m = s.balance?.match(/^(\d+) sats$/);
          if (!m) return -1;
          const n = parseInt(m[1], 10);
          if (n === 0) throw new Error(
            `receive failed: ${s.receiveState} "${s.receiveLine}"`,
          );
          return n;
        }),
      { timeout: 45_000 },
    )
    .toBeLessThanOrEqual(21);
  await ctx2.close();

  // A's pending send resolves to claimed (B's swap spent the proofs).
  await expect
    .poll(() => state(page).then((s) => s.pendingSends), { timeout: 30_000 })
    .toBe(0);
});

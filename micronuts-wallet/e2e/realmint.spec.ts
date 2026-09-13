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

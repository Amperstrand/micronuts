#!/usr/bin/env node
// Wallet-level e2e against a running micronuts mint (default: the local
// audit-adapter on :3030 with the FakeWallet backend).
//
// Exercises the full NUT-04/03/07/05 cycle with a REAL wallet library
// (cashu-ts v4+ from the cashu-cf workspace — no local install needed;
// v3 is LEGACY and rejected by the version guard below):
//   mint quote → poll UNPAID→PAID → mint proofs → swap/send
//   → melt quote → melt (preimage asserted) → double-spend rejection.
//
// Usage:
//   MINT_URL=http://localhost:3030 node scripts/e2e_wallet.mjs
// Env:
//   MINT_URL          (default http://localhost:3030)
//   CASHU_CF_ROOT     (default: ../../.. relative to this script — for node_modules)
//   SETTLE_POLL_TRIES (default 10) — mint-quote settle poll iterations
//   SETTLE_POLL_MS    (default 200) — per-iteration sleep
//   PAY_CMD           shell snippet run with $INVOICE set to the mint-quote
//                     bolt11 right after creation — for upstreams that need
//                     a real payer (signut); unset = no external payment
//   MELT_INVOICE      real bolt11 melt target (upstream mode; preimage
//                     asserted from the upstream payment)
//   MELT_AMOUNT       (default 10) must equal the MELT_INVOICE amount

import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';
import { createHash } from 'node:crypto';
import { execSync } from 'node:child_process';

const here = dirname(fileURLToPath(import.meta.url));
const MINT_URL = process.env.MINT_URL ?? 'http://localhost:3030';
const CASHU_CF_ROOT = resolve(process.env.CASHU_CF_ROOT ?? join(here, '..', '..', '..'));

const require = createRequire(join(CASHU_CF_ROOT, 'package.json'));
const { readFileSync } = require('node:fs');
const cashuTsManifestPath = join(CASHU_CF_ROOT, 'node_modules', '@cashu', 'cashu-ts', 'package.json');
const cashuTsVersion = JSON.parse(readFileSync(cashuTsManifestPath, 'utf8')).version;
const cashuTsMajor = Number(cashuTsVersion.split('.')[0]);
if (!(cashuTsMajor >= 4)) {
    console.error(
        `E2E-WALLET ERROR: cashu-ts ${cashuTsVersion} is legacy — this harness targets v4+ only ` +
        `(owner directive 2026-09-02; v3 idioms must not be reimplemented). ` +
        `Run \`npm ci\` in ${CASHU_CF_ROOT} to sync with the lockfile.`
    );
    process.exit(1);
}
const { Mint, Wallet } = require('@cashu/cashu-ts');

const ok = (msg) => console.log(`PASS ${msg}`);
const die = (msg) => {
    console.error(`FAIL ${msg}`);
    process.exit(1);
};
const assert = (cond, msg) => (cond ? ok(msg) : die(msg));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const fakeInvoice = (sats) => `lnbcdemo${sats}sat1micronuts`;

async function main() {
    console.log(`e2e wallet vs ${MINT_URL} (cashu-ts from ${CASHU_CF_ROOT})`);

    const mint = new Mint(MINT_URL, { unit: 'sat' });
    const wallet = new Wallet(mint, { unit: 'sat' });

    const info = await mint.getInfo();
    assert(info.name.length > 0, `NUT-06 info: name="${info.name}"`);
    const nuts = info.nuts ?? {};
    assert(nuts['4'] && nuts['5'], 'NUT-06 nuts map advertises 4 and 5');

    await wallet.loadMint();
    const keys = await mint.getKeys();
    const denomCount = Object.keys(keys.keysets[0]?.keys ?? {}).length;
    assert(denomCount > 0, `NUT-01 keys: ${denomCount} denominations`);

    // NUT-04: quote born UNPAID; FakeWallet settles on first poll.
    const amount = 64;
    const quoteResp = await mint.createMintQuoteBolt11({ amount, unit: 'sat' });
    assert(
        typeof quoteResp.quote === 'string' && quoteResp.quote.length > 0,
        `NUT-04 mint quote: ${quoteResp.quote}`
    );
    console.log(`     invoice: ${quoteResp.request.slice(0, 24)}… state=${quoteResp.state ?? quoteResp.paid}`);

    if (process.env.PAY_CMD) {
        console.log(`     paying via PAY_CMD: ${process.env.PAY_CMD.slice(0, 60)}…`);
        execSync(process.env.PAY_CMD, {
            stdio: 'inherit',
            timeout: 180_000,
            env: { ...process.env, INVOICE: quoteResp.request },
        });
    }

    let quoteObj;
    let state = quoteResp.state ?? 'UNPAID';
    const settleTries = Number(process.env.SETTLE_POLL_TRIES ?? 10);
    const settleMs = Number(process.env.SETTLE_POLL_MS ?? 200);
    for (let i = 0; i < settleTries && state !== 'PAID'; i++) {
        await sleep(settleMs);
        quoteObj = await mint.checkMintQuoteBolt11(quoteResp.quote);
        state = quoteObj.state;
    }
    if (!quoteObj) quoteObj = await mint.checkMintQuoteBolt11(quoteResp.quote);
    assert(state === 'PAID', `NUT-04 quote settles to PAID on poll (state=${state})`);

    // v4: mintProofs is (method, amount, quote); mintProofsBolt11 takes the
    // quote OBJECT (accounting fields required), not an ID.
    const proofs = await wallet.mintProofsBolt11(amount, quoteObj);
    const total = proofs.reduce((s, p) => s + Number(p.amount), 0);
    assert(total === amount, `NUT-04 minted ${total} sats in ${proofs.length} proofs`);

    // v4: proofs are caller-managed — send/melt take explicit proof lists.
    const { keep, send } = await wallet.send(21, proofs);
    const keepSum = keep.reduce((s, p) => s + Number(p.amount), 0);
    const sendSum = send.reduce((s, p) => s + Number(p.amount), 0);
    assert(sendSum === 21, `NUT-03 swap: sent ${sendSum} sats (${send.length} proofs)`);
    assert(keepSum === amount - 21, `NUT-03 swap: kept ${keepSum} sats`);

    // NUT-05 melt. In FakeWallet mode the target is a demo invoice with a
    // deterministic preimage; in upstream mode MELT_INVOICE carries a REAL
    // bolt11 (e.g. minted from testnut) — preimage comes from upstream.
    const meltAmt = Number(process.env.MELT_AMOUNT ?? 10);
    const upstreamMode = Boolean(process.env.MELT_INVOICE);
    const invoice = process.env.MELT_INVOICE ?? fakeInvoice(meltAmt);
    const meltQuote = await wallet.createMeltQuoteBolt11(invoice);
    assert(Number(meltQuote.amount) === meltAmt, `NUT-05 melt quote amount=${meltQuote.amount}`);
    const feeReserve = Number(meltQuote.fee_reserve ?? 0);

    const spendable = [...keep, ...send];
    const spendableSum = spendable.reduce((s, p) => s + Number(p.amount), 0);
    const spentByMelt = [...spendable];
    const meltResult = await wallet.meltProofsBolt11(meltQuote, spendable);
    // v4 nests the quote fields under `.quote`; amounts arrive as Amount
    // serializations — coerce with Number() (cashu-cf boundary lesson).
    const settled = meltResult?.quote ?? meltResult;
    const paid = settled?.paid ?? settled?.state === 'PAID';
    assert(paid, `NUT-05 melt paid (state=${settled?.state})`);
    if (upstreamMode) {
        assert(/^[0-9a-f]{64}$/.test(settled.payment_preimage ?? ''), 'NUT-05 upstream preimage returned');
    } else {
        const expectedPreimage = createHash('sha256').update(invoice).digest('hex');
        assert(settled.payment_preimage === expectedPreimage, 'NUT-05 deterministic FakeWallet preimage');
    }
    const changeSum = (settled.change ?? []).reduce((s, p) => s + Number(p.amount), 0);
    assert(
        changeSum === spendableSum - meltAmt - feeReserve,
        `NUT-08 melt change = ${changeSum} sats (overpay fully returned)`
    );

    // Double-spend: re-present the SAME proofs the first melt consumed.
    let doubleSpendRejected = false;
    try {
        const q2 = await wallet.createMeltQuoteBolt11(fakeInvoice(1));
        await wallet.meltProofsBolt11(q2, spentByMelt);
    } catch {
        doubleSpendRejected = true;
    }
    assert(doubleSpendRejected, 'double-spend of spent proofs rejected');

    const finalBalance = (settled.change ?? []).reduce((s, p) => s + Number(p.amount), 0);
    console.log(`     final held value: ${finalBalance} sats`);
    console.log('E2E-WALLET PASS');
}

main().catch((e) => {
    console.error('E2E-WALLET ERROR', e?.message ?? e);
    process.exit(1);
});

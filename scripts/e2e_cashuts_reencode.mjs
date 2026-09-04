#!/usr/bin/env node
// Real-wallet AUTHORED token: decode a cashuB token with cashu-ts v4,
// re-encode it with getEncodedToken, and assert the re-encoded form
// decodes to the same token. The output string is authored BY cashu-ts
// — the QR scan-in leg (#29 P3b) feeds exactly this string to the GM65
// so the device accepts a token a real wallet produced, not just one
// our encoder emitted.
//
// Usage: TOKEN=cashuB... node scripts/e2e_cashuts_reencode.mjs
// Env:   CASHU_CF_ROOT (default ../../cashu-cf sibling checkout)
// Exit: 0 pass (token on stdout, single line), 1 fail.

import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const CASHU_CF_ROOT = resolve(
    process.env.CASHU_CF_ROOT ?? join(here, '..', '..', 'cashu-cf')
);

const require = createRequire(join(CASHU_CF_ROOT, 'package.json'));
const { getDecodedToken, getEncodedToken } = require('@cashu/cashu-ts');

const token = process.env.TOKEN;
if (!token || !token.startsWith('cashuB')) {
    console.error('REENCODE ERROR: set TOKEN=cashuB...');
    process.exit(1);
}

const decoded = getDecodedToken(token, []);
const reencoded = getEncodedToken(decoded);

const redone = getDecodedToken(reencoded, []);
const fail = (msg) => {
    console.error(`REENCODE FAIL: ${msg}`);
    process.exit(1);
};
if (reencoded === token) fail('re-encode was a no-op — token already cashu-ts-shaped?');
if (redone.mint !== decoded.mint) fail(`mint drift ${redone.mint} != ${decoded.mint}`);
if (redone.unit !== decoded.unit) fail(`unit drift ${redone.unit} != ${decoded.unit}`);
if (redone.proofs.length !== decoded.proofs.length) fail('proof count drift');
for (const [i, p] of decoded.proofs.entries()) {
    if (Number(redone.proofs[i].amount) !== Number(p.amount)) fail(`proof[${i}] amount drift`);
    if (redone.proofs[i].C !== p.C) fail(`proof[${i}] C drift`);
    if (redone.proofs[i].secret !== p.secret) fail(`proof[${i}] secret drift`);
}
console.log(reencoded);

#!/usr/bin/env node
// Ecosystem-consumer conformance: feed a DEVICE-minted cashuB token to
// cashu-ts v4 (the real-wallet leg) — decode + offline NUT-12 DLEQ verify
// against the pinned demo keyset. No micronuts code parses the token on
// this path: the ONLY verifier is cashu-ts.
//
// Modes (exactly one required):
//   PORT=/dev/ttyACMx   drive the STM32 swap flow via mint-tool
//                       (generate 21 → sign → export), then verify
//   TOKEN=cashuB...     verify an already-exported token string
//   --selftest          verify the cashu-ts surface itself decodes the
//                       NUT-00 V4 spec example token (no hardware)
//
// Asserts on the decoded token (all against cashu-ts types):
//   mint === demo://micronuts, unit === sat, memo present
//   3 proofs summing 21 (16/4/1), every C 33-byte compressed (66 hex)
//   dleq present on every proof ({e,s,r} hex)
//   hasValidDleq(proof, pinnedDemoKeyset, {require:true}) === true
//
// Env:
//   CASHU_CF_ROOT  (default ../../.. from this script) — cashu-ts source
//   MINT_TOOL      (default target/release/mint-tool)
//   AMOUNT         (default 21)
//
// Exit: 0 pass, 1 fail (with the cashu-ts error surfaced verbatim —
// any rejection here is a wire-format finding, not a harness bug).

import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';
import { execSync } from 'node:child_process';

const here = dirname(fileURLToPath(import.meta.url));
// cashu-cf is a sibling checkout of this repo (~/src/cashu-cf) — its
// node_modules carries the real-wallet dependency set.
const CASHU_CF_ROOT = resolve(
    process.env.CASHU_CF_ROOT ?? join(here, '..', '..', 'cashu-cf')
);
const MINT_TOOL = resolve(process.env.MINT_TOOL ?? join(here, '..', 'target', 'release', 'mint-tool'));
const AMOUNT = Number(process.env.AMOUNT ?? 21);

// Pinned demo keyset: one key for ALL amounts = SHA256("demo://micronuts"),
// compressed; keyset id "00" (hex-decodes to the 0x00 version-1 prefix byte,
// so cashu-ts treats it as a full id — no short-id mapping needed).
const DEMO_KEYSET_ID = '00';
const DEMO_KEY =
    '03c3be0ab3a553e0f8047e089e34bb21a86ac78aa0620106d02c344ea2d3596d9c';
const DEMO_MINT = 'demo://micronuts';

const require = createRequire(join(CASHU_CF_ROOT, 'package.json'));
const { readFileSync } = require('node:fs');
const cashuTsManifestPath = join(CASHU_CF_ROOT, 'node_modules', '@cashu', 'cashu-ts', 'package.json');
const cashuTsVersion = JSON.parse(readFileSync(cashuTsManifestPath, 'utf8')).version;
if (!(Number(cashuTsVersion.split('.')[0]) >= 4)) {
    console.error(`CONFORMANCE ERROR: cashu-ts ${cashuTsVersion} is legacy — v4+ required.`);
    process.exit(1);
}
const { getDecodedToken, hasValidDleq } = require('@cashu/cashu-ts');

const ok = (msg) => console.log(`PASS ${msg}`);
const die = (msg) => {
    console.error(`FAIL ${msg}`);
    process.exit(1);
};
const assert = (cond, msg) => (cond ? ok(msg) : die(msg));

// cashu-ts Keys: amount -> pubkey hex. The demo mint uses the SAME key for
// every denomination, so map every power of two up to 2^32.
const demoKeys = {};
for (let i = 0; i < 32; i++) demoKeys[String(2 ** i)] = DEMO_KEY;
const demoKeyset = { id: DEMO_KEYSET_ID, keys: demoKeys };

// NUT-00 V4 spec example (nuts/00.md) — the selftest ground truth.
const SPEC_EXAMPLE =
    'cashuBo2F0gqJhaUgA_9SLj17PgGFwgaNhYQFhc3hAYWNjMTI0MzVlN2I4NDg0YzNjZjE4NTAxNDkyMThhZjkwZjcxNmE1MmJmNGE1ZWQzNDdlNDhlY2MxM2Y3NzM4OGFjWCECRFODGd5IXVW-07KaZCvuWHk3WrnnpiDhHki6SCQh88-iYWlIAK0mjE0fWCZhcIKjYWECYXN4QDEzMjNkM2Q0NzA3YTU4YWQyZTIzYWRhNGU5ZjFmNDlmNWE1YjRhYzdiNzA4ZWIwZDYxZjczOGY0ODMwN2U4ZWVhY1ghAjRWqhENhLSsdHrr2Cw7AFrKUL9Ffr1XN6RBT6w659lNo2FhAWFzeEA1NmJjYmNiYjdjYzY0MDZiM2ZhNWQ1N2QyMTc0ZjRlZmY4YjQ0MDJiMTc2OTI2ZDNhNTdkM2MzZGNiYjU5ZDU3YWNYIQJzEpxXGeWZN5qXSmJjY8MzxWyvwObQGr5G1YCCgHicY2FtdWh0dHA6Ly9sb2NhbGhvc3Q6MzMzOGF1Y3NhdA';

function selfTest() {
    const t = getDecodedToken(SPEC_EXAMPLE, []);
    assert(t.mint === 'http://localhost:3338', `spec example mint (${t.mint})`);
    assert(t.unit === 'sat', 'spec example unit sat');
    const amounts = t.proofs.map((p) => Number(p.amount)).sort((a, b) => b - a);
    assert(JSON.stringify(amounts) === '[2,1,1]', `spec example amounts ${JSON.stringify(amounts)}`);
    assert(
        t.proofs.every((p) => p.C.length === 66 && /^[0-9a-f]+$/.test(p.C)),
        'spec example C values are 33-byte compressed hex'
    );
    console.log('CONFORMANCE SELFTEST PASSED (cashu-ts surface verified, no hardware)');
}

async function deviceToken() {
    const port = process.env.PORT;
    if (!port) {
        console.error('CONFORMANCE ERROR: set PORT=<cdc device> or TOKEN=cashuB...');
        process.exit(1);
    }
    const run = (args) => execSync(`${MINT_TOOL} --port ${port} ${args}`, { encoding: 'utf8' });
    run(`generate --amount ${AMOUNT}`);
    console.log(`     device: imported ${AMOUNT} sat test token`);
    run('sign');
    console.log('     device: swap signed + DLEQ-verified on device');
    const out = run('export');
    const line = out.split('\n').find((l) => l.startsWith('Token: cashuB'));
    if (!line) die(`no token in export output:\n${out}`);
    return line.slice('Token: '.length).trim();
}

function verify(token) {
    console.log(`     token: ${token.slice(0, 40)}… (${token.length} chars)`);

    // The conformance event itself: a non-micronuts implementation decodes
    // the device token. Rejections here ARE the findings.
    const t = getDecodedToken(token, [DEMO_KEYSET_ID]);
    ok(`cashu-ts ${cashuTsVersion} decoded the device token`);

    assert(t.mint === DEMO_MINT, `mint === ${DEMO_MINT} (got ${t.mint})`);
    assert(t.unit === 'sat', `unit === sat (got ${t.unit})`);
    assert(typeof t.memo === 'string' && t.memo.length > 0, `memo intact ("${t.memo}")`);

    assert(t.proofs.length === 3, `3 proofs (got ${t.proofs.length})`);
    const amounts = t.proofs.map((p) => Number(p.amount)).sort((a, b) => b - a);
    assert(
        JSON.stringify(amounts) === JSON.stringify([16, 4, 1]),
        `denominations 16/4/1 (got ${JSON.stringify(amounts)})`
    );
    const total = amounts.reduce((s, a) => s + a, 0);
    assert(total === AMOUNT, `total ${total} === ${AMOUNT}`);

    for (const [i, p] of t.proofs.entries()) {
        assert(
            typeof p.C === 'string' && p.C.length === 66 && /^[0-9a-f]+$/.test(p.C),
            `proof[${i}] C is 33-byte compressed hex (${p.C?.length ?? '?'} chars)`
        );
        assert(
            p.dleq && /^[0-9a-f]{64}$/.test(p.dleq.e) && /^[0-9a-f]{64}$/.test(p.dleq.s) &&
                /^[0-9a-f]{64}$/.test(p.dleq.r),
            `proof[${i}] dleq {e,s,r} present as hex`
        );
    }

    // Offline NUT-12 verification with the pinned demo key — the
    // verify-if-present path cashu-ts runs on receive.
    for (const [i, p] of t.proofs.entries()) {
        let valid = false;
        try {
            valid = hasValidDleq(p, demoKeyset, { require: true });
        } catch (e) {
            die(`proof[${i}] hasValidDleq threw: ${e.message}`);
        }
        assert(valid, `proof[${i}] NUT-12 DLEQ verifies offline vs pinned demo key`);
    }

    console.log('E2E-CASHUTS-CONFORMANCE PASS: device token accepted by a real wallet');
}

async function main() {
    console.log(`cashu-ts conformance (cashu-ts ${cashuTsVersion} from ${CASHU_CF_ROOT})`);
    if (process.argv.includes('--selftest')) {
        selfTest();
        return;
    }
    const token = process.env.TOKEN ?? (await deviceToken());
    verify(token);
}

main().catch((e) => {
    console.error('E2E-CASHUTS-CONFORMANCE FAIL', e?.message ?? e);
    process.exit(1);
});

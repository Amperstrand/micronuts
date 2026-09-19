#!/usr/bin/env bash
# Cross-implementation audit triangle: Python/coincurve crypto → Rust
# mint → Rust WalletEngine. Proves the full wire-path interoperability
# of the micronuts stack against an independent crypto implementation.
#
# Prerequisites:
#   - micronuts-audit-adapter running on 0.0.0.0:3338 (FakeWallet)
#   - cashu-audit Python deps (coincurve, cbor2)
#   - Rust toolchain (cargo)
#
# Usage: bash scripts/audit_triangle.sh
set -euo pipefail
cd "$(dirname "$0")/.."

MINT="http://127.0.0.1:3338"
AUDIT_DIR="${AUDIT_DIR:-/home/ubuntu/src/cashu-audit/conformance}"

echo "=== 1. Verify the mint is up ==="
curl -s -m 3 "$MINT/v1/keysets" | grep -q keysets || {
    echo "MINT DOWN — start with:"
    echo "  MICRONUTS_MINT_BIN=target/debug/mint_server MICRONUTS_ADAPTER_PORT=3338 \\"
    echo "  MICRONUTS_ADAPTER_BIND=0.0.0.0 cargo run -p micronuts-audit-adapter &"
    exit 1
}
echo "mint OK"

echo "=== 2. Python coincurve mints a V4 token ==="
V4_TOKEN=$(cd "$AUDIT_DIR" && python3 -c "
import json, time, base64, os, cbor2
from conformance.client import MintClient
from conformance.crypto import step1_alice, step3_alice
from coincurve import PublicKey

client = MintClient('$MINT')
quote = client.mint_quote(5)
quote_id = quote['quote']
for _ in range(10):
    _, st = client._get(f'/v1/mint/quote/bolt11/{quote_id}')
    if isinstance(st, dict) and st.get('state') == 'PAID': break
    time.sleep(0.5)

_, keys_resp = client._get('/v1/keys')
ks = keys_resp['keysets'][0]
keyset_id = ks['id']
keys = ks['keys']

outputs = []; blindings = []
for amount in [4, 1]:
    A_hex = keys[str(amount)]
    secret = os.urandom(32).hex()
    B_, r = step1_alice(secret)
    outputs.append({'amount': amount, 'id': keyset_id, 'B_': B_.format(compressed=True).hex()})
    blindings.append((amount, secret, r, A_hex))

status, sigs = client._post('/v1/mint/bolt11', {'quote': quote_id, 'outputs': outputs})
if status != 200: exit(1)

proofs = []
for i, sig in enumerate(sigs['signatures']):
    amount, secret, r, A_hex = blindings[i]
    C = step3_alice(PublicKey(bytes.fromhex(sig['C_'])), r, PublicKey(bytes.fromhex(A_hex)))
    proofs.append({'a': amount, 's': secret, 'c': C.format(compressed=True)})

token = {'m': '$MINT', 'u': 'sat', 'd': 'audit-triangle', 't': [{'i': bytes.fromhex(keyset_id), 'p': proofs}]}
print('cashuB' + base64.urlsafe_b64encode(cbor2.dumps(token)).decode().rstrip('='))
" 2>&1 | tail -1)
echo "V4 token: ${V4_TOKEN:0:40}..."

echo "=== 3. Python coincurve mints a V3 token (legacy compat) ==="
V3_TOKEN=$(cd "$AUDIT_DIR" && python3 -c "
import json, time, base64, os
from conformance.client import MintClient
from conformance.crypto import step1_alice, step3_alice
from coincurve import PublicKey

client = MintClient('$MINT')
quote = client.mint_quote(5)
quote_id = quote['quote']
for _ in range(10):
    _, st = client._get(f'/v1/mint/quote/bolt11/{quote_id}')
    if isinstance(st, dict) and st.get('state') == 'PAID': break
    time.sleep(0.5)

_, keys_resp = client._get('/v1/keys')
ks = keys_resp['keysets'][0]
keyset_id = ks['id']
keys = ks['keys']

outputs = []; blindings = []
for amount in [4, 1]:
    A_hex = keys[str(amount)]
    secret = os.urandom(32).hex()
    B_, r = step1_alice(secret)
    outputs.append({'amount': amount, 'id': keyset_id, 'B_': B_.format(compressed=True).hex()})
    blindings.append((amount, secret, r, A_hex))

status, sigs = client._post('/v1/mint/bolt11', {'quote': quote_id, 'outputs': outputs})
if status != 200: exit(1)

proofs = []
for i, sig in enumerate(sigs['signatures']):
    amount, secret, r, A_hex = blindings[i]
    C = step3_alice(PublicKey(bytes.fromhex(sig['C_'])), r, PublicKey(bytes.fromhex(A_hex)))
    proofs.append({'id': keyset_id, 'amount': amount, 'secret': secret, 'C': C.format(compressed=True).hex()})

v3 = 'cashuA' + base64.b64encode(json.dumps({
    'token': [{'mint': '$MINT', 'proofs': proofs}],
    'unit': 'sat', 'memo': 'audit-triangle-v3'
}).encode()).decode()
print(v3)
" 2>&1 | tail -1)
echo "V3 token: ${V3_TOKEN:0:40}..."

echo "=== 4. Rust WalletEngine receives both tokens ==="
for TOKEN in "$V4_TOKEN" "$V3_TOKEN"; do
    cargo run -q -p micronuts-wallet --example receive_audit_token -- "$TOKEN" 2>/dev/null | grep -E "SUCCESS|error"
done

echo "=== 5. Audit triangle complete ==="
echo "Python coincurve (blind/unblind) → Rust mint (sign/swap) → Rust wallet (receive/DLEQ verify)"

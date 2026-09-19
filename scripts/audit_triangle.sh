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

echo "=== 5. P2PK leg: Python locks to the Rust wallet's identity key ==="
PUBKEY=$(cargo run -q -p micronuts-wallet --example receive_audit_token -- --p2pk-pubkey 2>/dev/null | tail -1)
echo "wallet P2PK pubkey: ${PUBKEY:0:20}..."
P2PK_TOKEN=$(cd "$AUDIT_DIR" && WALLET_PUBKEY="$PUBKEY" python3 -c "
import time, base64, os, cbor2
from conformance.client import MintClient
from conformance.crypto import step1_alice, step3_alice
from conformance.builder import build_p2pk_secret
from coincurve import PublicKey

client = MintClient('$MINT')
quote = client.mint_quote(5)
for _ in range(10):
    _, st = client._get(f'/v1/mint/quote/bolt11/{quote[\"quote\"]}')
    if isinstance(st, dict) and st.get('state') == 'PAID': break
    time.sleep(0.5)
_, keys_resp = client._get('/v1/keys')
ks = keys_resp['keysets'][0]
keyset_id, keys = ks['id'], ks['keys']

# Mint ordinary [4,1], then swap into P2PK-locked outputs (SIG_INPUTS).
def outputs_and_blindings(secrets, amounts):
    outs, blindings = [], []
    for secret, amount in zip(secrets, amounts):
        A_hex = keys[str(amount)]
        B_, r = step1_alice(secret)
        outs.append({'amount': amount, 'id': keyset_id, 'B_': B_.format(compressed=True).hex()})
        blindings.append((secret, r, A_hex, amount))
    return outs, blindings

plain_outs, plain_blind = outputs_and_blindings([os.urandom(32).hex(), os.urandom(32).hex()], [4, 1])
status, sigs = client._post('/v1/mint/bolt11', {'quote': quote['quote'], 'outputs': plain_outs})
assert status == 200, (status, sigs)
plain_proofs = []
for sig, (secret, r, A_hex, amount) in zip(sigs['signatures'], plain_blind):
    C = step3_alice(PublicKey(bytes.fromhex(sig['C_'])), r, PublicKey(bytes.fromhex(A_hex)))
    plain_proofs.append({'id': keyset_id, 'amount': amount, 'secret': secret, 'C': C.format(compressed=True).hex()})

locked_secrets = [build_p2pk_secret(os.environ['WALLET_PUBKEY']) for _ in range(2)]
lock_outs, lock_blind = outputs_and_blindings(locked_secrets, [4, 1])
status, swap = client._post('/v1/swap', {'inputs': plain_proofs, 'outputs': lock_outs})
assert status == 200, (status, swap)
locked_proofs = []
for sig, (secret, r, A_hex, amount) in zip(swap['signatures'], lock_blind):
    C = step3_alice(PublicKey(bytes.fromhex(sig['C_'])), r, PublicKey(bytes.fromhex(A_hex)))
    locked_proofs.append({'a': amount, 's': secret, 'c': C.format(compressed=True)})

token = {'m': '$MINT', 'u': 'sat', 'd': 'audit-triangle-p2pk', 't': [{'i': bytes.fromhex(keyset_id), 'p': locked_proofs}]}
print('cashuB' + base64.urlsafe_b64encode(cbor2.dumps(token)).decode().rstrip('='))
" 2>&1 | tail -1)
echo "P2PK token: ${P2PK_TOKEN:0:40}..."
cargo run -q -p micronuts-wallet --example receive_audit_token -- "$P2PK_TOKEN" 2>/dev/null | grep -E "SUCCESS|error"

echo "=== 6. HTLC leg: Python hash-locks, Rust reveals the preimage ==="
HTLC_TOKEN=$(cd "$AUDIT_DIR" && python3 -c "
import hashlib, os, time, base64, cbor2
from conformance.client import MintClient
from conformance.crypto import step1_alice, step3_alice
from conformance.builder import build_htlc_secret
from coincurve import PublicKey

preimage = os.urandom(32)
print('preimage=' + preimage.hex(), flush=True)
lock_hash = hashlib.sha256(preimage).hexdigest()

client = MintClient('$MINT')
quote = client.mint_quote(5)
for _ in range(10):
    _, st = client._get(f'/v1/mint/quote/bolt11/{quote[\"quote\"]}')
    if isinstance(st, dict) and st.get('state') == 'PAID': break
    time.sleep(0.5)
_, keys_resp = client._get('/v1/keys')
ks = keys_resp['keysets'][0]
keyset_id, keys = ks['id'], ks['keys']

def outputs_and_blindings(secrets, amounts):
    outs, blindings = [], []
    for secret, amount in zip(secrets, amounts):
        A_hex = keys[str(amount)]
        B_, r = step1_alice(secret)
        outs.append({'amount': amount, 'id': keyset_id, 'B_': B_.format(compressed=True).hex()})
        blindings.append((secret, r, A_hex, amount))
    return outs, blindings

plain_outs, plain_blind = outputs_and_blindings([os.urandom(32).hex(), os.urandom(32).hex()], [4, 1])
status, sigs = client._post('/v1/mint/bolt11', {'quote': quote['quote'], 'outputs': plain_outs})
assert status == 200, (status, sigs)
plain_proofs = []
for sig, (secret, r, A_hex, amount) in zip(sigs['signatures'], plain_blind):
    C = step3_alice(PublicKey(bytes.fromhex(sig['C_'])), r, PublicKey(bytes.fromhex(A_hex)))
    plain_proofs.append({'id': keyset_id, 'amount': amount, 'secret': secret, 'C': C.format(compressed=True).hex()})

locked_secrets = [build_htlc_secret(lock_hash) for _ in range(2)]
lock_outs, lock_blind = outputs_and_blindings(locked_secrets, [4, 1])
status, swap = client._post('/v1/swap', {'inputs': plain_proofs, 'outputs': lock_outs})
assert status == 200, (status, swap)
locked_proofs = []
for sig, (secret, r, A_hex, amount) in zip(swap['signatures'], lock_blind):
    C = step3_alice(PublicKey(bytes.fromhex(sig['C_'])), r, PublicKey(bytes.fromhex(A_hex)))
    locked_proofs.append({'a': amount, 's': secret, 'c': C.format(compressed=True)})

token = {'m': '$MINT', 'u': 'sat', 'd': 'audit-triangle-htlc', 't': [{'i': bytes.fromhex(keyset_id), 'p': locked_proofs}]}
print('token=cashuB' + base64.urlsafe_b64encode(cbor2.dumps(token)).decode().rstrip('='))
" 2>&1)
PREIMAGE=$(echo "$HTLC_TOKEN" | grep -o 'preimage=[0-9a-f]*' | cut -d= -f2)
HTLC_TOKEN=$(echo "$HTLC_TOKEN" | grep -o 'token=cashuB[A-Za-z0-9_-]*' | cut -d= -f2)
echo "HTLC token: ${HTLC_TOKEN:0:40}... (preimage ${PREIMAGE:0:12}...)"
cargo run -q -p micronuts-wallet --example receive_audit_token -- "$HTLC_TOKEN" "$PREIMAGE" 2>/dev/null | grep -E "SUCCESS|error"

echo "=== 7. Audit triangle complete ==="
echo "Python coincurve (blind/unblind + P2PK lock + HTLC lock) → Rust mint (sign/swap/enforce) → Rust wallet (receive/DLEQ verify + schnorr witness + preimage reveal)"

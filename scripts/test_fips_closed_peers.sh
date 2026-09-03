#!/usr/bin/env bash
# #57 ADR v1 regression: closed peers.allow / deny ALL posture.
#
# A second FIPS identity attempting to peer with the daemon (and through it
# the responder) must be rejected by the daemon-side ACL before any session
# or mint RPC can flow; the allowlisted sidecar identity must still connect.
#
# Isolation: the fips daemon hardcodes its ACL paths to /etc/fips/peers.{allow,deny},
# so the test daemon runs inside `sudo unshare -m` with a private bind-mount over
# /etc/fips — the host's real ACL state (and the system daemon) is never touched
# (verified by a pre/post directory hash).
#
# Identities are secp256k1 generator multiples (microfips tools/lab_keygen.py
# convention — publicly derivable test vectors, never real keys):
#   daemon G*40  node A (allowlisted sidecar) G*41  node B (attacker) G*42
#
# Benches required: fips + fipsctl binaries (built from /home/ubuntu/src/fips,
# NOT rebuilt here) and microfips fips-handshake probe (target/release/).
# Everything runs on 127.0.0.1:21219 — no hardware needed.

set -euo pipefail

FIPS_BIN=${FIPS_BIN:-/home/ubuntu/src/fips/target/release/fips}
FIPSCTL_BIN=${FIPSCTL_BIN:-/home/ubuntu/src/fips/target/release/fipsctl}
HANDSHAKE_BIN=${HANDSHAKE_BIN:-/home/ubuntu/src/microfips/target/release/fips-handshake}
SIM_BIN=${SIM_BIN:-/home/ubuntu/src/microfips/target/release/microfips-sim}

DAEMON_NSEC=0000000000000000000000000000000000000000000000000000000000000028
DAEMON_NPUB=0391de2f6bb67b11139f0e21203041bf080eacf59a33d99cd9f1929141bb0b4d0b
NODE_A_NSEC=0000000000000000000000000000000000000000000000000000000000000029
NODE_A_NPUB_NIP19=npub102fhtttpv7k4f2n5cc6ge32dx3xvthy5slvywpyat64mp7srerasmevhat
NODE_B_NSEC=000000000000000000000000000000000000000000000000000000000000002a
NODE_B_NPUB_NIP19=npub1l6x3avdukdpjk8d4svlltu3zdkwttejuaeps2kxp3mf68jrvuxhs2ylqv6

PORT=21219
WORK=$(mktemp -d /tmp/micronuts-acl57-XXXX)
DAEMON_PIDS=""
SUDO_NS_PID=""
cleanup() {
    # Word-split on purpose: pgrep can return several PIDs (runuser + fips).
    for pid in $DAEMON_PIDS; do kill "$pid" 2>/dev/null || true; done
    [ -n "$SUDO_NS_PID" ] && kill "$SUDO_NS_PID" 2>/dev/null || true
    rm -rf "$WORK"
}
trap cleanup EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }
pass() { echo "PASS: $*"; }

for bin in "$FIPS_BIN" "$FIPSCTL_BIN" "$HANDSHAKE_BIN" "$SIM_BIN"; do
    [ -x "$bin" ] || fail "missing binary: $bin (set FIPS_BIN/FIPSCTL_BIN/HANDSHAKE_BIN/SIM_BIN)"
done

# The host's real ACL state must be byte-identical before and after the run.
HOST_ETC_HASH_BEFORE=$(sudo -n sha256sum /etc/fips/peers.allow /etc/fips/peers.deny 2>/dev/null | sha256sum || echo none)

mkdir -p "$WORK/ns-fips" "$WORK/run"
chmod 755 "$WORK/ns-fips" "$WORK/run"

# Closed posture: allow ONLY node A; deny everything else.
echo "# acl57 regression — allowlisted sidecar only" > "$WORK/ns-fips/peers.allow"
echo "$NODE_A_NPUB_NIP19" >> "$WORK/ns-fips/peers.allow"
echo "ALL" > "$WORK/ns-fips/peers.deny"

cat > "$WORK/daemon.yaml" <<EOF
node:
  identity:
    nsec: $DAEMON_NSEC
    persistent: false
  heartbeat_interval_secs: 5
  rendezvous:
    lan:
      enabled: false
transports:
  udp:
    bind_addr: "127.0.0.1:$PORT"
EOF

echo "== launching isolated daemon (private /etc/fips mount namespace) on 127.0.0.1:$PORT"
sudo -n unshare -m sh -c "
    mount --bind '$WORK/ns-fips' /etc/fips &&
    exec setsid runuser -u ubuntu -- env XDG_RUNTIME_DIR='$WORK/run' \
        '$FIPS_BIN' --config '$WORK/daemon.yaml'
" > "$WORK/daemon.log" 2>&1 &
SUDO_NS_PID=$!

# Resolve the actual daemon PID (runuser -> fips) and wait for the socket.
DAEMON_PIDS=""
for _ in $(seq 1 50); do
    DAEMON_PIDS=$(pgrep -f "fips --config $WORK/daemon.yaml" || true)
    [ -n "$DAEMON_PIDS" ] && break
    sleep 0.2
done
[ -n "$DAEMON_PIDS" ] || { cat "$WORK/daemon.log"; fail "daemon did not start"; }
pass "daemon up (pid $DAEMON_PIDS, control socket $WORK/run/fips/control.sock)"

for _ in $(seq 1 50); do
    [ -S "$WORK/run/fips/control.sock" ] && break
    sleep 0.2
done
[ -S "$WORK/run/fips/control.sock" ] || { cat "$WORK/daemon.log"; fail "control socket never appeared"; }

probe() { # <nsec> <npub> — prints probe stdout, returns probe exit status
    FIPS_NSEC="$1" FIPS_PEER_NPUB="$2" timeout 20 "$HANDSHAKE_BIN" "127.0.0.1:$PORT" 2>&1 || true
}

echo "== node A (allowlisted): sustained sim link"
FIPS_NSEC="$NODE_A_NSEC" FIPS_PEER_NPUB="$DAEMON_NPUB" \
    "$SIM_BIN" --udp "127.0.0.1:$PORT" --initiator --target 8cd70ac73af37f4f4257d7754d8c3955 > "$WORK/sim-a.log" 2>&1 &
SIM_A_PID=$!
HANDSHAKED=0
for _ in $(seq 1 100); do
    # Default sim builds gate `session: handshake ok` behind the `log`
    # feature — steady RX traffic (phase=0x0) is the wire-level proof.
    grep -qE "handshake ok|entering steady|RX .*phase=0x0" "$WORK/sim-a.log" 2>/dev/null && { HANDSHAKED=1; break; }
    sleep 0.3
done
[ "$HANDSHAKED" = 1 ] || { cat "$WORK/sim-a.log"; kill "$SIM_A_PID" 2>/dev/null || true; fail "allowlisted node A could NOT handshake"; }
pass "node A handshake ok (sim steady)"

echo "== node A visible as connected peer"
CONNECTED=0
for _ in $(seq 1 40); do
    PEERS=$(env XDG_RUNTIME_DIR="$WORK/run" "$FIPSCTL_BIN" -s "$WORK/run/fips/control.sock" show peers 2>&1 || true)
    echo "$PEERS" | grep -q "connected" && { CONNECTED=1; break; }
    sleep 0.3
done
[ "$CONNECTED" = 1 ] || { echo "$PEERS"; kill "$SIM_A_PID" 2>/dev/null || true; fail "node A not listed as connected"; }
pass "fipsctl lists node A connected"

echo "== node B (NOT allowlisted) handshake — must be rejected"
OUT_B=$(probe "$NODE_B_NSEC" "$DAEMON_NPUB")
echo "$OUT_B" | tail -2
echo "$OUT_B" | grep -q "SUCCESS" && { cat "$WORK/daemon.log"; fail "node B handshake SUCCEEDED under closed posture"; }
pass "node B got no handshake (no session, no RPC path)"

echo "== daemon rejected B by ACL (warn log with B's npub)"
grep -q "Rejected peer by ACL" "$WORK/daemon.log" || { cat "$WORK/daemon.log"; fail "no ACL rejection logged for node B"; }
grep -q "$NODE_B_NPUB_NIP19" "$WORK/daemon.log" >/dev/null || fail "ACL rejection does not name node B's npub";
pass "daemon log: 'Rejected peer by ACL' naming node B"

echo "== node A still connected after B's attempt"
CONNECTED=0
for _ in $(seq 1 40); do
    PEERS2=$(env XDG_RUNTIME_DIR="$WORK/run" "$FIPSCTL_BIN" -s "$WORK/run/fips/control.sock" show peers 2>&1 || true)
    echo "$PEERS2" | grep -q "connected" && { CONNECTED=1; break; }
    sleep 0.3
done
kill "$SIM_A_PID" 2>/dev/null || true
[ "$CONNECTED" = 1 ] || fail "node A lost connectivity after B's rejected attempt"
pass "node A unaffected"

echo "== host /etc/fips untouched"
HOST_ETC_HASH_AFTER=$(sudo -n sha256sum /etc/fips/peers.allow /etc/fips/peers.deny 2>/dev/null | sha256sum || echo none)
[ "$HOST_ETC_HASH_BEFORE" = "$HOST_ETC_HASH_AFTER" ] || fail "host /etc/fips changed during the run"
pass "host ACL files unchanged"

echo
echo "ALL CHECKS PASSED: closed peers.allow / deny ALL posture rejects the second identity (#57 ADR v1)"

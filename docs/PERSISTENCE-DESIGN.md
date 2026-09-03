# Micronuts Mint Persistence — Design (2026-09-02)

Milestone: durable mint state on host (phase 1, shipped), reserve wallet
(phase 2), device NVS/LittleFS (phase 3, design only). Tracker: issue #52.
Audit F8 (P1): RAM-only state meant double-spends across restarts and
lost quotes.

## Phase 1 — host file store (shipped)

`src/persist.rs` + `DemoMint::with_state_file(path)`; enabled by the
`MICRONUTS_MINT_STATE_FILE` env var in the stdio server path
(`demo_roles::mint_from_env`). Default (no env var): in-memory behavior,
byte-for-byte unchanged.

**What is durable**: the NUT-04/05 quote tables (state + accounting
fields), the NUT-07 spent set (`Y` hex), and the NUT-09 restore index
(`B_` → blind signature, incl. NUT-12 DLEQ attachments).

**Crash-consistency model**: snapshot-per-mutation. Every mutation of the
four collections writes the whole state as JSON to `<path>.tmp`, fsyncs,
then `rename(2)`s over `<path>` — readers (and a restarting process)
always see either the previous or the new snapshot, never a torn file.
Cost is O(state) per mutation: accepted at prototype scale (hundreds of
entries); compaction or a WAL is the documented upgrade path when a
profile says so.

**Persist-before-use ordering**: in `post_melt`, the snapshot is written
after the input claim + PENDING transition and *before* `pay_invoice` —
a crash mid-payment can re-play the melt quote, never re-mint the inputs.
`post_swap`/`post_mint` persist after the spent-mark/issuance mutations
and before the response is returned, so any signature the client can
observe is already durable.

**Fail-stop policy** (both directions):

- *Boot*: a state file that exists but is corrupt/unreadable panics
  ("refusing to start"). Silently starting empty would resurrect spent
  proofs and re-mint. Restoring from a backup is an operator decision;
  there is no automatic recovery.
- *Runtime*: a persist failure panics. Serving claims that a restart
  would lose is worse than dying: the adapter respawns `mint_server`,
  which loads the last good snapshot. This deliberately does NOT apply
  to post-payment bookkeeping at the upstream seam (upstream.rs keeps
  warnings-only semantics there — a payment that happened stays happened).

**Known limitation — deterministic demo keyset (audit F10)**: the demo
keyset is static, so a wiped state file plus the same keyset means old
tokens verify again. Until keysets rotate (issue backlog), a state-file
wipe must be treated as a mint-reset for issuance purposes; never do it
while tokens are outstanding. Reserve (upstream) secrets are random per
output and cannot be re-derived (see `reserve.rs` module doc).

**Verification**: `tests/persistence.rs` — the restart harness. Drives
real lifecycle ops, drops the mint (crash — no shutdown hook exists,
which is the property under test), reconstructs from the file, asserts:
spent stays spent (+ respend rejected as `TokensAlreadySpent`), quote
state/accounting survives (ISSUED terminal, re-mint rejected), the NUT-09
index restores, melt rollback (proof release + FAILED quote) survives,
corrupt files refuse boot, and boot creates a valid-JSON snapshot
eagerly.

## Phase 2 — reserve wallet durability (next)

`ReserveWallet` state (proofs, cached upstream keyset, bootstrap margin)
serializes to its own file (`MICRONUTS_RESERVE_STATE_FILE`, same atomic
snapshot mechanism) with save points after bootstrap, change recovery,
and selection-removal. Persist ordering mirrors the mint: selected proofs
are removed from disk *before* the upstream melt POST (an ambiguous melt
must not be payable twice from a restored wallet) — the parked-proofs
caveat in `upstream.rs` already models the in-memory version of this.

## Phase 3 — device (NVS / LittleFS, design only)

The ESP32 front-end (`micronuts-esp32-mint`) maps the same snapshot onto
NVS with house rules (bounded strings/blobs, explicit commits per
`tollgate-s3-rs`): the four collections become NVS keys with
length-prefixed entries; a commit is `nvs_commit()` after the blob write,
which NVS makes atomic per key. Whole-state-per-key keeps phase-3 code
shaped like phase 1 (same snapshot struct, different `FileStore`
implementation behind a small trait) — the trait extraction happens when
the second backend lands, not before (YAGNI at one backend).

# Vision QA runbook (z.ai vision over screen artifacts)

**2026-09-14 · first pass evidence below**

Hash fixtures are the **blocking** gate (`screens.fixtures.tsv` ×2: wallet
+ device journey — deterministic, CI-enforced). Vision is the
**advisory semantic pass**: hashes prove "the screen didn't change";
vision proves "the screen is right". They catch disjoint bug classes.

## When to run the pass

- **Fixture drift adoption** — before committing re-recorded hashes,
  have vision review the new PNGs (drift artifacts land in
  `target/shots/` locally via the example, or from CI artifacts).
- New screens or UI changes (journey example gains a step, shotui set).
- Release polish sweeps.
- After ANY display/font/format code change — see first-pass findings:
  it caught bugs the hash gates had been happily pinning for months.

## How (agent workflow)

The reviewing session runs the zai-vision MCP over the PNGs in
`target/shots/`:

1. Generate artifacts: `cargo run -p micronuts-app --example journey
   --features std` (device command-path screens), `cargo run -p
   micronuts-wallet --example shots` (wallet screens; local fonts are
   fine for semantic review — hashes are CI-canonical).
2. `analyze_image` per screen with a QA prompt: truncation/clipping,
   overlap, legibility/contrast, alignment, and for QR screens quiet
   zone + module quality. Ask for `VERDICT: OK | ISSUES` + bullets.
3. **Crop + 2x-upscale suspect regions** (PIL) and re-analyze to verify
   fixes at pixel level — full-screen prompts miss small glyphs.
4. Batch 2–3 calls max with pauses; the API rate-limits (429s and
   timeouts observed on long bursts). Retry with backoff.
5. Triage: real defects → fix + regression test (pin the *shape*, not
   just presence — see below); cosmetic → advisory notes here; nothing
   → record the clean pass.

Vision findings never block CI: they become fixes with tests, or notes.

## First-pass findings (2026-09-14, journey set 8/8 analyzed)

### Found and fixed (pixel-verified after fix)

1. **All nonzero amounts/proof counts rendered invisibly** —
   `u64_to_string` did `9u8 as char` (a control character, not `'9'`),
   so token amounts and PROOFS counts drew nothing on token-info and
   scan-result screens, on hardware, since the screens existed. The
   unit test only ever fed the literal `"255"` to the glyph table,
   never through the converter. Fix: `char::from(b'0' + digit)` +
   `u64_to_string_yields_ascii_digits` regression test.
2. **"ERROR" title rendered "RROR"** — the hero pixel-font glyph table
   had no `E`. First fix bitmap was itself wrong (both verticals — an
   "8"), **caught by a second vision pass**; final glyph pinned by
   shape in `hero_glyphs_cover_error_title`.

### Advisory (non-blocking, owner's call)

- Status screens ("Blinded outputs ready", "Proofs ready", "Scanning…")
  sit slightly above geometric center — reads as intentional optical
  centering; "Proofs ready" text is smallish (~14–16 px) — confirm on
  the physical panel.
- Scanning state shows a static ⓘ glyph — fine for the static fixture,
  consider an activity indicator on hardware.
- MINT row values end ~18 px from the card edge; long mint URLs have no
  visible truncation affordance (ellipsis/scroll) — worth a policy.
- Export-QR caption is mid-gray on near-black — contrast at the low
  end; also confirm long-token caption behavior (this fixture's token
  fits one line).
- Journey export-QR passes the QR checks: ~65 % panel width, clean
  quiet zone, intact finder patterns.

### Pending

- Wallet-screen pass (9 PNGs) — the service rate-limited mid-run;
  re-run per this runbook before the next fixture adoption.

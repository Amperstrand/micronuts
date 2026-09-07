# walletport fuzz

libFuzzer targets: `decode_token` (cashuA/B envelope → V4 CBOR decode),
`cbor_token`, `envelope`, `gate_verify`. Run with `cargo fuzz` (nightly).

## Corpus policy

Corpora are COMMITTED — they are the regression net (60M+ execs, zero
panics; every entry is an input the decoder has already survived).
Growth lands through minimization only:

```bash
cargo fuzz cmin -s none <target>
```

…as periodic atomic `fuzz: corpus refresh` commits. Never commit raw
session growth, never gitignore the directories. `artifacts/` (crash
inputs) must stay empty — a crash gets minimized into a regression test
in the owning crate before anything else happens.

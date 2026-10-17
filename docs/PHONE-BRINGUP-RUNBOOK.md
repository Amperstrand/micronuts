# Phone wallet bring-up runbook — Samsung S20 FE + cashu.me (2026-10-16/17)

The proven path for standing the bench phone up as the NFC/HCE
counterpart for phone↔device token e2e (fork issue nucula#1
"Session 2"). Everything below was executed on the bench host (.208)
over `adb` — no hands touched the phone after the initial USB-debugging
authorization.

## Hardware + enrollment

- **Phone**: Samsung S20 FE (SM-G780F, `r8s`), adb serial
  `RF8NB01ZYAH`, Android 13. USB-debugging authorized once (owner).
- **labgrid**: place `phone-s20` on the `ai-legion-nfc` exporter —
  `USBDevice` match `ID_VENDOR_ID=04e8 ID_MODEL_ID=6860
  ID_SERIAL_SHORT=RF8NB01ZYAH` (in `/etc/labgrid/exporter-nfc.yaml`,
  restart `labgrid-exporter-nfc.service`). Acquire/release verified.
- A second device (Pixel, `ZY326DPC7R`) sits unauthorized on the same
  hub — ignore it.

## Wallet install (cashu.me v0.2.0 — the NFC-capable native app)

1. APK from `github.com/cashubtc/wallet` release `v0.2.0`
   (`cashu-wallet-android-v0.2.0.apk`, ~101 MB). The app registers:
   - `cashu:` URI scheme (BROWSABLE deep links → token receive),
   - an HCE service `com.cashu.me/.Core.NfcReceive.CashuNfcHostApduService`
     (the NFC counterpart for tap-to-send/receive),
   - `android.permission.NFC` granted at install.
2. **The stock APK rejects plain-HTTP mints** (targetSdk 36,
   `usesCleartextTraffic` unset) and **the mint-picker hard-requires
   `https://`** — for bench-local mints you must patch the manifest:
   `apktool d` → add `android:usesCleartextTraffic="true"` to
   `<application>` and flip `extractNativeLibs` to `"true"` (the
   rebuilt zip otherwise fails `INSTALL_FAILED_INVALID_APK`) →
   `apktool b --use-aapt2` → `zipalign -f 4` → `apksigner sign
   --ks ~/.android/debug.keystore --ks-pass pass:android`. All tools
   are on .208 (`apt install zipalign` was needed). Bench artifacts:
   `/root/src/worktrees/apk-patch/cw2-signed.apk`.
   *(Not needed if the phone only uses public HTTPS test mints — which
   is what we settled on.)*

## Onboarding driven over adb (uiautomator, no screenshots)

The app sets `FLAG_SECURE` — **screenshots of the seed screen come out
black**; drive it via `uiautomator dump` + text-bounds taps instead.
Helper: `/root/src/worktrees/phone_flow.py` (dump/tap_text/screen).

1. Keep the phone awake: `svc power stayon true` +
   `settings put system screen_off_timeout 7200000` (otherwise the
   lockscreen races every tap sequence).
2. Launch via `monkey -p com.cashu.me -c android.intent.category.LAUNCHER 1`
   (the MAUI entry is `.App.MainActivity`).
3. **Create Wallet** → seed screen: scroll down → **Tap to reveal** →
   checkbox **"I've written down my seed phrase…"** → **I've Saved My
   Seed Phrase**.
4. **Mint pick**: don't fight the in-app custom-URL field — tap **Skip
   for now**, then **Mints → Add mint**, type the URL, and tap the
   *field-trailing* action icon to submit (the on-screen "Add mint"
   text can be a header, not the button; dump clickables to tell them
   apart).
5. **Funding the wallet**: the in-app invoice flow failed against
   testnut (mint-side `20008` on quote mint — app's cdk 0.17.3-rc vs
   the mint). Working path: mint the token host-side and hand it to
   the phone over a **bench-served browser link** (below).

## The token hand-off that works (adb text input is lossy)

`input text` mangles ~400-char tokens, and `am start -d cashu:…`
truncates. The reliable channel:

1. Mint host-side: `MINT_URL=https://testnut.cashu.space cargo test
   --features payer -- --ignored --nocapture` in the fork rig (greps
   `TOKEN=cashuB…`; write to `/tmp/tok.txt`).
   **Use `testnut.cashu.space` — NOT `.cashu.exchange`** (stale
   keysets there decode as ₿0; owner directive 2026-10-17).
2. Serve a link page (repo: `tokenserve/index.html` pattern):
   `<a href="cashu:<token>">TAP</a>` via
   `python3 -m http.server 8077` on .208 — **open ufw 8077/tcp from
   the bench LAN first** (the phone's browser times out otherwise).
3. On the phone: open `http://192.168.13.208:8077/` in the browser
   (Samsung Internet onboarding: Continue → Next → Start), then tap
   the link **by coordinates** — uiautomator cannot see web-content
   links. The `cashu:` scheme opens the wallet's Receive-Ecash sheet
   with amount/mint/fee parsed.

## State at last light (2026-10-17)

Wallet created; **testnut.cashu.exchange** added (needs swapping for
`.cashu.space` per directive); browser-link token path proven to the
Receive screen (₿0 only because the token was from the stale
`.exchange` mint); NFC/HCE service present but not yet exercised.
**The bench LAN dropped mid-session** (AP SSID "2" off the air,
.208 unreachable — infra outage, watch-loop in place); phone e2e
resumes from exactly here.

## Next steps (the actual e2e)

- [ ] Swap the phone's mint to `https://testnut.cashu.space`
- [ ] Fresh `.space` token → browser link → phone shows real amount →
      Receive → balance > 0
- [ ] Phone **Send → NFC**: presents the HCE tag; m5stick (fork wallet
      reader mode) reads it and redeems — the fork rig's
      `relay_token_over_the_air_via_boltcard` path with a phone
      instead of a boltcard
- [ ] Reverse direction: fork wallet presents (stickup/CE), phone
      receives by tap
- [ ] Micronuts esp32-wallet with `nfc-mfrc522` (PR #108) as a second
      reader counterpart
- [ ] Fold this runbook into `tests/hardware/phone/` as a real suite

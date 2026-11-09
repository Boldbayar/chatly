# Security

- [Reporting a vulnerability](#reporting-a-vulnerability)
- [What Chatly defends against](#what-chatly-defends-against)
- [How the protections are implemented](#how-the-protections-are-implemented)
- [Known limitations](#known-limitations)
- [Open findings](#open-findings)
- [For people distributing builds](#for-people-distributing-builds)

## Reporting a vulnerability

Please **don't open a public issue** for a security problem.

Report it privately through GitHub: open the repository's **Security** tab and choose **Report a vulnerability**. Include:

- what an attacker can do, and what they need first (physical access to an unlocked Mac, a malicious web page, local code execution, and so on)
- steps to reproduce, with the Chatly commit and macOS version
- whether you've seen it exploited

If there's no **Report a vulnerability** button, open an issue that only asks for a private contact, with no details.

Reports are handled on a best-effort basis. Fixes go to the default branch. There are no separately maintained release branches, so only the latest commit on `main` is supported.

## What Chatly defends against

Chatly is designed for one situation: **someone else at your Mac** (a coworker, family member, or IT staff at the desk) who must not read your Instagram and Facebook chats.

| Threat | Defense |
|---|---|
| Someone opens Chatly while you're away | Password or Touch ID lock. Auto-lock on idle, screen lock, sleep, minimize and hide, and optionally app switch. |
| Someone copies your session cookies off the disk | Cookies are only stored encrypted (XChaCha20-Poly1305, key wrapped with Argon2id). Browsing data never touches the disk. |
| Someone guesses the password at the lock screen | Exponential delay after 5 misses, up to 1 hour. Optional erase after 10. |
| Your screen is shared or recorded | The window is excluded from capture. |
| Your work browser's extensions and sync | Chatly uses its own WebKit views, with no extensions and no shared cookie store. |
| A malicious page inside Instagram or Facebook | It can't call Chatly's Rust commands (no permission covers those webviews), can't load non-Meta pages in place, can't open non-web URL schemes, and can't use the camera or microphone. |

No software is perfectly secure. This table describes what Chatly is built to do, not a guarantee. The [known limitations](#known-limitations) and [open findings](#open-findings) below are part of the picture.

## How the protections are implemented

[CONTRIBUTING.md](CONTRIBUTING.md#rules-you-must-keep) lists the rules that keep these properties when the code changes.

| Area | How it works | Where |
|---|---|---|
| Password | Never stored. Argon2id (v1.3, 64 MiB, 3 passes, 1 lane, random 16-byte salt) turns it into a 256-bit key-encryption key. The parameters are stored per vault. Passwords are wrapped in `Zeroizing` as soon as they reach Rust. | `vault.rs`, `main.rs` |
| Vault key | A random 256-bit data key, wrapped by the key-encryption key. A `key_check` value encrypted with the data key verifies a key read from the keychain. | `vault.rs` |
| Encryption | XChaCha20-Poly1305 with a random 192-bit nonce for every write. Each encrypted field has its own associated data (`chatly/v1/…`), so one ciphertext can't be swapped for another. Only the cookies are encrypted. Settings, the wrong-password counter and the last site are plain text. | `vault.rs` |
| Key in memory | Held only while unlocked. On lock, Chatly saves the sessions and then drops the key, unless it was unlocked again in the meantime. Decrypted cookie values are ordinary strings and aren't zeroized after use. | `main.rs` |
| Writing the vault | Written to `vault.json.tmp` with mode 600, synced to disk, then renamed over `vault.json`. The data folder is created with mode 700. A vault that can't be parsed is renamed to `vault.damaged-<time>.json`. | `vault.rs`, `main.rs` |
| Touch ID | A biometrics-only `LAContext` policy (no password fallback). The data key is kept as a generic-password item in the login keychain, together with a SHA-256 hash of the enrolled-fingers state. A different hash turns Touch ID off. | `biometric.rs` |
| Locking | Site webviews are hidden before the lock screen grows, media is paused, and focus moves to the password field. A thread checks every second for idle time (key presses, clicks and scrolls in Chatly), screen lock or sleep (session state, or a gap of more than 20 s), and minimize or hide. Losing focus is handled as a window event. | `main.rs`, `guard.rs` |
| Wrong passwords | 5 free attempts, then a 30-second wait that doubles with each miss, up to 1 hour. Optional erase after 10. | `vault.rs`, `main.rs` |
| Site webviews | A non-persistent WebKit store per site, a Safari user agent, drag and drop disabled, and an injected `content.js` that makes camera and microphone requests fail. Release builds have no web inspector. | `main.rs`, `content.js` |
| IPC | A command exists only if it is listed in `build.rs` (denied by default) and granted in `capabilities/shell.json`, which lists only the local `shell` webview. `withGlobalTauri` is off. Commands re-check state (`require_unlocked`), settings are clamped (`Settings::sanitized`), and the site argument must be a known site. The shell's CSP allows scripts and styles only from `ui/`, no frames, and no network except IPC. | `build.rs`, `capabilities/`, `tauri.conf.json`, `main.rs` |
| Navigation | Every navigation in a site webview, frames included, is checked. Allowed: `https` to Meta's domains and their subdomains, captcha providers, `about:blank` and `about:srcdoc`, and `blob:` URLs with a Meta origin. Everything else is refused silently. | `services.rs` |
| External links | Meta's redirect links are unwrapped, `fbclid`, `igsh` and `igshid` are removed, and Chatly asks before opening (unless turned off). Only `http` and `https` URLs are passed to `/usr/bin/open`. New windows are refused while locked. | `main.rs`, `services.rs`, `content.js` |
| Downloads | Refused while locked. Otherwise saved, unencrypted, to the Downloads folder. | `main.rs` |
| Logging | Errors go to stderr without passwords, keys or cookie names and values. Blocked URLs and cookie counts are logged only in debug builds. | `main.rs` |
| Screen capture | The window is created with content protection, which excludes it from screen capture. | `main.rs` |

## Known limitations

These are deliberate trade-offs or platform limits. Reports that only restate them aren't vulnerabilities, but ideas for closing them are welcome.

1. **Touch ID is enforced by Chatly's code, not by the Secure Enclave.** The vault key is a generic password in the login keychain. Binding it to biometrics in hardware needs the data-protection keychain, which needs a paid Apple Developer signing identity. Anyone with your Mac login password, or software running as you that the keychain trusts, can read the key without a fingerprint. Leave Touch ID off if that matters.
2. **The lockout counter and settings are stored in plain text and aren't authenticated.** Someone who can write to your user files can reset the wrong-password delay or loosen settings such as the auto-lock time. Someone with that much access can also install a keylogger, so this is out of scope. Against a copied `vault.json`, only Argon2id and the strength of your password protect you.
3. **The wrong-password delay uses the system clock.** Changing the clock needs an administrator.
4. **Downloads aren't encrypted.** Files you save from a chat go to `~/Downloads` like in any browser. Downloads are refused while Chatly is locked.
5. **Erase works from the lock screen.** This is the forgot-password path. Anyone at your Mac can erase Chatly and sign you out, but they can't read anything.
6. **Pages keep running while locked.** Locking hides the chats, pauses media and drops the vault key, but the pages, and their session cookies, stay in memory. Software that can read Chatly's memory (a debugger running as you, or root) can read them.
7. **No protection from your employer's infrastructure.** HTTPS inspection, MDM-installed monitoring, EDR and keyloggers can all see your chats. The Work laptop check flags the common cases.
8. **Ad-hoc signing.** Builds aren't notarized, and macOS can't tell a genuine build from a modified one. Build Chatly yourself from source you've read.
9. **No address bar.** Chatly doesn't show which site a page comes from. The navigation allowlist is what keeps other sites' pages, such as fake login pages, out of the app.

## Open findings

Issues and unverified assumptions found while reviewing the code. They are listed here so they can be fixed in the open. None of them lets a web page call into Chatly or read the vault without the password.

| # | Finding | Priority | Suggested fix |
|---|---|---|---|
| 1 | **The Touch ID fingers check can fail open.** `biometric::authenticate` hashes `evaluatedPolicyDomainState`, an API Apple has deprecated, and uses an empty value when it returns nothing. If that happened both when Touch ID was turned on and when unlocking, the hashes would match, and adding a fingerprint would no longer turn Touch ID off. A fingerprint is still required to unlock. | Medium | Refuse to turn on or use Touch ID when the state is empty, and use the newer `domainState` API where it exists. |
| 2 | **`fbsbx.com` is on the allowlist.** It is Meta's domain for user-uploaded content. With no address bar, a page served from it would look like part of Facebook. Which hosts on it Instagram and Facebook actually need inside the app hasn't been checked. | Medium (unverified) | Find out which `fbsbx.com` hosts are needed (with `cargo run`, blocked navigations are logged) and allow only those. |
| 3 | **The wrong-password delay isn't atomic.** `unlock` checks the delay before deriving the key and records a failure after it, so parallel `unlock` calls would all pass the check. Only Chatly's own UI can call `unlock`, and it sends one attempt at a time. Exploiting this needs code running in the shell webview, which release builds don't expose. | Low | Record the attempt before deriving the key, under the same lock. |
| 4 | **Erase leaves set-aside vaults.** `vault.damaged-<time>.json` files stay after Erase. They are encrypted under the old password. | Low | Delete them on erase. Until then, the README tells users where they are. |
| 5 | **Unverified: nothing but the vault reaches the disk.** This relies on WebKit's non-persistent stores. On one Mac in regular use, `vault.json` was the only Chatly file under `~/Library`, but this hasn't been tested across macOS versions. Swap files and crash reports can contain process memory. macOS encrypts swap. | Low | Add a disk check to the self-test, and repeat it on each major macOS release. |
| 6 | **Unverified: every capture method honors the window's content protection.** Exclusion relies on macOS. It hasn't been tested with every recorder, meeting app and mirroring method on current macOS releases, which is why the README says "most". | Low | Test the common capture tools on each major macOS release and list the results. |

## For people distributing builds

- Build with `./scripts/build-release.sh`. A plain `cargo build` embeds your home-folder path, and with it your macOS username, in the binary. The script removes it, then checks the binary in every built `Chatly.app` and fails if the path is still there.
- Distribute **release** builds only. Debug builds include the self-test, a web inspector and extra logging.
- Include [LICENSE](LICENSE) and [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
- If you change the bundle identifier (`com.chatly.desktop` in `src-tauri/tauri.conf.json`), your build uses its own data folder. The Touch ID keychain item is named by `KEYCHAIN_SERVICE` in `src-tauri/src/biometric.rs`. Change that too, so your build doesn't share the key with upstream Chatly.
- The [release checklist](CONTRIBUTING.md#release-checklist) covers signing, notarization and checksums, none of which is set up yet.

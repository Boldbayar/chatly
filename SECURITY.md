# Security

## Reporting a vulnerability

Please **don't open a public issue** for a security problem.

Report it privately through GitHub: open the repository's **Security** tab and choose **Report a vulnerability**. Include:

- what an attacker can do, and what they need first (physical access to an unlocked Mac, a malicious web page, local code execution, and so on)
- steps to reproduce, with the Chatly commit and macOS version
- whether you've seen it exploited

Reports are handled on a best-effort basis. Fixes go to the default branch. There are no separately maintained release branches.

## What Chatly defends against

Chatly is designed for one situation: **someone else at your Mac** (a coworker, family member, or IT staff at the desk) who must not read your Instagram, Facebook and Telegram chats.

| Threat | Defense |
|---|---|
| Someone opens Chatly while you're away | Password or Touch ID lock. Auto-lock on idle, screen lock, sleep, minimize and hide, and optionally app switch. |
| Someone copies your sessions off the disk | Cookies, and the local storage items where Telegram keeps its sign-in, are only stored encrypted (XChaCha20-Poly1305, key wrapped with Argon2id). Other browsing data never touches the disk. |
| Someone guesses the password at the lock screen | Exponential delay after 5 misses, up to 1 hour. Optional erase after 10. |
| Your screen is shared or recorded | The window is excluded from capture. |
| Your work browser's extensions and sync | Chatly uses its own WebKit views, with no extensions and no shared cookie store. |
| A malicious page inside one of the sites | It can't call Chatly's Rust commands (no permission covers those webviews), can't load pages outside its own site's domain list in place (Meta's domains for Instagram and Facebook, web.telegram.org for Telegram), can't reach the other sites' cookies or storage, can't open non-web URL schemes, and can't use the camera or microphone. |

## Known limitations

These are deliberate trade-offs or platform limits. Reports that only restate them aren't vulnerabilities, but ideas for closing them are welcome.

1. **Touch ID is enforced by Chatly's code, not by the Secure Enclave.** The vault key is a generic password in the login keychain. Binding it to biometrics in hardware needs the data-protection keychain, which needs a paid Apple Developer signing identity. Anyone with your Mac login password, or software running as you that the keychain trusts, can read the key without a fingerprint. Leave Touch ID off if that matters.
2. **The lockout counter and settings are stored in plain text and aren't authenticated.** Someone who can write to your user files can reset the wrong-password delay or loosen settings such as the auto-lock time. Someone with that much access can also install a keylogger, so this is out of scope. Against a copied `vault.json`, only Argon2id and the strength of your password protect you.
3. **The wrong-password delay uses the system clock.** Changing the clock needs an administrator.
4. **Downloads aren't encrypted.** Files you save from a chat go to `~/Downloads` like in any browser. Downloads are refused while Chatly is locked.
5. **Erase works from the lock screen.** This is the forgot-password path. Anyone at your Mac can erase Chatly and sign you out, but they can't read anything.
6. **Pages keep running while locked.** Locking hides the chats, pauses media and drops the vault key, but the pages, their session cookies and their local storage stay in memory. Software that can read Chatly's memory (a debugger running as you, or root) can read them.
7. **No protection from your employer's infrastructure.** HTTPS inspection, MDM-installed monitoring, EDR and keyloggers can all see your chats. The Work laptop check flags the common cases.
8. **Ad-hoc signing.** Builds aren't notarized, and macOS can't tell a genuine build from a modified one. Build Chatly yourself from source you've read.
9. **Telegram's saved sign-in is a whole device session.** The auth keys Chatly keeps for Telegram let whoever has them use your Telegram account, the same as a linked desktop app. They're protected like the cookies (encrypted in the vault, in memory while Chatly runs). **Sign out** in Chatly's Settings only deletes them from this Mac. The session stays valid on Telegram's side until you end it in Telegram under **Settings → Devices**, or log out inside Telegram first.
10. **Saved site storage is whatever the page wrote.** Chatly copies a site's local storage as-is (capped at 32 KiB per item and 512 KiB in all) and gives it back to the same origin only. A page can't use this to reach Chatly or another site, but it can put any small data it likes in your vault.

## For people distributing builds

- Build with `./scripts/build-release.sh`. A plain `cargo build` embeds your home-folder path, and with it your macOS username, in the binary. The script removes it and checks that it's gone.
- Distribute **release** builds only. Debug builds include the self-test, a web inspector and extra logging.
- Include [LICENSE](LICENSE) and [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
- If you change the bundle identifier (`com.chatly.desktop` in `src-tauri/tauri.conf.json`), your build uses its own data folder. The Touch ID keychain item is named by `KEYCHAIN_SERVICE` in `src-tauri/src/biometric.rs`. Change that too, so your build doesn't share the key with upstream Chatly.

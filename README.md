# Chatly

Instagram and Facebook in a separate macOS app that only you can unlock. Built with Rust and Tauri 2.

> Chatly is an independent open-source project. It is not affiliated with, endorsed by, or sponsored by Meta Platforms, Inc. Instagram and Facebook are trademarks of Meta Platforms, Inc. Chatly loads the regular instagram.com and facebook.com websites. Using them through Chatly is still subject to Meta's terms.

- [Why not just use a browser?](#why-not-just-use-a-browser)
- [Install](#install)
- [Using Chatly](#using-chatly)
- [How the lock works](#how-the-lock-works)
- [What it can't protect against](#what-it-cant-protect-against)
- [Limits](#limits)
- [Troubleshooting](#troubleshooting)
- [Contributing](#contributing)
- [License](#license)

## Why not just use a browser?

On a work laptop, the browser is the worst place for personal chats:

- **Anyone at the keyboard is already signed in.** A browser has no lock of its own.
- **The profile is often managed.** Work browsers sync history and cookies, and run extensions that IT installs (DLP, monitoring) with full access to the pages you open.
- **Sessions sit on disk in the clear.** Browser cookies can be copied off the machine.

Chatly runs each site in its own embedded WebKit view with a **private, in-memory store**: no extensions, no sync, and no cookies shared with your browser or between the two sites. The only file it writes is an encrypted vault.

Links *from* chats to other sites open in your default browser (Chatly asks first), because Chatly blocks every non-Meta page from loading inside it.

## Install

Chatly is distributed as source. You build it on the Mac you'll use it on.

### Requirements

| What | Version | How to check | How to install |
|---|---|---|---|
| macOS | 13 Ventura or later, Apple silicon or Intel | Apple menu → About This Mac | |
| Xcode Command Line Tools | any recent | `xcode-select -p` | `xcode-select --install` |
| Rust | 1.85 or later | `rustc --version` | [rustup.rs](https://rustup.rs) |
| Tauri CLI | 2.x | `cargo tauri --version` | `cargo install tauri-cli --version "^2" --locked` |

No Node.js or npm is needed. Chatly's UI is plain HTML, CSS and JavaScript.

### Build and install

1. Get the source:

   ```bash
   git clone <repository-url> chatly
   ```

   ```bash
   cd chatly
   ```

2. Build the app:

   ```bash
   ./scripts/build-release.sh
   ```

   The first build takes a few minutes. The script builds `src-tauri/target/release/bundle/macos/Chatly.app` and removes your home-folder path from the binary, so the build doesn't carry your macOS username.

3. Copy it to Applications:

   ```bash
   cp -R src-tauri/target/release/bundle/macos/Chatly.app /Applications/
   ```

4. Open **Chatly** from Applications or Spotlight.

### Update

```bash
git pull
```

```bash
./scripts/build-release.sh
```

```bash
rm -rf /Applications/Chatly.app && cp -R src-tauri/target/release/bundle/macos/Chatly.app /Applications/
```

Your password and saved sessions are kept: they live in `~/Library/Application Support/com.chatly.desktop/`, not inside the app.

### Uninstall

1. Open Chatly, go to **Settings → Erase → Erase everything**. This deletes the vault and the Touch ID key from your keychain.
2. Delete `/Applications/Chatly.app`.
3. Optionally delete the folder `~/Library/Application Support/com.chatly.desktop/`.

If you can't unlock Chatly anymore, use **Forgot your password?** on the lock screen instead of step 1.

## Using Chatly

### First launch

1. Chatly asks you to **set a password**. Use at least 8 characters. Four random words work well. The meter under the field estimates strength.
2. Type it again and choose **Create password**. It takes a moment.
3. Instagram opens. Sign in as usual. Meta may ask you to confirm the login by email, SMS or a captcha. These all work inside Chatly.
4. Switch to Facebook with the second icon in the left rail (or ⌘2) and sign in there too if you use it.

**There is no password recovery.** If you forget it, you can only erase Chatly and sign in to Instagram and Facebook again. Your accounts and messages are not affected.

### Unlocking

- Type your password and press Return.
- Or choose **Use Touch ID** if you turned it on in Settings. When Chatly starts, it asks for your fingerprint right away.
- After 5 wrong passwords, Chatly makes you wait 30 seconds before the next try. Each further miss doubles the wait, up to 1 hour.

### The rail

When unlocked, the narrow column on the left is the **rail**:

| Item | What it does |
|---|---|
| Instagram icon | Shows Instagram. A badge shows the unread count from the page title. |
| Facebook icon | Shows Facebook. |
| Sliders icon | Opens Settings over the chats. |
| Lock icon | Locks Chatly now. |

Each site loads the first time you open it, then stays loaded (hidden) while you use the other one.

### Keyboard shortcuts

| Shortcut | Action |
|---|---|
| ⌘L | Lock now |
| ⌘1 / ⌘2 | Instagram / Facebook |
| ⇧⌘M | Messages (Instagram Direct or Facebook Messages) |
| ⇧⌘H | Home feed |
| ⌘[ / ⌘] | Back / Forward |
| ⌘R | Reload |
| ⌘, | Settings |
| Esc | Close Settings |
| ⌘H | Hide Chatly (locks it, by default) |
| ⌘M | Minimize (locks it, by default) |
| ⌘Q | Quit (saves your sessions first) |

### Opening links

Clicking a link to another website shows **Open this link in your browser?** with the full address. Choose **Open in Browser** or **Cancel**. Chatly removes Meta's redirect and the `fbclid`, `igsh` and `igshid` tracking parameters first. Only `http` and `https` links are ever opened.

### Settings

Open with the sliders icon or ⌘,. Every change is saved immediately.

| Section | Setting | Default | What it does |
|---|---|---|---|
| Auto-lock | Lock after | 5 minutes | Locks after this long without a key press, click or scroll in Chatly. Time spent in other apps counts. Moving the pointer doesn't. 1 minute to 1 hour. |
| | When the screen locks or the Mac sleeps | On | Locks when the screen saver locks, the lid closes, you switch users, or the Mac sleeps. |
| | When Chatly is minimized or hidden | On | Locks on ⌘M, ⌘H or the yellow button. |
| | When you switch to another app | Off | Locks whenever Chatly loses focus. The strictest option. |
| Unlocking | Unlock with Touch ID | Off | Fingerprint only. Your Mac login password can't unlock Chatly. Turns itself off if you add or remove a fingerprint in System Settings. |
| | Password → Change… | | Enter your current password, then the new one twice. Your sessions stay. |
| | Erase after 10 wrong passwords | Off | After 10 wrong passwords in a row, Chatly erases its vault. This stops guessing for good, but anyone at your Mac can trigger it and sign you out. |
| Privacy | Hide from screen sharing and screenshots | On | Most meeting apps and screenshot tools show an empty window. |
| | Ask before opening links | On | Shows the confirmation described above. Off: links open in your browser immediately. |
| | Stay signed in | On | Saves your Instagram and Facebook sessions, encrypted, so you stay signed in after quitting. Off: the saved sessions are deleted and you sign in each time Chatly starts. |
| Accounts | Sign out (Instagram / Facebook) | | Click twice to confirm. Clears that site's cookies in Chatly and in the vault. |
| Work laptop check | Run check | | See [Work laptop check](#work-laptop-check). |
| Erase | Erase everything | | Type `ERASE` to confirm. Signs out of both sites, deletes the vault and the Touch ID key, and removes your password. Chatly returns to first launch. |

### Work laptop check

**Settings → Work laptop check → Run check** reports what on this Mac or network can see past Chatly's lock:

| Check | Warns when |
|---|---|
| Device management | The Mac is enrolled in MDM. Its admins can install software and certificates. |
| Proxy | A system HTTPS proxy, auto-config script or auto-discovery is set. |
| Connection to instagram.com / facebook.com | The certificate isn't from a public certificate authority, which means the network is decrypting HTTPS. Chatly checks against Mozilla's public list of roots instead of the Mac's trust store, so a company root installed by IT is caught. |

If anything is flagged, use your phone for anything private.

### Forgot your password

1. On the lock screen, choose **Forgot your password?**.
2. Type `ERASE` and choose **Erase and start over**.
3. Set a new password and sign in to Instagram and Facebook again.

### Where your data is

| What | Where |
|---|---|
| Encrypted vault (sessions, settings) | `~/Library/Application Support/com.chatly.desktop/vault.json`, readable only by your user (mode 600) |
| Touch ID copy of the vault key (only if Touch ID is on) | Login keychain, item "Chatly" / "touch-id-unlock-key" |
| A vault that couldn't be read | Set aside as `vault.damaged-<time>.json` in the same folder. Chatly then starts fresh. |
| Files you download from chats | Your Downloads folder, **not encrypted** |

Nothing else is written to disk. Browsing data (cache, local storage, IndexedDB) lives in memory and is gone when Chatly quits.

## How the lock works

| | |
|---|---|
| **Password** | Argon2id (64 MiB, 3 passes) turns it into a key that unwraps a random 256-bit vault key. |
| **Vault** | Your Instagram and Facebook cookies, encrypted with XChaCha20-Poly1305. Saved when you lock, when you quit, and every 10 minutes. |
| **Locking** | Chats are hidden and their media paused, the lock screen fills the window, keyboard focus moves to it, and the vault key is dropped from memory. |
| **Auto-lock** | See [Settings](#settings). Pointer movement doesn't count as activity, so a mouse jiggler can't keep Chatly open. |
| **Touch ID** | Biometrics only. The vault key is stored in your login keychain together with a fingerprint of your enrolled fingers. If those change, the stored key is rejected. |
| **Screen sharing** | The window is excluded from screen capture. |
| **Isolation** | Only Chatly's own UI can call into Rust. No permission covers the Instagram or Facebook views, so every call from them is refused (the self-test checks this from instagram.com). Camera and microphone are blocked. |

## What it can't protect against

Chatly stops **people using your Mac**. It doesn't stop **your employer's infrastructure**, or anyone who controls your macOS account:

- **HTTPS inspection.** Many company networks decrypt HTTPS with a certificate that IT installed. Every app on the Mac, Chatly included, can then be read by the network. The [Work laptop check](#work-laptop-check) tests for this.
- **Monitoring software** installed by IT (screen recording, keyloggers, EDR) sees what you see.
- **Your login keychain.** Touch ID is enforced by Chatly, not by the Secure Enclave. Anyone who has your Mac login password, or software running as you that the keychain trusts, can read the Touch ID copy of the vault key without a fingerprint. If that matters, leave Touch ID off.
- **A copy of the vault file.** The wrong-password delays only apply inside Chatly. Someone who copies `vault.json` can guess offline, slowed only by Argon2id. A long password is what protects you.

See [SECURITY.md](SECURITY.md) for the full list and how to report a vulnerability.

## Limits

- Voice and video calls are off. Camera and microphone are blocked.
- No notifications. Unread badges show in the rail only while unlocked.
- Facebook may ask for your Messenger PIN once per launch to restore end-to-end encrypted chat history. That history lives in browser storage, which Chatly deliberately doesn't keep.
- Builds are ad-hoc signed, not notarized. A build runs on the Mac that built it. On other Macs, Gatekeeper blocks it.
- macOS only.

## Troubleshooting

**The build fails at the linking step with an SDK error.** Some Command Line Tools releases ship a beta SDK that the linker can't read. List the installed SDKs:

```bash
ls /Library/Developer/CommandLineTools/SDKs
```

Then create `.cargo/config.toml` in the repository root that pins a non-beta one, for example:

```toml
[env]
SDKROOT = "/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk"
```

`.cargo/` is git-ignored, so this stays on your Mac. Delete it once the tools are updated.

**macOS says Chatly "can't be opened".** The build came from another Mac. Build it yourself. Only if you trust whoever built it: System Settings → Privacy & Security → **Open Anyway**.

**Touch ID turned itself off.** Your fingerprints changed in System Settings, or the keychain item was removed. Unlock with your password and turn Touch ID on again.

**Instagram or Facebook keeps signing me out.** Check that **Settings → Privacy → Stay signed in** is on. Quit Chatly with ⌘Q rather than force-quitting, so it can save your sessions.

**A link doesn't open.** Only `http` and `https` links leave Chatly. Other link types (such as `mailto:` or app links) are ignored on purpose.

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) explains how the code is organized, the rules that keep it secure, and step-by-step recipes for common changes: adding a command, a setting, a site or a shortcut.

## License

[MIT](LICENSE). Third-party crates and their licenses are listed in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

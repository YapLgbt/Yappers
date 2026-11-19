# Yap in a browser

During testing, the public homepage directs interested people to [Yap’s Discord](https://discord.gg/GZMMvk2d3q) for testing updates. The PWA remains available to anyone with its [direct URL](https://yap.lgbt/app/). On iPhone, use Safari’s Share menu and choose **Add to Home Screen** to install it. A browser with WebAssembly, Web Crypto, IndexedDB and Web Locks is required.

The Nearby feed supports posts, replies, voting and bookmarks. Choose a town or use your location to set a radius from 1 to 250 miles. Town searches use the bundled [GeoNames populated-place directory](https://www.geonames.org/export/) on the device. The app rounds selected coordinates to hundredths of a degree. In Groups, browse, create and join local or worldwide communities. You can try a local demo from the welcome screen before creating an account.

Use **You → Edit profile** for a display name, profile details and a cropped photo. Named posts include a profile snapshot. The post menu includes mute, hide, block and report actions. Preferences includes minimum account ages for the feed and inbox, anonymous-post visibility and approved contacts.

Select the author’s name on a post or reply, then choose **Message** on their profile for an end-to-end encrypted conversation with another account on the same home. **Inbox** contains conversations and requests. Message history and encryption keys stay on the messaging devices.

The HTTPS host serves the app and a public enrollment document. The browser then connects to the home over Iroh with the same Rust client and signed event format used on iPhone and Mac. The service worker caches the application files. The wallet, saved feed and signed outbox live in encrypted IndexedDB. After an initial online visit, the app can reopen the saved feed and queue posts or replies. Delivery resumes on refresh, reconnect or return to the app. Browsers that schedule Background Sync can also deliver the outbox with the app closed. Periodic refresh uses the browser’s installed-app scheduling and permission where available. Clearing site storage removes the local wallet, cache and queued work.

## Parity with iOS and Android

The PWA shares the native apps’ Rust account, feed and encrypted messaging protocol. The SvelteKit app uses Material 3 for its navigation, action buttons, tabs and privacy switches. Its theme uses Yap’s plum palette, Geist controls and serif post text. Bits UI handles focus, dismissal and keyboard navigation for sheets, menus and tab panels.

| Workflow | Browser behavior |
|---|---|
| Conversations | Select a headline or post body to read the thread, or select the reply count to jump to Comments. Browser Back returns to the feed position. Choose **Write a reply** to start writing. |
| Offline feeds and uploads | Previously visited group and nearby views are cached on this device. A new radius can use posts from the cached all-cities feed until a fresh result arrives. **You → Preferences → Pending uploads** can discard queued posts, replies, votes and deletions after confirmation. Anything already accepted by the home stays there. |
| Profiles | Drag or pinch the photo to frame the crop, or use the zoom and position sliders. Closing an edited profile asks before discarding changes. |
| Nearby and local groups | GPS uses rounded coordinates and the offline town directory for a nearby place name. An unmatched location is shown as **Current area**. The Local group directory remembers its selected town separately from the feed radius. |
| Privacy | Open **Inbox → Inbox settings** for Privacy or Blocked accounts. Changes apply locally and show a pending message until the home confirms them. An accepted request or block can be queued offline. |

### Browser and device differences

Android schedules background work with WorkManager. iOS schedules its own background refresh. The PWA requests background delivery and periodic refresh when the browser supports them. It also retries when a connection returns or the app reopens. Timing depends on the device and browser.

The PWA supports encrypted backups and recovery keys, and can offer password-manager save and retrieval when the browser exposes those controls. Manage stored keys in the selected password manager. Compatible passkeys can also protect a recovery file. Browser site storage holds the local account and message history, so clearing it has the same practical consequence as clearing app data on a phone.

## Recover the same account

On a fresh visit, **Create an account** walks you through joining yap.lgbt, choosing a display name, claiming a required username and backing up your account. **Enter neighborhood** opens the feed. **Restore with a recovery key** opens a saved account and verifies its username. An account that still needs a username can claim one during setup.

In **Preferences → Back up account**, choose **Save to password manager** in a supported browser, or copy the key into a secure note. Anyone who obtains the key can authorize a device on your account. On a fresh browser profile, choose **Restore with a recovery key** on the feed, paste the key and select **Restore account**. This method works without a passkey or a downloaded file.

The optional **Use a passkey and file instead** section creates an encrypted file with a compatible passkey. Previously downloaded files still work. In the restore dialog, choose **Use a recovery file** and verify the original passkey.

Recovery runs on the device. The home receives the public authorization chain and a signed proof. Each restored device gets a fresh signing key. A recovery key restores identity. For authored history and drafts, use **You → Account data → Save a full backup**. The passphrase-encrypted .age file also opens in the native apps. Choose **Restore a full backup** during onboarding or restore from Account data. A full restore authorizes a new device, replaces prior signing authorization and clears this browser’s existing chat history. Unsent posts can be recovered as private drafts. Provide another home’s invitation in the restore form to move your account.

## Website and PWA

The public site and PWA are SvelteKit routes in `website/src/routes`. Public pages are prerendered HTML. Only the `/app/` route hydrates an interactive UI. Components and browser-facing TypeScript types live in `website/src/lib`. The Iroh client, IndexedDB store, passkeys and recovery operations load only in the browser, from `website/static/app`.

Use Node 24 LTS or newer. For website or PWA UI changes, run:

```sh
cd website
npm ci
npm run check
npm run build
npm run preview
```

`npm run dev` starts Vite with hot reload. Use a production build to exercise offline installation. The module service worker at `/app/background-sw.js` and classic fallback at `/app/sw.js` precache the built UI, WASM and app assets. New releases activate after windows using the previous worker close. Source and enrollment URLs are served by the existing Fly configuration.

## Rebuild the Rust browser client

From the repository root, install the pinned Rust toolchain’s `wasm32-unknown-unknown` target and the version of `wasm-bindgen-cli` matching `Cargo.lock`. On macOS, the build uses Homebrew LLVM for its WebAssembly C dependencies. Set `WASM_BINDGEN` if the matching CLI is outside your PATH.

```sh
scripts/build-web.sh
```

The script regenerates the WASM and bindings in `website/static/app/runtime/`, checks the UI and builds `website/dist/`. The Fly Docker build uses the locked npm dependencies and checked-in browser runtime to generate the same static site. No Node process runs on the deployed home.

## Browser tests

Set `YAP_UPGRADE_FROM` to a previous static site build to test upgrading an installed PWA with an existing identity, bookmark and queued post. The test switches the served build on the same local origin.

For browser integration tests, install Playwright for Node, Chrome and Playwright’s WebKit browser. Make the Playwright package available through Node module resolution or `NODE_PATH`. The following commands create an isolated test home and check posting, replying, saving a post, an offline reload and delivery after reconnecting. They also exchange encrypted messages in both directions with a disposable native Netkit peer using the Apple client ABI, check message requests and blocking, and verify offline, cross-tab delivery. They require Python 3 and access to the Iroh relay.

```sh
scripts/test-web.sh
YAP_BROWSER=webkit scripts/test-web.sh
YAP_QA_PARITY_ONLY=1 scripts/test-web.sh
YAP_BROWSER=webkit YAP_QA_PARITY_ONLY=1 scripts/test-web.sh
```

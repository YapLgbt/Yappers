# Messaging and blocking

Tap the author’s name on a post or reply, then select **Message** on their profile. Both people need to have opened the updated app on the same home so it can register their messaging keys. **Inbox** shows conversations. **Block account** works from a post, a reply or an existing conversation. Open **Inbox → Blocked accounts** to undo a block.

Blocks apply to the account across its aliases and the communities served by the home. The home filters posts and replies before applying feed limits. A block from an anonymous post uses the event ID and returns an opaque block-list entry. Blocking stops new private deliveries both ways and discards queued mail between the two accounts at the home. Existing conversation history stays on each device. Posts in public communities can still be seen by other accounts.

## Feed privacy and message requests

Open **You → Privacy** or **Inbox → Privacy** on iPhone or Mac. Hide anonymous posts and replies, or set a minimum account age of one, seven or thirty days. Account age reflects the first time this home saw the account. Older homes may omit age information. When a minimum age is set, the device treats an unknown age as new. Your own content stays visible. The post menu also lets you mute an entire thread on this device. Privacy lists muted threads so you can bring one back.

Under **Direct messages**, turn on **Approved people only**, or choose a minimum account age for the inbox. New senders who fall outside your preference appear in **Inbox → Requests**. Requests show the sender with the message body hidden. Accept to release the encrypted messages or block the account. Sending a message also approves the recipient. An existing request stays in Requests until accepted, blocked or expired. The initial settings show all ages and anonymous content and allow messages from new senders.

The device applies visibility rules to cached content and suppresses identified blocked senders independently. An anonymous event gives the device a single item it can hide immediately. Its home can resolve the author and hide their other activity after syncing. Federation operators retain their own signed copies and can link an anonymous event to its account. Home and device filters control the experience for people using them.

## Encryption and delivery

Each device uses an Olm account from [Matrix’s vodozemac Rust library](https://github.com/matrix-org/vodozemac). Public one-time keys enable first contact while the recipient is offline. A portable account’s authorized signing device signs each prekey and ciphertext, binding it to the account and home. The client verifies the named contact’s original signed post and pins the remote messaging identity when opening a session. Olm advances session keys as the two people exchange messages.

The sender saves the advanced ratchet, message history and encrypted outgoing packet together before submitting the packet. The home deduplicates retries and holds ciphertext for up to 30 days. Each receiver saves its ratchet and mailbox cursor together before acknowledging receipt. The home can see the participants, timestamps, traffic volume and packet sizes. DM bodies and display labels travel inside the ciphertext. The server keeps mail separate from replicated feed events.

On iPhone, Yap requests background refresh for the encrypted inbox and queued sends. iOS chooses when to run it. Leaving the app with outgoing messages also requests a short upload window. On Mac the running app syncs with the window hidden, minimized or closed, and resumes faster polling when focused. Quitting stops the Mac poller. **Sent** means the home accepted the encrypted packet. The client keeps up to 500 recent messages and up to 16 queued sends. Each account currently has one messaging device per home. Conversations and ratchets stay on that device and are excluded from portable profile exports. A second or replacement device cannot take over the existing mailbox in this release.

The first installation to sync an account’s private inbox registers its messaging keys, even before sending a message. After recovering the account on another installation, use the registered installation to read and send messages. The native apps keep any locally cached conversation history available, and continue syncing posts, privacy settings and blocks on the recovered installation. Restoring the recovery key or a portable profile backup does not transfer the inbox keys.

On iOS the message file uses AES-GCM with a device-only Keychain key and is excluded from backup. The macOS development client uses its existing owner-only data directory and atomic file writes for message state. Older client versions keep their existing feed behavior.

If Inbox says the account is linked to another installation, open Yap on the installation that first registered its messaging keys. Registration can happen in the background before the first conversation. A fresh installation generates new inbox keys even when you restore the same account. The feed remains available when the home rejects those keys.

## Verification

Run `cargo test --locked --workspace private_mailbox_ratchets_offline_and_enforces_account_blocks` for the isolated two-client exchange. It exercises offline first contact, a reply, exact-ciphertext retry, anonymous blocking and unblocking.

The opt-in `deployed_home_private_exchange` test uses two fresh accounts and sends messages only between them. Set `YAP_SMOKE_ENROLLMENT` to a downloaded public enrollment document and run `cargo test --locked --workspace deployed_home_private_exchange -- --ignored`.

Run `NETKIT_TEST_SCOPE=messaging scripts/test-netkit-ios.sh` for the iPhone simulator test. A private local test account waits for the iOS message and replies. The tests reopen Yap, check message persistence, accept an unsolicited encrypted request, and exercise blocking and unblocking from the inbox.

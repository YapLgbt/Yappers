# Profiles and headline posts

The iPhone composer requires a headline (1–80 UTF-16 units) and body (1–500), plus a topic. Replies remain body-only. Existing body-only posts remain readable; the app does not invent headlines for historical user posts. Untouched fictional seed posts are enriched by matching both ID and original body during migration.

## Expressive profiles

**You → Edit profile** saves a display name (2–30), optional photo, pronouns (40), bio (280), free-form identity (80), current joys/interests (120), and up to six “here for” tags. Labels are self-described, never inferred. Photos and identity fields are optional. Save is explicit; Cancel guards unsaved edits. On iPhone, the editor opens in a native sheet. Its actions stay in place while its fields scroll. Changes persist across app launches. Reset demo data restores the sample feed while preserving your saved name and profile.

The system PHPicker presents the iOS photo library without requesting unrestricted library access. It returns only the image the person chooses. iPhone and Mac show a circular crop preview where people can drag their photo and adjust the zoom. Choosing **Use crop** sets a 96×96 JPEG of at most 3072 bytes (4096 base64 characters) on the draft. **Save profile** applies it. Rendering removes source metadata, including GPS. The prototype stores the thumbnail with the device-local profile; there is no full-resolution upload or image-hosting dependency. Remove photo clears the draft and Save commits it. Cancellation preserves the previous photo. Kotlin bounds remote image dimensions to 128×128. Profiles with an empty photo field display the [Rook Cubimal](https://github.com/tinyspeck/glitch-items/tree/master/cubimals/npc_cubimal_rook) by Tiny Speck (CC0), rendered from the [SVG edition](https://opengameart.org/content/glitch-inhabitants-and-cubimals-svg) on a lavender background. The default portrait is bundled with the app and displayed locally. A missing shared profile or unreadable custom image uses a neutral placeholder or omits the portrait.

Tap a named byline to see the author’s shared profile or start a direct message. Named posts carry a signed snapshot including the photo; anonymous posts carry **no profile**, photo, pronouns, bio, identity, interests, or tags. Rust rejects anonymous payloads that include one. A stable signing key still links anonymous posts at the protocol level; this is presentation privacy, not unlinkability.

Profile edits apply to future named posts. Historical posts retain the exact snapshot signed at creation. Removing a photo or changing a name does not retract copies already replicated to other homes. Demo and live feeds use a device-local editable display name and profile. Onboarding suggests a two-word display name. Users can ask for another or write their own.

## Federated usernames

On yap.lgbt, claim a username during account setup. Closing Yap during this step resumes it when you return. If you restore an account that already has a username, the home verifies the existing claim. After setup you can post anonymously or by name. A named byline shows both the display name and the home-qualified username, such as **Hazel** and **hazel@yap.lgbt**. Older posts keep their original bylines.

The home verifies a device's signed account proof and atomically reserves one permanent, case-insensitive username per account. The local part accepts 2–24 ASCII letters, digits, or underscores and starts with a letter. Account recovery preserves ownership. The HA ledger replicates claims together with the rest of the home state.

For each new named event the home signs a separate username receipt bound to its home ID and the event ID. Its transaction stores that receipt with the accepted event. Federation log pages carry the receipt, and receiving peers verify it before committing the page. Anonymous events omit the receipt. The original signed event and acceptance formats remain compatible with existing archives.

Yap pins the public home ID for yap.lgbt. Operators can configure other trusted home/domain pairs through the server's `YAP_USERNAME_DOMAINS` environment variable, using comma-separated `publicHomeID=domain` entries. Assign each domain to one verified public home. Configure both members of a standby pair identically. Private homes without a mapped domain retain their existing posting behavior. A receiver displays the domain it trusts for the originating home.

## Wire compatibility

The composer sends a `postV2` action inside a version-4 signed event. The action contains these fields.

```json
{
  "kind": "postV2",
  "body": "Bring a book and a blanket.",
  "topic": "Friends",
  "anonymous": false,
  "displayName": "Tender comet",
  "headline": "Quiet company in the park",
  "profile": {
    "pronouns": "they / them",
    "bio": "Finding my people.",
    "identity": "Queer",
    "interests": "Books, chosen family, long walks",
    "hereFor": ["Friends", "Community"],
    "photo": ""
  }
}
```

Rust serializes fields in struct order. Anonymous posts omit `profile`. The v5 signature commits to the action and its random nonce. Deleting the post removes the opening, including the profile snapshot. See [the signing and deletion format](netkit/deletion.md#what-a-peer-verifies).

The Rust CLI accepts `post --headline 'Quiet company' --body 'Bring a book' ...`. Omitting `--headline` produces a body-only post. The native composer requires both fields. Remote projections and pending outbox entries preserve the headline and profile.

Feed responses remain within Netkit's 1 MiB frame: the server drops the oldest complete conversations from the response until it fits. Stored events are retained. Profile thumbnails and fields are bounded at ingestion; the transport remains bounded. This prototype has no media CDN, original photo download, or separate attachment protocol.

## Implementation

- `CommunityProfile.kt`: serializable optional identity fields, limits, photo picker interface.
- `ProfileScreen.kt`: profile editor, photo controls, author sheet and avatars.
- `EditorSheet.kt`: shared safe-area-height editor for profile and post forms; the field list owns vertical gestures.
- `FeedRepository.kt`: local persistence, legacy seed migration, anonymous omission.
- `AppleProfilePhotoLibrary.swift`: limited-access system picker and thumbnail encoding.
- `NetkitController.kt` / `NetkitCache.kt`: signed action creation, outbox and projection mapping.
- `netkit/src/model.rs`: event validation, signing domains and profile snapshot projection.
- `netkit/src/server.rs`: transport-size-aware feed projection.

## Adding comments

Conversation detail uses a full-width multiline text area with a 500 UTF-16-unit limit, visible counter, posting name, and Post comment action. Return inserts a newline. Empty/whitespace drafts cannot submit; duplicate submission is disabled while saving. Failure keeps the draft. On successful durable local save/enqueue, the app clears the field, dismisses the keyboard, and scrolls the matching new comment into view. It does not jump to unrelated incoming comments while someone is reading. Explicit keyboard dismissal keeps an unfinished draft.

Queued live comments appear immediately with Waiting to sync and retain their stable ID when the home acknowledges them. Local demo comments are immediately saved. The server's bounded conversation projection keeps the latest 200 comments in canonical chronological order, so new activity is retained when older history exceeds the window. Earlier event records remain stored; this is a projection limit, not deletion or pagination. The UI uses existing reply events on the wire; no ALPN/signature change is required. The pending flag is client-local cache/UI state.

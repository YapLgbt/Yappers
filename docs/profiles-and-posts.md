# Profiles and headline posts

The iPhone composer requires a headline (1–80 UTF-16 units) and body (1–500), plus a topic. Replies remain body-only. Existing body-only posts remain readable; the app does not invent headlines for historical user posts. Untouched fictional seed posts are enriched by matching both ID and original body during migration.

## Expressive profiles

**You → Edit profile** saves a display name (2–30), optional photo, pronouns (40), bio (280), free-form identity (80), current joys/interests (120), and up to six “here for” tags. Labels are self-described, never inferred. Photos and identity fields are optional. Save is explicit; Cancel guards unsaved edits. On iPhone, the editor opens in a native sheet. Its actions stay in place while its fields scroll. Changes persist across app launches. Reset demo data restores the sample feed while preserving your saved name and profile.

The system photo picker returns the selected image. The editor crops it to a square 1024×1024 JPEG, strips source metadata including GPS, and adjusts JPEG quality to fit 288 KiB. The Home stores the portrait separately, and the app loads it when someone opens the profile. Feed avatars use a smaller preview. Older Homes receive a preview within their advertised limits. Existing uploads retain their saved resolution until the person uploads a new photo. Remove photo clears the draft and Save commits it. Cancellation preserves the previous photo. Profiles with an empty photo field display the bundled Rook Cubimal on a lavender background. A missing or unreadable image uses a neutral placeholder. [Rook Cubimal by Tiny Speck (CC0)](https://github.com/tinyspeck/glitch-items/tree/master/cubimals/npc_cubimal_rook) · [SVG edition](https://opengameart.org/content/glitch-inhabitants-and-cubimals-svg).

Tap a named byline to open the author's current public profile. The home authenticates edits against the account and assigns a revision. Named posts and replies resolve that account on each feed read. A changed name, photo or bio appears on earlier conversations and saved posts after synchronization. Offline devices show their last synchronized view. Anonymous bylines omit the public account reference and profile. The home still authenticates the event's signer.

A stale revision prompts the editor to reload the latest profile. Repeating a save with identical values after an interrupted acknowledgement returns the committed revision. The full-home standby replicates this table together with account state. Public replicas refresh their moderated projection, and direct federation peers fetch changed profile revisions for named authors in shared communities.

## Federated usernames

On yap.lgbt, claim a username during account setup. Closing Yap during this step resumes it when you return. If you restore an account that already has a username, the home verifies the existing claim. After setup you can post anonymously or by name. A named byline shows both the display name and the home-qualified username, such as **Hazel** and **hazel@yap.lgbt**. The username receipt stays bound to its original event.

The home verifies a device's signed account proof and atomically reserves one permanent, case-insensitive username per account. The local part accepts 2–24 ASCII letters, digits, or underscores and starts with a letter. Account recovery preserves ownership. The HA ledger replicates claims together with the rest of the home state.

For each new named event the home signs a separate username receipt bound to its home ID and the event ID. Its transaction stores that receipt with the accepted event. Federation log pages carry the receipt, and receiving peers verify it before committing the page. Anonymous events omit the receipt. Protocol 6 uses a separate event signing domain from the retired public home generation.

Yap pins the public home ID for yap.lgbt. Operators can configure other trusted home/domain pairs through the server's `YAP_USERNAME_DOMAINS` environment variable, using comma-separated `publicHomeID=domain` entries. Assign each domain to one verified public home. Configure both members of a standby pair identically. Private homes without a mapped domain retain their existing posting behavior. A receiver displays the domain it trusts for the originating home.

## Wire compatibility

The composer sends a `postV2` action inside a version-4 signed event. The action contains these fields.

```json
{
  "kind": "postV2",
  "body": "Bring a book and a blanket.",
  "topic": "Friends",
  "anonymous": false,
  "displayName": "Neighbor",
  "headline": "Quiet company in the park"
}
```

The posting clients sign the content and its anonymous or named posting choice. The home rejects profile thumbnails or fields embedded in a new submission. Named account details come from the current profile record.

The Rust CLI accepts `post --headline 'Quiet company' --body 'Bring a book' ...`. Omitting `--headline` produces a body-only post. The native composer requires both fields. Remote projections and pending outbox entries preserve the headline and profile.

Feed responses stay within Netkit’s 1 MiB frame. The Home leaves older conversations out of that response until it fits and retains their events. Full portraits have a dedicated request. Private attachments travel in encrypted chunks on a separate request from the feed.

## Implementation

- `CommunityProfile.kt`: serializable optional identity fields, limits, photo picker interface.
- `ProfileScreen.kt`: profile editor, photo controls, author sheet and avatars.
- `EditorSheet.kt`: shared safe-area-height editor for profile and post forms; the field list owns vertical gestures.
- `FeedRepository.kt`: local persistence, legacy seed migration, anonymous omission.
- `AppleProfilePhotoLibrary.swift`: limited-access system picker and thumbnail encoding.
- `NetkitController.kt` / `NetkitCache.kt`: signed action creation, outbox and projection mapping.
- `netkit/src/model.rs`: event validation, signing domains and feed models.
- `netkit/src/server.rs`: transport-size-aware feed projection.

## Adding comments

Conversation detail uses a full-width multiline text area with a 500 UTF-16-unit limit, visible counter, posting name, and Post comment action. Return inserts a newline. Empty/whitespace drafts cannot submit; duplicate submission is disabled while saving. Failure keeps the draft. On successful durable local save/enqueue, the app clears the field, dismisses the keyboard, and scrolls the matching new comment into view. It does not jump to unrelated incoming comments while someone is reading. Explicit keyboard dismissal keeps an unfinished draft.

Queued live comments appear immediately with Waiting to sync and retain their stable ID when the home acknowledges them. Local demo comments are immediately saved. The server's bounded conversation projection keeps the latest 200 comments in canonical chronological order, so new activity is retained when older history exceeds the window. Earlier event records remain stored; this is a projection limit, not deletion or pagination. The UI uses existing reply events on the wire; no ALPN/signature change is required. The pending flag is client-local cache/UI state.

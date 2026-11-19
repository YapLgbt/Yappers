# Android UI audit

Reviewed November 19, 2026 on the API 37 x86-64 emulator. Yap renders its Android UI with Compose and Material 3. The largest convention gaps were in app navigation and activity recreation. The branded feed, serif posts, and shared Kotlin screens work within Android's component model.

## Findings and changes

| Priority | Finding | Result |
|---|---|---|
| Major | The four destinations used hand-drawn tabs at every Android window size. | AndroidX now selects a Material navigation bar or rail for the current window. Both use the same destination order, labels, and selection. [Adaptive navigation](https://developer.android.com/develop/ui/compose/layouts/adaptive/build-adaptive-navigation) |
| Major | Changing system text size while writing a post closed the editor and lost the draft. | The post editor retains its draft in memory across activity recreation, scoped to its account and group. Discarding the draft clears it. Process death ends that in-memory lifetime. [Compose state lifespans](https://developer.android.com/develop/ui/compose/state-lifespans) |
| Moderate | The inbox, privacy, and conversation screens repeated the brand header and used inconsistent return controls. | Detail screens use a Material top app bar with a leading, direction-aware Up arrow. System Back returns to the originating screen. The compose sheet uses Android's keyboard dismissal and preserves the dirty-draft confirmation. |
| Moderate | Transient feedback used a fixed timer and a manually positioned overlay. | Android displays notices through the Scaffold's Material snackbar host, which accounts for placement and accessibility timeouts. |
| Moderate | The activity window always requested a light theme, and the launcher received a flat square icon. | The window follows the system's light or dark appearance. The launcher receives adaptive foreground, background and monochrome layers using the existing Yap lettering. |
| Minor | Selecting a profile image used a generic content request. | AndroidX requests an image through the system photo picker and provides the compatible document-picker fallback. [Android photo picker](https://developer.android.com/training/data-storage/shared/photopicker) |

Existing platform integrations worth keeping include Android Credential Manager, the system document picker for backups, coarse-location permissions, and Material text fields, switches, dialogs and menus. The app's plum palette and reading typography are expressed through its Material theme.

Android uses the AndroidX Material 3 1.5.0-rc01 expressive theme for motion, shape and typography. The toolbar, post actions and empty states use rounded Material Symbols. Topic and profile chips pair a checkmark with the selected color. Privacy settings use Material list items with a single accessible control per row.

## Verification

- Visited Nearby, Groups, Saved, and You on a phone. Opened Inbox from Groups, opened Requests, used system Back, then used Up to return to Groups. Separately exercised the system edge-back gesture.
- Inspected the phone feed, Privacy screen and post editor with the keyboard open. Checked dark appearance and 130% system text. On a tablet-sized window, checked rail selection, opening a conversation, and returning to the feed.
- Opened the system photo picker from Edit profile and canceled. Typed into a new post, dismissed the keyboard with Back, and verified the discard prompt. Changed font size and theme with an unfinished test post, then discarded it and verified that the next editor opened empty.
- Built the Android debug APK, ran Android app lint, and compiled the shared JVM UI. Lint reported zero errors. Four existing warnings cover the target SDK, two Android-test dependency versions, and backup configuration. A fifth notes that Android reads the predictive-Back manifest opt-in on API 33 and above.
- Checked the expressive controls in Groups, Saved, Inbox, You and Privacy. Entered a headline and discarded it. Switched appearance with Privacy open, and resized the Groups search from a phone to a tablet with 130% text. The screen and search survived those changes.

- Exercised up, down, and toggle-off in an isolated Android build with local starting scores of 9, 99 and −10. Each score stayed on the same center pixel between the arrows. A Compose regression test also covers wider values.
- Inspected first-run create and restore screens in a separate install, the post composer above the IME, group creation, inbox subpages, account recovery, and Help & about. Captured Privacy during an edge-back gesture with You visible behind it, then verified the destination after release. Also checked navigation with the Android animator scale set to zero.

## Remaining coverage

AndroidX Navigation 3 now owns detail transitions and system Back. TalkBack traversal and announcements on a physical device, and foldable hinge behavior, still need device testing. Profile edits and unsent direct messages keep their drafts in composition-local state and warrant a separate recreation check. The adaptive icon's final mask and themed appearance depend on the user's launcher.

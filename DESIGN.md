# Yap design system

## Type and color

The lowercase **yap** wordmark uses Leckerli One. Center its visible lettering in the app icon and navigation bar. Use Geist Serif for public posts and replies, including drafts. The shared UI uses Geist Sans. iPhone navigation and form controls use system text styles and follow Dynamic Type.

The light theme uses plum actions on pale lavender. The dark theme uses lilac actions on deep plum. Use each theme’s paired foreground color for selected controls. The shared [color schemes](composeApp/src/commonMain/kotlin/app/yappers/App.kt), [reading styles](composeApp/src/commonMain/kotlin/app/yappers/YapTypography.kt) and [PWA theme](website/src/routes/app/skeleton.css) define the implementation values.

## Feed and profiles

- Lay posts on a continuous reading surface with a thin rule between conversations. Align bylines, titles, body text and replies to a common leading edge.
- Tapping the post opens the thread at the original post. Tapping the reply count takes the reader to the comments. Keep votes beside the reply count and the bookmark at the far right.
- A named byline opens the author’s profile. Put **Message** there. Show anonymous attribution as plain text. The profile on a named post shows the details its author shared at publication.
- On iPhone, **Create an account** previews the profile before the user claims a username. Photo, pronouns, bio, identity, interests and “Here for” tags are optional.
- After posting a comment, reveal it and dismiss the keyboard. Incoming comments preserve the reader’s scroll position.

## Platform controls

| Surface | Conventions |
|---|---|
| iPhone | UIKit owns the tab bar, navigation stacks, menus, sheets and text editing. Inline form rows share the page background. Individual inputs can use a rounded system fill. Present the system photo picker before the crop editor. |
| Android | Compose Material 3 provides the compact navigation bar, adaptive navigation rail, app bars and snackbars. Detail screens use a leading Up action and system Back. Editors keep actions reachable above the system keyboard. Use the Android photo picker for profile images and adaptive launcher icon layers for the Yap wordmark. |
| Mac | Use Compose Material controls. A sidebar replaces bottom navigation at 900 dp. Wide windows can show a thread beside its feed. |
| PWA | Use the Yap theme for Skeleton components, with native HTML controls for text entry and selection. Keep navigation destinations stable as the bottom bar becomes a sidebar. Keep keyboard focus inside open dialogs and return it to the trigger on dismissal. |

On Android, use the Material expressive motion scheme and rounded Material Symbols for navigation and in-content actions. Reserve solid plum for primary actions and selected controls. Use tonal surfaces for supporting actions and settings rows, and a scalloped symbol for empty destinations. Writing and reading share the same Geist Serif styles.

Android keeps its destination bar or rail in place during tab changes. Detail screens run in AndroidX Navigation 3, with Material spatial springs for scale and effects springs for opacity. Back gestures use the navigation host’s predictive transition. Use paired press and selected shapes on buttons, chips and related choices.

In the Android feed, group both vote controls around a centered tabular score. The reply bubble shows its count and exposes a full accessibility label. Put the primary writing action within thumb reach, with space to scroll the last post clear of it. In the editor, keep the publish action above the keyboard.

On You, use the Estuary print to frame the profile portrait. Show the person’s posts first, then group privacy, community and recovery under Settings. The guide and the two legal links belong together under Help & about.

## Website imagery

Use human-made imagery. Prefer photographs with a verified CC0 dedication and credit the photographer and source alongside the image. Yap’s procedural pride prints are also part of the visual identity.

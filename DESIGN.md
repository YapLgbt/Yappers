# Yap design system

## Type and color

The lowercase **yap** wordmark uses Leckerli One. Center its visible lettering in the app icon and navigation bar. Use Geist Serif for public posts and replies, including drafts. The shared UI uses Geist Sans. iPhone navigation and form controls use system text styles and follow Dynamic Type.

The light theme uses plum actions on pale lavender. The dark theme uses lilac actions on deep plum. Use each theme’s paired foreground color for selected controls. The shared [color schemes](composeApp/src/commonMain/kotlin/app/yappers/App.kt), [reading styles](composeApp/src/commonMain/kotlin/app/yappers/YapTypography.kt) and [PWA theme](website/src/routes/app/material.css) define the implementation values.

## Feed and profiles

- Lay posts on a continuous reading surface with a thin rule between conversations. Align bylines, titles, body text and replies to a common leading edge.
- Tapping the post opens the thread at the original post. Tapping the reply count takes the reader to the comments. Keep votes beside the reply count and the bookmark at the far right in the native apps.
- A named byline opens the author’s profile. Put **Message** there. Show anonymous attribution as plain text. The profile on a named post shows the details its author shared at publication.
- On iPhone, **Create an account** previews the profile before the user claims a username. Photo, pronouns, bio, identity, interests and “Here for” tags are optional.
- After posting a comment, reveal it and dismiss the keyboard. Incoming comments preserve the reader’s scroll position.

## Platform controls

| Surface | Conventions |
|---|---|
| iPhone | UIKit owns the tab bar, navigation stacks, menus, sheets and text editing. Inline form rows share the page background. Individual inputs can use a rounded system fill. Present the system photo picker before the crop editor. |
| Android and Mac | Use Compose Material controls. Editor actions stay reachable while fields scroll and the keyboard is open. On desktop, a sidebar replaces bottom navigation at 900 dp. Wide windows can show a thread beside its feed. |
| PWA | Use Material controls and Bits UI dialogs. Keep keyboard focus inside open dialogs and return it to the trigger on dismissal. |

## Website imagery

Use human-made imagery. Prefer photographs with a verified CC0 dedication and credit the photographer and source alongside the image. Yap’s procedural pride prints are also part of the visual identity.

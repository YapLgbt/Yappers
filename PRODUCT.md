# Using Yap

Yap is for queer adults who want to find neighbors, make plans and talk about what’s happening nearby. You can read the public feed and post without adding a photo or filling out a profile.

## What you can do

| Feature | iPhone, Android and Mac | Web |
|---|---|---|
| Posts | Read, post, reply and save conversations. | Read, post, reply and save conversations. The installed web app can queue posts and replies offline. |
| Nearby and groups | Browse all cities, choose a town and radius, or request a location fix. Join city-based or worldwide groups. | Choose a town or GPS area, set a mile radius, and browse, create or join local and worldwide groups. |
| Private messages | Named accounts on the same home can exchange encrypted messages. | Select the author’s name on a post or reply and choose **Message** on their profile. |
| Recovery | From **You → Account recovery**, export an encrypted backup, recover an account or move homes. | Use **You → Account data** for an encrypted full backup, restore or home move. Recovery keys and passkey files are also available. |

The [PWA parity notes](docs/web-app.md#parity-with-ios-and-android) describe conversation navigation, offline feeds, privacy controls and browser-specific recovery and background behavior.

## Homes and identity

Your home accepts signed posts and shares them with peers authorized for that community. Yap operates the default public home at yap.lgbt. Independent operators can run the server. Outside peering with the Yap-operated home is closed. The [peering covenant](https://yap.lgbt/peering/) describes the admission terms.

Named posts can include a snapshot of your profile. Anonymous posts omit the profile, but homes can link posts by their signing identifiers. When you share a posting area, Yap rounds the coordinates before sending them. Choosing **All cities** omits location on new posts. [Profiles](docs/profiles-and-posts.md) · [Location sharing](docs/public-home.md#location-behavior)

Your account has a signing identity independent of the home you choose. A backup lets you authorize a fresh device. On the web, bookmarks, drafts and pending posts stay in the original browser. [Account ownership](docs/ownership.md) · [Browser recovery](docs/web-app.md#recover-the-same-account)

# Yap home and shared Rust code

The first-party home executable in `backend/` and the home implementation in `netkit/src/` marked `SPDX-License-Identifier: AGPL-3.0-only` are licensed under the GNU Affero General Public License, version 3 only. This includes federation, peering admission, home storage and deletion, directory services, server-side messaging and the standby implementation. The same license covers the home build, packaging and service scripts included in the server source release.

The remaining first-party Rust code in `netkit/src/`, including the client, wire types, signing and transport, is available under either the MIT License or the Apache License, version 2.0, at your option. Building with the server feature includes the AGPL-covered home code. Building client-only Netkit excludes those modules.

The license texts are in `LICENSES/`. Dependencies retain their respective licenses. Public peering terms apply to admission by Yap. This release preserves permissions granted under prior releases.

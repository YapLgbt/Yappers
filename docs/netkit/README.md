# Netkit

Netkit is the Rust library used by Yap clients and homes. It signs events, verifies receipts and exchanges community records over Iroh. The current transport negotiates `yappers/netkit/5`. New posts and replies use event version 4. When a home processes a valid deletion, it keeps the signed evidence and discards the text and its salt. [Wire protocol](protocol.md) · [Event commitments and deletion](deletion.md)

## Guides

- [Architecture](architecture.md) follows a post from the device outbox through home acceptance and peer replication.
- [Groups](groups.md) explains how homes discover a group and obtain permission to host it. Yap’s public home also applies its [peering policy](../public-home.md#outside-peering).
- [Account ownership](../ownership.md) covers device authorization, encrypted backups and moving homes.
- [Privacy and moderation](security.md) describes what members, operators and peers can see.
- [Native client integration](ios-ffi.md) documents the Rust C ABI, local storage and crash recovery.
- [Home operations](operations.md) covers local setup, deployment and standby recovery.
- [Testing](validation.md) lists the checks for Rust, the native apps and the web client.

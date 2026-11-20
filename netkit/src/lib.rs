// SPDX-License-Identifier: MIT OR Apache-2.0
//! Shared Yap networking and user-owned portability foundation.
//!
//! Mobile builds use client/signing/transport plus the owned C ABI in [`ffi`].
//! The optional `server` feature adds Turso, SQLite and scoped federation; those dependencies
//! never enter the iOS client archive. Application wire rules live in [`model`].
//! See `docs/netkit/` for architecture, canonical signing bytes, and operating limits.

#[cfg(feature = "server")]
pub mod account_store;
#[cfg(feature = "server")]
pub mod backup;
#[cfg(target_arch = "wasm32")]
mod browser;
pub mod client;
#[cfg(feature = "server")]
pub mod directory;
mod dm_client;
#[cfg(feature = "server")]
mod feed_page;
#[cfg(feature = "server")]
mod feed_replies;
#[cfg(feature = "server")]
mod feed_votes;
#[cfg(not(target_arch = "wasm32"))]
pub mod ffi;
pub mod groups;
#[cfg(feature = "server")]
mod home_sql;
#[cfg(feature = "server")]
mod mailbox;
pub mod messaging;
pub mod model;
#[cfg(feature = "server")]
mod native_admin;
#[cfg(feature = "server")]
mod native_content;
#[cfg(feature = "server")]
mod native_guard;
#[cfg(feature = "server")]
mod native_primary;
#[cfg(feature = "server")]
mod native_projection;
#[cfg(feature = "server")]
mod native_transfer;
#[cfg(feature = "server")]
mod notifications;
#[cfg(feature = "server")]
pub mod peering;
pub mod portability;
#[cfg(feature = "server")]
mod privacy;
mod private_group_client;
#[cfg(feature = "server")]
mod private_group_store;
pub mod private_groups;
#[cfg(feature = "server")]
mod profiles;
#[cfg(feature = "server")]
mod push;
#[cfg(feature = "server")]
mod redaction;
pub mod report;
#[cfg(feature = "server")]
pub mod report_inbox;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
pub mod store;
pub mod takeover;
pub mod transport;
#[cfg(feature = "server")]
mod usernames;
pub use model::*;
/// Pinned transport dependency; update together with Cargo.toml and Cargo.lock.
pub const IROH_VERSION: &str = "1.4.1";
/// Changing incompatible application wire rules requires a new ALPN.
pub const ALPN: &[u8] = b"yappers/netkit/6";
/// The same JSON wire protocol with independent concurrent RPC streams.
pub const MULTIPLEX_ALPN: &[u8] = b"yappers/netkit/7";
/// Maximum encoded JSON request or response, including signatures and framing data.
pub const MAX_FRAME: usize = 1024 * 1024;
/// Deadline for an entire request/response exchange.
pub const RPC_TIMEOUT_SECS: u64 = 20;
/// Maximum scanned log rows per federation page, including rows outside peer scope.
pub const MAX_PAGE_ENTRIES: u32 = 100;

/// Transport RPC version; new submissions and replication require committed v4 events.
pub const PROTOCOL_VERSION: u8 = 6;

// Fits beside feed metadata in one wire frame, including on older homes.
pub(crate) const DELETION_PAGE: usize = 10_000;

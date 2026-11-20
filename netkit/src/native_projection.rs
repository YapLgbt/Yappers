// SPDX-License-Identifier: AGPL-3.0-only
//! Cache each displayed body under that event's independently retired key.
//! Parent links and named bylines are inside the encrypted record.
use crate::{
    backup,
    model::{Envelope, RemotePost, RemoteReply},
    native_guard,
    store::Store,
};
use anyhow::{Context, Result, ensure};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize)]
enum Item {
    Post(Box<RemotePost>),
    Reply {
        parent: String,
        reply: Box<RemoteReply>,
    },
}

fn key(store: &Store, community: &str, id: &str) -> Result<Option<[u8; 32]>> {
    let raw: Option<String> = store
        .db
        .query_row(
            "SELECT envelope FROM events WHERE id=? AND community=?
         AND json_extract(envelope,'$.deletion') IS NULL",
            [id, community],
            |r| r.get(0),
        )
        .optional()?;
    let Some(raw) = raw else { return Ok(None) };
    let event: Envelope = serde_json::from_str(&raw)?;
    let key = native_guard::opening_key(&store.db, &event, false, false)?;
    Ok(Some(blake3::derive_key(
        "yap native per-event public projection v1",
        &key,
    )))
}

fn seal(store: &Store, community: &str, id: &str, item: &Item) -> Result<(String, String)> {
    let key = key(store, community, id)?.context("projection is missing its signed event")?;
    let sealed = backup::seal(&serde_json::to_vec(item)?, &key)?;
    Ok((id.to_owned(), hex::encode(sealed)))
}

pub(crate) fn encode(store: &Store, community: &str, posts: &[RemotePost]) -> Result<String> {
    if !store.is_native() {
        return Ok(serde_json::to_string(posts)?);
    }
    let mut records = Vec::new();
    for post in posts {
        let mut post = post.clone();
        let replies = std::mem::take(&mut post.replies);
        let parent = post.id.clone();
        records.push(seal(
            store,
            community,
            &parent,
            &Item::Post(Box::new(post)),
        )?);
        for reply in replies {
            let id = reply.id.clone();
            records.push(seal(
                store,
                community,
                &id,
                &Item::Reply {
                    parent: parent.clone(),
                    reply: Box::new(reply),
                },
            )?);
        }
    }
    Ok(serde_json::to_string(&records)?)
}

pub(crate) fn decode(store: &Store, community: &str, raw: &str) -> Result<Vec<RemotePost>> {
    if !store.is_native() {
        return Ok(serde_json::from_str(raw)?);
    }
    let mut posts: Vec<RemotePost> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();
    for (id, encoded) in serde_json::from_str::<Vec<(String, String)>>(raw)? {
        let Some(key) = key(store, community, &id)? else {
            continue;
        };
        let item = serde_json::from_slice(&backup::unseal(&hex::decode(encoded)?, &key)?)?;
        match item {
            Item::Post(post) => {
                ensure!(
                    post.id == id && post.replies.is_empty(),
                    "invalid cached post"
                );
                positions.insert(id, posts.len());
                posts.push(*post);
            }
            Item::Reply { parent, reply } => {
                ensure!(reply.id == id, "invalid cached reply");
                if let Some(&index) = positions.get(&parent) {
                    posts[index].replies.push(*reply);
                }
            }
        }
    }
    Ok(posts)
}

// SPDX-License-Identifier: AGPL-3.0-only
//! Authenticated, capability-addressed transport for opaque encrypted chunks.
use crate::{
    attachments::{ATTACHMENT_CHUNK_BYTES, CHUNK_OVERHEAD, chunk_count, chunk_size, id_bytes},
    model::now,
    store::Store,
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use rusqlite::{OptionalExtension, params};

impl Store {
    pub(crate) fn put_attachment(
        &self,
        owner: &str,
        id: &str,
        size: usize,
        part: usize,
        ciphertext: &str,
    ) -> Result<()> {
        id_bytes(id)?;
        let expected = chunk_size(size, part)? + CHUNK_OVERHEAD;
        ensure!(
            ciphertext.len() <= expected.div_ceil(3) * 4,
            "invalid encrypted attachment part"
        );
        let bytes = STANDARD.decode(ciphertext)?;
        ensure!(bytes.len() == expected, "invalid encrypted attachment part");
        let tx = self.db.unchecked_transaction()?;
        if part == 0 {
            self.expire_incomplete_attachments(now())?;
            self.db.execute(
                "INSERT INTO encrypted_attachments(id,owner,size,created) VALUES(?,?,?,?)
                   ON CONFLICT(id) DO NOTHING",
                params![id, owner, size, now()],
            )?;
        }
        let (uploader, recorded_size, received): (String, usize, usize) = self
            .db
            .query_row(
                "SELECT owner,size,received FROM encrypted_attachments WHERE id=?",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .context("start the attachment upload first")?;
        ensure!(
            uploader == owner && recorded_size == size && part <= received,
            "invalid attachment upload"
        );
        if part < received {
            let previous: Vec<u8> = self.db.query_row(
                "SELECT ciphertext FROM encrypted_attachment_chunks WHERE id=? AND part=?",
                params![id, part],
                |row| row.get(0),
            )?;
            ensure!(previous == bytes, "attachment part was already committed");
        } else {
            self.db.execute(
                "INSERT INTO encrypted_attachment_chunks(id,part,ciphertext) VALUES(?,?,?)",
                params![id, part, bytes],
            )?;
            self.db.execute(
                "UPDATE encrypted_attachments SET received=received+1, created=? WHERE id=?",
                params![now(), id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn get_attachment(&self, id: &str, part: usize) -> Result<String> {
        id_bytes(id)?;
        let (size, received): (usize, usize) = self
            .db
            .query_row(
                "SELECT size,received FROM encrypted_attachments WHERE id=?",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .context("attachment unavailable")?;
        ensure!(
            received == chunk_count(size)? && part < received,
            "attachment unavailable"
        );
        let bytes: Vec<u8> = self.db.query_row(
            "SELECT ciphertext FROM encrypted_attachment_chunks WHERE id=? AND part=?",
            params![id, part],
            |row| row.get(0),
        )?;
        Ok(STANDARD.encode(bytes))
    }

    fn expire_incomplete_attachments(&self, at: i64) -> Result<()> {
        let cutoff = at - 86400;
        self.db.execute(
            "DELETE FROM encrypted_attachment_chunks WHERE id IN (
                 SELECT id FROM encrypted_attachments WHERE created<? AND received*?<size)",
            params![cutoff, ATTACHMENT_CHUNK_BYTES],
        )?;
        self.db.execute(
            "DELETE FROM encrypted_attachments WHERE created<? AND received*?<size",
            params![cutoff, ATTACHMENT_CHUNK_BYTES],
        )?;
        Ok(())
    }
}

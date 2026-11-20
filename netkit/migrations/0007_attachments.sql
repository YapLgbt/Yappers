CREATE TABLE IF NOT EXISTS encrypted_attachments(
    id TEXT PRIMARY KEY,
    owner TEXT NOT NULL,
    size INTEGER NOT NULL,
    created INTEGER NOT NULL,
    received INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS encrypted_attachments_owner ON encrypted_attachments(owner);
CREATE TABLE IF NOT EXISTS encrypted_attachment_chunks(
    id TEXT NOT NULL,
    part INTEGER NOT NULL,
    ciphertext BLOB NOT NULL,
    PRIMARY KEY(id,part)
);

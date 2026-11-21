// SPDX-License-Identifier: AGPL-3.0-only
//! Single durable commit authority. A prepared local SQLite transaction is speculative until
//! the authority records its state and response together. The snapshot revision, authenticated
//! node identity, and one-use operation permit fence every commit.
use super::*;
use crate::takeover::FullHomeStandby;
use rusqlite::{Connection, OptionalExtension, params};
use std::{path::Path, time::Instant};

/// Export a consistent committed SQLite image even while the standby is serving clients.
pub fn export_ha(path: &Path, grant: &FullHomeStandby, output: &Path) -> Result<()> {
    grant.validate(&grant.home)?;
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let guard = path.with_added_extension("deletion-guard");
    ensure!(guard.is_file(), "missing deletion guard");
    db.execute(
        "ATTACH DATABASE ? AS guard",
        [guard.to_str().context("invalid guard path")?],
    )?;
    let tx = db.unchecked_transaction()?;
    crate::store::migrations::guard_version(&tx, "guard")?;
    let (image, key): (Vec<u8>, Vec<u8>) = tx.query_row(
        "SELECT image,key FROM head JOIN membership USING(singleton)
         JOIN deletion_epoch USING(singleton) JOIN guard.state USING(id,epoch)
         WHERE home=? AND standby=?",
        params![grant.home, grant.standby.id.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let sealed = crate::backup::seal_snapshot(&tx, &image, &key)?;
    tx.commit()?;
    crate::backup::publish(&sealed, output)
}

/// Apply an operator's local moderation to the latest durable state. The same revision
/// comparison used by request workers fences any in-flight computation on the old view.
pub fn moderate_ha(path: &Path, grant: &FullHomeStandby, event: &str, hide: bool) -> Result<()> {
    grant.validate(&grant.home)?;
    ensure!(
        event.len() == 64 && event.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid event ID"
    );
    let mut db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    db.busy_timeout(Duration::from_secs(5))?;
    db.execute_batch("PRAGMA secure_delete=ON; PRAGMA synchronous=EXTRA")?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    migrate_receipt_disclosures(&tx)?;
    let (rev,image):(u64,Vec<u8>)=tx.query_row("SELECT revision,image FROM head JOIN membership USING(singleton) WHERE home=?1 AND standby=?2",
        params![grant.home,grant.standby.id.to_string()],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let mut store = Store {
        db: Connection::open_in_memory()?.into(),
    };
    store.restore_snapshot(&image)?;
    store.moderate(event, hide)?;
    let snapshot = store.snapshot()?;
    ensure!(
        snapshot.len() <= snapshot_limit()?,
        "home exceeds replication capacity"
    );
    let next = rev.checked_add(1).context("revision exhausted")?;
    ensure!(
        tx.execute(
            "UPDATE head SET revision=?1,image=?2 WHERE revision=?3",
            params![next, snapshot, rev]
        )? == 1,
        "authority changed, retry moderation"
    );
    tx.commit()?;
    Ok(())
}
// Size this transport allocation to the memory and bandwidth of both operators.
fn snapshot_limit() -> Result<usize> {
    let mib = match std::env::var("YAP_HA_SNAPSHOT_MIB") {
        Ok(raw) => raw
            .parse::<usize>()
            .context("invalid YAP_HA_SNAPSHOT_MIB")?,
        Err(std::env::VarError::NotPresent) => 64,
        Err(error) => return Err(error.into()),
    };
    let bytes = mib
        .checked_mul(1024 * 1024)
        .context("snapshot budget overflow")?;
    ensure!(
        (1..=1_000_000_000).contains(&bytes),
        "snapshot budget exceeds SQLite blob range"
    );
    Ok(bytes)
}
const HEADER_LIMIT: usize = 2 * MAX_FRAME + 4096;
const OPERATION_TTL: Duration = Duration::from_secs(8);
const RECEIPT_TTL: i64 = 300;
pub(super) const CONTROL: u8 = 0x81;

pub enum HaRole {
    Primary,
    Standby {
        transport_key: String,
        ledger: PathBuf,
    },
}
pub struct HaOptions {
    pub grant: FullHomeStandby,
    pub role: HaRole,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
enum ReceiptPolicy {
    /// A write acknowledgment can be replayed without running the write again.
    Durable,
    /// Retry against current state when a saved read is stale.
    Read,
    /// Preserve deduplication for calls that both disclose data and change state.
    #[default]
    ReadAndWrite,
    /// Only a terminal deletion confirmation is a durable acknowledgment.
    AccountDeletion,
}

impl ReceiptPolicy {
    fn for_request(call: &Request) -> Self {
        use crate::{messaging::SocialOp, private_groups::Operation as PrivateGroup};
        match call {
            Request::PublicReplicaFeed { .. }
            | Request::DeletionsAfter { .. }
            | Request::PeerProfiles { .. }
            | Request::AccountLookup { .. }
            | Request::AccountDeleteAvailability { .. }
            | Request::Feed { .. } => Self::Read,
            Request::Social { op, .. } => match op.as_ref() {
                SocialOp::PrivacyFeed { .. }
                | SocialOp::Feed { .. }
                | SocialOp::FeedReplies { .. }
                | SocialOp::Contact { .. }
                | SocialOp::Profiles { .. }
                | SocialOp::ProfilePortrait { .. }
                | SocialOp::AttachmentGet { .. }
                | SocialOp::PrivacyStatus
                | SocialOp::NotificationInbox
                | SocialOp::NotificationThread { .. }
                | SocialOp::Blocks
                | SocialOp::OpenPush { .. } => Self::Read,
                SocialOp::Inbox { .. } | SocialOp::Claim { .. } => Self::ReadAndWrite,
                SocialOp::SetPush { .. }
                | SocialOp::ReadNotification { .. }
                | SocialOp::MuteNotifications { .. }
                | SocialOp::DismissRequest { .. }
                | SocialOp::SetProfile { .. }
                | SocialOp::SetProfilePortrait { .. }
                | SocialOp::AttachmentPut { .. }
                | SocialOp::ClaimUsername { .. }
                | SocialOp::SetPrivacy { .. }
                | SocialOp::Approve { .. }
                | SocialOp::Block { .. }
                | SocialOp::Unblock { .. }
                | SocialOp::Publish { .. }
                | SocialOp::Deliver { .. } => Self::Durable,
            },
            Request::PrivateGroup { op, .. } => match op.as_ref() {
                PrivateGroup::Roster { .. }
                | PrivateGroup::RosterAfter { .. }
                | PrivateGroup::Read { .. } => Self::Read,
                PrivateGroup::Create { .. }
                | PrivateGroup::Change { .. }
                | PrivateGroup::Publish { .. } => Self::Durable,
            },
            Request::AnnounceAccount { .. }
            | Request::AccountRegister { .. }
            | Request::AccountExport { .. }
            | Request::Directory { .. }
            | Request::CovenantPeers { .. }
            | Request::EnrollHost { .. }
            | Request::CreateGroup { .. }
            | Request::JoinGroup { .. }
            | Request::Groups { .. }
            | Request::GroupsAfter { .. }
            | Request::PullPublicProjection { .. }
            | Request::PullDeletions { .. }
            | Request::Pull { .. } => Self::ReadAndWrite,
            Request::AccountDelete { .. } => Self::AccountDeletion,
            Request::AccountImport { .. }
            | Request::Submit { .. }
            | Request::ReportAvailability
            | Request::SubmitReport { .. }
            | Request::ReportStatus { .. }
            | Request::ServerSource
            | Request::HomeOperation { .. } => Self::Durable,
        }
    }

    fn durable(self, result: &str) -> bool {
        self == Self::Durable
            || (self == Self::AccountDeletion
                && matches!(
                    serde_json::from_str::<Response>(result),
                    Ok(Response::AccountDeleted { .. })
                ))
    }
}

const EXPIRED_RECEIPT: &str =
    r#"{"type":"error","code":"expired","message":"Content changed; refresh"}"#;
const DURABLE_RECEIPT: i64 = -1;

fn migrate_receipt_disclosures(db: &Connection) -> Result<()> {
    // Keep the established ledger tables usable during a rolling upgrade.
    // The trigger also sees revisions published by the offline moderation CLI.
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS receipt_epoch(singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL);
         INSERT OR IGNORE INTO receipt_epoch VALUES(1,0);
         CREATE TABLE IF NOT EXISTS receipt_visibility(remote TEXT NOT NULL, id TEXT NOT NULL, revision INTEGER NOT NULL, PRIMARY KEY(remote,id));
         CREATE TRIGGER IF NOT EXISTS receipt_epoch_publish AFTER UPDATE OF image ON head BEGIN
             UPDATE receipt_epoch SET revision=NEW.revision WHERE singleton=NEW.singleton;
         END;
         CREATE TRIGGER IF NOT EXISTS receipt_visibility_expire AFTER DELETE ON receipts BEGIN
             DELETE FROM receipt_visibility WHERE remote=OLD.remote AND id=OLD.id;
         END;",
    )?;
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
struct Operation {
    remote: String,
    id: String,
    digest: String,
    #[serde(default)]
    receipt_policy: ReceiptPolicy,
}
impl Operation {
    fn new(remote: &str, id: String, bytes: &[u8]) -> Result<Self> {
        ensure!(
            id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid request ID"
        );
        Ok(Self {
            remote: remote.into(),
            id,
            digest: blake3::hash(bytes).to_hex().to_string(),
            receipt_policy: ReceiptPolicy::Durable,
        })
    }
}
#[derive(Serialize, Deserialize)]
enum Control {
    Seed,
    Open {
        op: Operation,
        known: Option<u64>,
    },
    Commit {
        op: Operation,
        permit: String,
        result: String,
    },
}
#[derive(Serialize, Deserialize)]
enum Answer {
    Ready { revision: u64, permit: String },
    Committed { revision: u64, result: String },
    Busy,
    Seeded,
    Refused,
}
struct Permit {
    token: String,
    node: bool,
    op: Operation,
    revision: u64,
    issued: Instant,
}
struct Ledger {
    _lock: std::fs::File,
    _guard_lock: std::fs::File,
    db: Connection,
    permit: Option<Permit>,
}
impl Ledger {
    fn open(path: &Path, grant: &FullHomeStandby) -> Result<Self> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)?;
        ensure!(
            f.metadata()?.permissions().mode() & 0o077 == 0,
            "private ledger permissions required"
        );
        let authority_lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path.with_added_extension("ha-lock"))?;
        authority_lock
            .try_lock()
            .context("another HA authority already owns this ledger")?;
        let mut db = Connection::open(path)?;
        db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA secure_delete=ON; PRAGMA synchronous=EXTRA;
            CREATE TABLE IF NOT EXISTS membership(singleton INTEGER PRIMARY KEY CHECK(singleton=1), home TEXT NOT NULL, standby TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS head(singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL, image BLOB NOT NULL);
            CREATE TABLE IF NOT EXISTS seed_receipt(digest TEXT PRIMARY KEY);
            CREATE TABLE IF NOT EXISTS receipts(remote TEXT NOT NULL, id TEXT NOT NULL, digest TEXT NOT NULL, response TEXT NOT NULL, expires INTEGER NOT NULL, PRIMARY KEY(remote,id));")?;
        // Install the authority-local migration atomically before serving.
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        migrate_receipt_disclosures(&tx)?;
        tx.commit()?;
        crate::backup::attach(&mut db, path)?;
        let guard_lock = crate::backup::lock_authority(path)?;
        crate::backup::verify_epoch(&db)?;
        if let Some(image) = db
            .query_row("SELECT image FROM head", [], |r| r.get::<_, Vec<u8>>(0))
            .optional()?
        {
            deletion_set(&image).context("legacy authority image requires a clean v5 cutover")?;
        }
        let standby = grant.standby.id.to_string();
        db.execute(
            "INSERT OR IGNORE INTO membership VALUES(1,?1,?2)",
            params![grant.home, standby],
        )?;
        ensure!(
            db.query_row(
                "SELECT home=?1 AND standby=?2 FROM membership WHERE singleton=1",
                params![grant.home, standby],
                |r| r.get::<_, bool>(0)
            )?,
            "ledger belongs to a different authorized home pair"
        );
        Ok(Self {
            _lock: authority_lock,
            _guard_lock: guard_lock,
            db,
            permit: None,
        })
    }

    fn exchange(
        &mut self,
        is_primary: bool,
        cmd: Control,
        image: Vec<u8>,
    ) -> Result<(Answer, Vec<u8>)> {
        ensure!(
            image.len() <= snapshot_limit()?,
            "home exceeds replication capacity"
        );
        let mut data = Vec::new();
        let answer = match cmd {
            Control::Seed => {
                ensure!(
                    is_primary && image.starts_with(b"SQLite format 3\0"),
                    "invalid initial snapshot"
                );
                let tx = self.db.transaction()?;
                let digest = blake3::hash(&image).to_hex().to_string();
                if let Some(previous) = tx
                    .query_row("SELECT digest FROM seed_receipt", [], |r| {
                        r.get::<_, String>(0)
                    })
                    .optional()?
                {
                    ensure!(previous == digest, "a different seed has already committed");
                    return Ok((Answer::Seeded, Vec::new()));
                }
                tx.execute("INSERT INTO seed_receipt VALUES(?)", [digest])?;
                tx.execute(
                    "INSERT INTO head(singleton,revision,image) VALUES(1,1,?1)",
                    [&image],
                )?;
                remember_guard_state(&tx, &image, true)?;
                for deletion in deletion_set(&image)? {
                    deletion.remember(&tx)?;
                }
                tx.commit()?;
                Answer::Seeded
            }
            Control::Open { op, known } => {
                // Pin the revision and its receipt before an operator can publish
                // a concurrently updated moderation decision on another connection.
                let tx = self.db.unchecked_transaction()?;
                let (rev, disclosure_revision): (u64, i64) = tx.query_row(
                    "SELECT head.revision,receipt_epoch.revision FROM head JOIN receipt_epoch USING(singleton)",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                let prior: Option<(String, String, Option<i64>)> = tx.query_row(
                    "SELECT r.digest,r.response,v.revision FROM receipts r LEFT JOIN receipt_visibility v USING(remote,id)
                     WHERE r.remote=?1 AND r.id=?2 AND r.expires>=?3",
                    params![op.remote, op.id, now()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                ).optional()?;
                if let Some((digest, result, receipt_revision)) = prior {
                    ensure!(digest == op.digest, "request ID reused");
                    let valid = match receipt_revision {
                        Some(revision) => {
                            revision == DURABLE_RECEIPT || revision == disclosure_revision
                        }
                        // An unmarked content-bearing receipt has no proof of current access.
                        None => op.receipt_policy.durable(&result),
                    };
                    if valid || op.receipt_policy != ReceiptPolicy::Read {
                        return Ok((
                            Answer::Committed {
                                revision: rev,
                                result: if valid {
                                    result
                                } else {
                                    EXPIRED_RECEIPT.into()
                                },
                            },
                            data,
                        ));
                    }
                }
                // Let the in-flight operation finish. A crashed worker stops delaying other
                // callers once its short, authority-clock permit expires.
                if self
                    .permit
                    .as_ref()
                    .is_some_and(|p| p.revision == rev && p.issued.elapsed() < OPERATION_TTL)
                {
                    return Ok((Answer::Busy, Vec::new()));
                }
                let token = new_secret();
                self.permit = Some(Permit {
                    token: token.clone(),
                    node: is_primary,
                    op,
                    revision: rev,
                    issued: Instant::now(),
                });
                if known != Some(rev) {
                    data = tx.query_row("SELECT image FROM head", [], |r| r.get(0))?;
                }
                Answer::Ready {
                    revision: rev,
                    permit: token,
                }
            }
            Control::Commit { op, permit, result } => {
                let granted = self.permit.as_ref().context("no live permit")?;
                ensure!(
                    granted.node == is_primary
                        && granted.token == permit
                        && granted.op.remote == op.remote
                        && granted.op.id == op.id
                        && granted.op.digest == op.digest
                        && granted.op.receipt_policy == op.receipt_policy
                        && granted.issued.elapsed() < OPERATION_TTL,
                    "operation fenced"
                );
                ensure!(
                    result.len() <= MAX_FRAME,
                    "response exceeds receipt capacity"
                );
                let next = granted
                    .revision
                    .checked_add(1)
                    .context("revision exhausted")?;
                let tx = self
                    .db
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                let updated = if image.is_empty() {
                    tx.execute(
                        "UPDATE head SET revision=?1 WHERE revision=?2",
                        params![next, granted.revision],
                    )?
                } else {
                    ensure!(
                        image.starts_with(b"SQLite format 3\0"),
                        "invalid candidate image"
                    );
                    let old: Vec<u8> = tx.query_row("SELECT image FROM head", [], |r| r.get(0))?;
                    let retired = remember_guard_state(&tx, &image, false)?;
                    let before = deletion_set(&old)?;
                    let after = deletion_set(&image)?;
                    ensure!(
                        before.is_subset(&after),
                        "candidate rolls back a known deletion"
                    );
                    if before != after || retired {
                        for deletion in after.difference(&before) {
                            deletion.remember(&tx)?;
                        }
                        // A cached feed, contact lookup, export or earlier acceptance
                        // may contain the opening. Fence its operation ID against reruns.
                        tx.execute("UPDATE receipts SET response=?", [EXPIRED_RECEIPT])?;
                        tx.execute(
                            "UPDATE receipt_visibility SET revision=?",
                            [DURABLE_RECEIPT],
                        )?;
                    }
                    tx.execute(
                        "UPDATE head SET revision=?1,image=?2 WHERE revision=?3",
                        params![next, image, granted.revision],
                    )?
                };
                ensure!(updated == 1, "stale revision");
                let receipt_revision = if op.receipt_policy.durable(&result) {
                    DURABLE_RECEIPT
                } else {
                    tx.query_row("SELECT revision FROM receipt_epoch", [], |r| {
                        r.get::<_, i64>(0)
                    })?
                };
                let now = now();
                tx.execute("DELETE FROM receipts WHERE expires<?1", [now])?;
                ensure!(
                    tx.execute(
                        "INSERT INTO receipts(remote,id,digest,response,expires) VALUES(?1,?2,?3,?4,?5)
                         ON CONFLICT(remote,id) DO UPDATE SET response=excluded.response,expires=excluded.expires
                         WHERE receipts.digest=excluded.digest",
                        params![op.remote, op.id, op.digest, result, now + RECEIPT_TTL],
                    )? == 1,
                    "request ID reused"
                );
                tx.execute(
                    "INSERT OR REPLACE INTO receipt_visibility(remote,id,revision) VALUES(?1,?2,?3)",
                    params![op.remote, op.id, receipt_revision],
                )?;
                tx.commit()?;
                self.permit = None;
                Answer::Committed {
                    revision: next,
                    result,
                }
            }
        };
        Ok((answer, data))
    }
}

fn remember_guard_state(ledger: &Connection, image: &[u8], seed: bool) -> Result<bool> {
    let mut db = Connection::open_in_memory()?;
    db.deserialize_read_exact(rusqlite::MAIN_DB, image, image.len(), true)?;
    let read_retired = |db: &Connection, sql: &str| -> Result<BTreeSet<(String, String)>> {
        Ok(db
            .prepare(sql)?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    };
    let known = read_retired(
        ledger,
        "SELECT account,record_id FROM guard.known_deleted_accounts",
    )?;
    // Older authority images acquire the new main table on their first staging restore.
    let next = if db.table_exists(Some(rusqlite::MAIN_DB), c"deleted_accounts")? {
        read_retired(&db, "SELECT account,record_id FROM deleted_accounts")?
    } else {
        BTreeSet::new()
    };
    ensure!(
        known.is_subset(&next),
        "candidate rolls back a deleted account"
    );
    for (account, record) in next.difference(&known) {
        if !seed {
            let raw: String = ledger.query_row(
                "SELECT chain FROM guard.current_accounts WHERE account=?",
                [account],
                |r| r.get(0),
            )?;
            let chain: Vec<crate::portability::AccountRecord> = serde_json::from_str(&raw)?;
            let current = crate::portability::validate_chain(&chain)?;
            ensure!(
                current.payload.account == *account && current.id()? == *record,
                "HA deletion does not match current guard registration"
            );
        }
        ledger.execute(
            "INSERT INTO guard.known_deleted_accounts VALUES(?,?)",
            params![account, record],
        )?;
        ledger.execute(
            "DELETE FROM guard.current_accounts WHERE account=?",
            [account],
        )?;
        ledger.execute("DELETE FROM guard.opening_keys WHERE owner=?", [account])?;
    }
    let retired = next != known;
    let retired_ids: BTreeSet<_> = next.iter().map(|(id, _)| id.as_str()).collect();
    let mut stmt = db.prepare("SELECT account,chain FROM account_states")?;
    let deliveries =
        crate::backup::copy_deliveries(&db, ledger, "peer_deliveries", "guard.known_deliveries")?;
    let mut changed = 0;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (account, chain) = row?;
        ensure!(
            !retired_ids.contains(account.as_str()),
            "candidate resurrects a deleted account"
        );
        changed+=ledger.execute("INSERT INTO guard.current_accounts VALUES (?,?) ON CONFLICT(account) DO UPDATE SET chain=excluded.chain WHERE chain<>excluded.chain",params![account,chain])?;
    }
    if changed > 0 {
        // A retry of a read-only preflight must not certify a superseded registration.
        let expired = r#"{"type":"error","code":"expired","message":"Account changed; refresh"}"#;
        ledger.execute(
            "UPDATE receipts SET response=? WHERE json_valid(response) AND json_extract(response,'$.type')='accountDeleteAvailable'",
            [expired],
        )?;
        ledger.execute(
            "UPDATE receipt_visibility SET revision=? WHERE (remote,id) IN (SELECT remote,id FROM receipts WHERE response=?)",
            params![DURABLE_RECEIPT, expired],
        )?;
    }
    if changed > 0 || deliveries > 0 || retired {
        ledger.execute("UPDATE guard.state SET epoch=epoch+1", [])?;
        ledger.execute("UPDATE deletion_epoch SET epoch=epoch+1", [])?;
    }
    Ok(retired)
}
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct RetainedDeletion {
    target: String,
    owner: String,
    community: String,
    private: bool,
    proof: String,
}
impl RetainedDeletion {
    fn remember(&self, db: &Connection) -> Result<()> {
        crate::backup::remember(
            db,
            &self.target,
            &self.owner,
            &self.community,
            self.private,
            &self.proof,
        )
    }
}
fn deletion_set(image: &[u8]) -> Result<BTreeSet<RetainedDeletion>> {
    let mut db = Connection::open_in_memory()?;
    db.deserialize_read_exact(rusqlite::MAIN_DB, image, image.len(), true)?;
    let mut stmt = db.prepare("SELECT target,owner,community,private,proof FROM deletions")?;
    Ok(stmt
        .query_map([], |r| {
            Ok(RetainedDeletion {
                target: r.get(0)?,
                owner: r.get(1)?,
                community: r.get(2)?,
                private: r.get(3)?,
                proof: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
}

fn packet<T: Serialize>(header: &T, image: &[u8]) -> Result<Vec<u8>> {
    let h = serde_json::to_vec(header)?;
    ensure!(
        h.len() <= HEADER_LIMIT && image.len() <= snapshot_limit()?,
        "replication capacity exceeded"
    );
    let mut bytes = Vec::with_capacity(4 + h.len() + image.len());
    bytes.extend_from_slice(&(h.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&h);
    bytes.extend_from_slice(image);
    Ok(bytes)
}
fn unpack<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<(T, Vec<u8>)> {
    let (length, rest) = bytes.split_at_checked(4).context("truncated HA packet")?;
    let n = u32::from_be_bytes(length.try_into()?) as usize;
    ensure!(n <= HEADER_LIMIT, "invalid replication header");
    let (header, image) = rest.split_at_checked(n).context("truncated HA header")?;
    ensure!(
        image.len() <= snapshot_limit()?,
        "invalid replication image"
    );
    Ok((serde_json::from_slice(header)?, image.to_vec()))
}
// A cancelled request can drop its future while its blocking job is still running.
tokio::task_local! {
    pub(super) static STAGING: Arc<tokio::sync::OwnedMutexGuard<Option<u64>>>;
}

pub(super) struct Ha {
    policy: String,
    grant: FullHomeStandby,
    ledger: Option<Arc<Mutex<Ledger>>>,
    ledger_worker: Arc<Semaphore>,
    // Only one operation can touch the staging database on each physical node at once.
    cached: Arc<tokio::sync::Mutex<Option<u64>>>,
}
impl Ha {
    pub(super) fn new(
        options: &HaOptions,
        config: &ServerConfig,
        peering: Option<&Policy>,
    ) -> Result<Self> {
        // Filesystem paths, dial hints and peer ordering are machine-local. Membership tokens,
        // origin scopes and moderation trust policy must match for the two authorized hosts.
        let peers: BTreeMap<_, _> = config
            .peers
            .iter()
            .map(|p| (p.address.id.to_string(), &p.communities))
            .collect();
        let policy = blake3::keyed_hash(
            &signing_key(&config.private_key)?.to_bytes(),
            &serde_json::to_vec(&(
                crate::PROTOCOL_VERSION,
                &config.communities,
                peers,
                &config.public_replica_feeds,
                config.auto_federation.enabled,
                &config.auto_federation.blocked_hosts,
                peering,
            ))?,
        )
        .to_hex()
        .to_string();
        snapshot_limit()?;
        let ledger = match &options.role {
            HaRole::Primary => None,
            HaRole::Standby { ledger, .. } => {
                Some(Arc::new(Mutex::new(Ledger::open(ledger, &options.grant)?)))
            }
        };
        Ok(Self {
            policy,
            grant: options.grant.clone(),
            ledger,
            ledger_worker: Arc::new(Semaphore::new(1)),
            cached: Arc::default(),
        })
    }
    pub(super) fn committed_capacity(&self) -> Result<(usize, usize)> {
        let ledger = self
            .ledger
            .as_ref()
            .context("standby ledger required")?
            .lock()
            .map_err(|_| anyhow::anyhow!("ledger unavailable"))?;
        let bytes = ledger.db.query_row(
            "SELECT length(image) FROM head JOIN deletion_epoch USING(singleton)
            JOIN guard.state USING(id,epoch) WHERE revision>0",
            [],
            |row| row.get(0),
        )?;
        Ok((bytes, snapshot_limit()?))
    }

    pub(super) async fn drain(&self) {
        let _all = self.ledger_worker.acquire().await;
    }

    async fn local(&self, primary: bool, op: Control, data: Vec<u8>) -> Result<(Answer, Vec<u8>)> {
        let ledger = self.ledger.clone().context("not the commit authority")?;
        let permit = self.ledger_worker.clone().acquire_owned().await?;
        tokio::task::spawn_blocking(move || {
            let (_permit, ledger) = (permit, ledger);
            ledger
                .lock()
                .map_err(|_| anyhow::anyhow!("ledger unavailable"))?
                .exchange(primary, op, data)
        })
        .await?
    }
    pub(super) async fn control(
        &self,
        remote: &str,
        recv: &mut iroh::endpoint::RecvStream,
    ) -> Result<Vec<u8>> {
        // Authenticate the transport before accepting an oversized frame or reading private state.
        ensure!(
            self.ledger.is_some() && remote == self.grant.home,
            "not an authorized HA member"
        );
        let bytes = recv
            .read_to_end(4 + HEADER_LIMIT + snapshot_limit()?)
            .await?;
        let ((policy, op), image): ((String, Control), _) = unpack(&bytes)?;
        ensure!(policy == self.policy, "home admission policies differ");
        let (answer, image) = self
            .local(true, op, image)
            .await
            .unwrap_or((Answer::Refused, Vec::new()));
        packet(&answer, &image)
    }
    async fn exchange(
        &self,
        endpoint: &Endpoint,
        op: Control,
        image: Vec<u8>,
    ) -> Result<(Answer, Vec<u8>)> {
        if self.ledger.is_some() {
            return self.local(false, op, image).await;
        }
        tokio::time::timeout(Duration::from_secs(crate::RPC_TIMEOUT_SECS), async {
            let connection = endpoint
                .connect(self.grant.standby.clone(), crate::ALPN)
                .await?;
            let (mut send, mut recv) = connection.open_bi().await?;
            send.write_all(&[CONTROL]).await?;
            send.write_all(&packet(&(&self.policy, op), &image)?)
                .await?;
            send.finish()?;
            let bytes = recv
                .read_to_end(4 + HEADER_LIMIT + snapshot_limit()?)
                .await?;
            let result = unpack(&bytes);
            connection.close(0u32.into(), b"replication complete");
            result
        })
        .await?
    }
    pub(super) async fn seed_image(&self, endpoint: &Endpoint, image: Vec<u8>) -> Result<()> {
        ensure!(
            matches!(
                self.exchange(endpoint, Control::Seed, image).await?.0,
                Answer::Seeded
            ),
            "ledger bootstrap refused"
        );
        Ok(())
    }
    async fn run<F, Fut>(&self, state: &Arc<State>, op: Operation, work: F) -> Result<Response>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Response>>,
    {
        let mut cache = self.cached.clone().lock_owned().await;
        // Taking the known revision first makes cancellation at any later await invalidate
        // speculative local changes, including cancellation after a successful remote commit.
        let known = cache.take();
        let (answer, image) = tokio::time::timeout(Duration::from_secs(12), async {
            loop {
                let reply = self
                    .exchange(
                        &state.endpoint,
                        Control::Open {
                            op: op.clone(),
                            known,
                        },
                        Vec::new(),
                    )
                    .await?;
                if !matches!(reply.0, Answer::Busy) {
                    return anyhow::Ok(reply);
                }
                tokio::time::sleep(Duration::from_millis(90)).await;
            }
        })
        .await??;
        let (rev, permit) = match answer {
            Answer::Committed { result, .. } => {
                return transport::decode_response(result.as_bytes());
            }
            Answer::Ready { revision, permit } => (revision, permit),
            _ => bail!("home commit authority unavailable"),
        };
        let cache = Arc::new(cache);
        let (ok, serialized, image) = STAGING
            .scope(cache.clone(), async {
                let before = state
                    .ha_blocking(move |store| {
                        if !image.is_empty() {
                            store.restore_snapshot(&image)?;
                        } else {
                            ensure!(known == Some(rev), "missing committed snapshot");
                        }
                        Ok(store
                            .db
                            .sqlite()
                            .context("expected HA staging")?
                            .total_changes())
                    })
                    .await?;
                let result = work().await;
                let ok = result.is_ok();
                // Queue behind any staging work still completing after a background timeout.
                let (serialized, image) = state
                    .ha_blocking(move |store| {
                        let response = result.unwrap_or_else(|e| Response::Error {
                            code: "rejected".into(),
                            message: e.to_string(),
                        });
                        let serialized = String::from_utf8(transport::encode_response(&response)?)?;
                        ensure!(serialized.len() <= MAX_FRAME, "home response exceeds bound");
                        let image = if ok
                            && before
                                != store
                                    .db
                                    .sqlite()
                                    .context("expected HA staging")?
                                    .total_changes()
                        {
                            store.snapshot()?
                        } else {
                            Vec::new()
                        };
                        Ok((serialized, image))
                    })
                    .await?;
                anyhow::Ok((ok, serialized, image))
            })
            .await?;
        let mut cache =
            Arc::try_unwrap(cache).map_err(|_| anyhow::anyhow!("staging still busy"))?;
        let (answer, _) = self
            .exchange(
                &state.endpoint,
                Control::Commit {
                    op,
                    permit,
                    result: serialized,
                },
                image,
            )
            .await?;
        match answer {
            Answer::Committed { revision, result } => {
                if ok {
                    *cache = Some(revision);
                }
                transport::decode_response(result.as_bytes())
            }
            _ => bail!("operation fenced before commit"),
        }
    }
    pub(super) async fn request(
        &self,
        state: &Arc<State>,
        remote: &str,
        id: String,
        call: Request,
    ) -> Result<Response> {
        if matches!(call, Request::ServerSource) {
            return state.dispatch(remote, call).await;
        }
        if matches!(
            call,
            Request::ReportAvailability
                | Request::SubmitReport { .. }
                | Request::ReportStatus { .. }
        ) {
            return Ok(state.dispatch(remote, call).await.unwrap_or_else(|error| {
                Response::Error {
                    code: "rejected".into(),
                    message: error.to_string(),
                }
            }));
        }
        // Previous disclosures must never bypass a changed admission policy.
        let receipt_policy = ReceiptPolicy::for_request(&call);
        let bytes = if receipt_policy != ReceiptPolicy::Durable {
            serde_json::to_vec(&(&self.policy, &call))?
        } else {
            serde_json::to_vec(&call)?
        };
        let mut op = Operation::new(remote, id, &bytes)?;
        op.receipt_policy = receipt_policy;
        self.run(state, op, || state.dispatch(remote, call)).await
    }
    pub(super) async fn expire_mail(&self, state: &Arc<State>) -> Result<bool> {
        self.background(state, state.ha_blocking(|store| store.sweep_expired_mail()))
            .await
    }

    pub(super) async fn push_operation(
        &self,
        state: &Arc<State>,
        jobs: Option<(serde_json::Value, serde_json::Value)>,
    ) -> Result<serde_json::Value> {
        let op = Operation::new(&self.grant.home, new_secret(), b"push-dispatch")?;
        match self
            .run(state, op, || {
                state.ha_blocking(move |store| {
                    let result = match jobs {
                        Some((jobs, outcomes)) => store.finish_push(&jobs, &outcomes)?,
                        None => store.claim_push()?,
                    };
                    Ok(Response::Social { result })
                })
            })
            .await?
        {
            Response::Social { result } => Ok(result),
            _ => bail!("push state unavailable"),
        }
    }
    // Each operation gets its own permit and deadline. A failed peer cannot roll back
    // another peer's cursor, and slow metadata cannot roll back a verified page.
    async fn background<T, F>(&self, state: &Arc<State>, work: F) -> Result<T>
    where
        T: Serialize + serde::de::DeserializeOwned,
        F: std::future::Future<Output = Result<T>>,
    {
        let op = Operation::new(&self.grant.home, new_secret(), b"background-federation")?;
        let result = self
            .run(state, op, || async {
                let value = tokio::time::timeout(Duration::from_secs(3), work).await??;
                Ok(Response::Social {
                    result: serde_json::to_value(value)?,
                })
            })
            .await?;
        match result {
            Response::Social { result } => Ok(serde_json::from_value(result)?),
            Response::Error { message, .. } => bail!("{message}"),
            _ => bail!("unexpected sync receipt"),
        }
    }

    pub(super) async fn synchronize(&self, server: &Server) -> Result<usize> {
        if self.ledger.is_some() || self.cached.try_lock().is_err() {
            return Ok(0);
        }
        let mut errors = Vec::new();
        if server.state.auto.enabled
            && let Err(e) = self.background(&server.state, server.discover_once()).await
        {
            errors.push(format!("discovery: {e:#}"));
        }
        let peers = self
            .background(&server.state, server.state.blocking(|op| op.sync_peers()))
            .await?;
        let mut count = 0;
        for peer in peers {
            let result: Result<()> = async {
                for _ in 0..MAX_SYNC_PAGES {
                    let (entries, more) = self
                        .background(&server.state, server.sync_page(&peer))
                        .await?;
                    count += entries;
                    if !more {
                        self.background(&server.state, server.sync_peer_metadata(&peer))
                            .await?;
                        break;
                    }
                }
                Ok(())
            }
            .await;
            if let Err(error) = result {
                errors.push(format!("{}: {error:#}", peer.address.id));
            }
        }
        ensure!(
            errors.is_empty(),
            "some peers failed: {}",
            errors.join("; ")
        );
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant() -> FullHomeStandby {
        let standby = iroh::SecretKey::generate().public();
        FullHomeStandby::issue(
            &signing_key(&new_secret()).unwrap(),
            EndpointAddr::new(standby),
        )
        .unwrap()
    }
    fn begin(ledger: &mut Ledger, primary: bool, op: &Operation) -> (String, u64) {
        let (answer, _) = ledger
            .exchange(
                primary,
                Control::Open {
                    op: op.clone(),
                    known: None,
                },
                Vec::new(),
            )
            .unwrap();
        let Answer::Ready { revision, permit } = answer else {
            panic!("expected operation permit")
        };
        (permit, revision)
    }
    fn commit(op: &Operation, permit: &str) -> Control {
        Control::Commit {
            op: op.clone(),
            permit: permit.into(),
            result: "durable receipt".into(),
        }
    }
    #[tokio::test]
    async fn cancelled_and_timed_out_staging_writes_keep_the_fence() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let config = super::super::federation_tests::config(dir.path().join("staging"));
        let standby = iroh::SecretKey::generate();
        let grant = FullHomeStandby::issue(
            &signing_key(&config.private_key)?,
            EndpointAddr::new(standby.public()),
        )?;
        let server = Server::start_ha(
            config,
            HaOptions {
                grant,
                role: HaRole::Standby {
                    transport_key: hex::encode(standby.to_bytes()),
                    ledger: dir.path().join("ledger"),
                },
            },
        )
        .await?;
        let state = &server.state;
        let ha = state.ha.as_ref().unwrap();
        let image = Store::open(&dir.path().join("seed"))?.snapshot()?;
        ha.local(true, Control::Seed, image).await?;
        for cancel_request in [true, false] {
            let (started, running) = tokio::sync::oneshot::channel();
            let (resume, paused) = std::sync::mpsc::channel();
            let mut operation = Box::pin(ha.background(
                state,
                state.ha_blocking(move |store| {
                    started.send(()).unwrap();
                    paused.recv_timeout(Duration::from_secs(6))?;
                    store.moderate("speculative", true)
                }),
            ));
            tokio::select! {
                started = running => started?,
                result = &mut operation => panic!("unexpected completion: {result:?}"),
            }
            if cancel_request {
                drop(operation);
                assert!(ha.cached.try_lock().is_err());
                resume.send(())?;
            } else {
                // The background deadline expires while the mutation is paused.
                assert!(
                    tokio::time::timeout(Duration::from_millis(3200), &mut operation)
                        .await
                        .is_err()
                );
                assert!(ha.cached.try_lock().is_err());
                resume.send(())?;
                assert!(operation.await.is_err());
            }
            assert!(ha.cached.lock().await.is_none());
            if cancel_request {
                // Simulate expiry of the abandoned authority permit.
                ha.ledger
                    .as_ref()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .permit
                    .as_mut()
                    .unwrap()
                    .issued -= OPERATION_TTL;
            }
            let hidden = ha
                .background(state, state.ha_blocking(|store| store.hidden()))
                .await?;
            assert!(hidden.is_empty());
        }
        server.close().await;
        Ok(())
    }

    #[test]
    fn capacity_warning_precedes_the_commit_limit() {
        let health = |used| WorkerHealth {
            ready: true,
            image_bytes: Some(used),
            budget_bytes: Some(1000),
            oldest_projection_age_seconds: None,
        };
        assert!(health(799).has_headroom());
        assert!(!health(800).has_headroom());
        assert!(!health(1000).has_headroom());
    }

    #[test]
    fn publication_of_a_new_image_fences_reads_and_keeps_write_acknowledgments() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let route = grant();
        let path = dir.path().join("ledger");
        let seed = Store::open(&dir.path().join("seed"))?.snapshot()?;
        let mut ledger = Ledger::open(&path, &route)?;
        ledger.exchange(true, Control::Seed, seed)?;
        let operations = [
            ReceiptPolicy::Read,
            ReceiptPolicy::ReadAndWrite,
            ReceiptPolicy::Durable,
        ]
        .map(|receipt_policy| Operation {
            receipt_policy,
            ..Operation::new("device", new_secret(), b"call").unwrap()
        });
        for operation in &operations {
            let (permit, _) = begin(&mut ledger, true, operation);
            ledger.exchange(true, commit(operation, &permit), Vec::new())?;
        }
        let open = |op: &Operation| Control::Open {
            op: op.clone(),
            known: None,
        };
        let [read, mixed, acknowledgment] = &operations;
        assert!(matches!(
            ledger.exchange(false, open(read), Vec::new())?.0,
            Answer::Committed { result, .. } if result == "durable receipt"
        ));

        moderate_ha(&path, &route, &new_secret(), true)?;
        drop(ledger);
        let mut ledger = Ledger::open(&path, &route)?;
        let (permit, _) = begin(&mut ledger, false, read);
        ledger.exchange(false, commit(read, &permit), Vec::new())?;
        assert!(matches!(
            ledger.exchange(false, open(mixed), Vec::new())?.0,
            Answer::Committed { result, .. } if result == EXPIRED_RECEIPT
        ));
        assert!(matches!(
            ledger.exchange(false, open(acknowledgment), Vec::new())?.0,
            Answer::Committed { result, .. } if result == "durable receipt"
        ));
        Ok(())
    }

    #[test]
    fn account_retirement_fences_ha_receipts_and_snapshot_rollback() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let route = grant();
        let wallet = crate::portability::AccountWallet::create(new_secret(), route.home.clone())?;
        let account = &wallet.validate()?.payload.account;
        let record = wallet.validate()?.id()?;
        let mut seed = Store::open(&dir.path().join("seed"))?;
        seed.register_account(&wallet.chain)?;
        let old = seed.snapshot()?;
        let mut ledger = Ledger::open(&dir.path().join("ledger"), &route)?;
        ledger.exchange(true, Control::Seed, old.clone())?;
        let read = Operation::new("reader", new_secret(), b"feed")?;
        let (lease, _) = begin(&mut ledger, true, &read);
        ledger.exchange(true, commit(&read, &lease), vec![])?;
        let mut store = Store::from_sqlite(Connection::open_in_memory()?);
        store.restore_snapshot(&old)?;
        store
            .db
            .execute("DELETE FROM account_states WHERE account=?", [account])?;
        store.db.execute(
            "INSERT INTO deleted_accounts VALUES(?,?)",
            [account, &record],
        )?;
        let delete = Operation::new("device", new_secret(), b"retire")?;
        let (lease, _) = begin(&mut ledger, false, &delete);
        ledger.exchange(false, commit(&delete, &lease), store.snapshot()?)?;
        let replay = ledger
            .exchange(
                true,
                Control::Open {
                    op: read,
                    known: None,
                },
                vec![],
            )?
            .0;
        assert!(matches!(replay,Answer::Committed { result,.. } if result.contains("expired")));
        let stale = Operation::new("worker", new_secret(), b"older image")?;
        let (lease, _) = begin(&mut ledger, true, &stale);
        assert!(ledger.exchange(true, commit(&stale, &lease), old).is_err());
        let pinned: String = ledger.db.query_row(
            "SELECT record_id FROM guard.known_deleted_accounts WHERE account=?",
            [account],
            |r| r.get(0),
        )?;
        assert_eq!(pinned, record);
        Ok(())
    }

    #[test]
    fn expired_restarted_wrong_node_and_stale_tokens_cannot_commit() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let route = grant();
        let path = tmp.path().join("ledger.db");
        let mut ledger = Ledger::open(&path, &route)?;
        assert!(Ledger::open(&path, &route).is_err());
        let image = Store::open(&tmp.path().join("seed.db"))?.snapshot()?;
        ledger.exchange(true, Control::Seed, image.clone())?;
        assert!(matches!(
            ledger.exchange(true, Control::Seed, image.clone())?.0,
            Answer::Seeded
        ));
        let op = Operation::new("a", new_secret(), b"one")?;
        let (stale, rev) = begin(&mut ledger, true, &op);
        assert!(
            ledger
                .exchange(false, commit(&op, &stale), Vec::new())
                .is_err()
        );
        let other = Operation::new("b", new_secret(), b"two")?;
        assert!(matches!(
            ledger
                .exchange(
                    false,
                    Control::Open {
                        op: other.clone(),
                        known: None
                    },
                    Vec::new()
                )?
                .0,
            Answer::Busy
        ));
        // Advance only the authority's monotonic permit clock, never a client's wall clock.
        ledger.permit.as_mut().unwrap().issued -= OPERATION_TTL;
        assert!(
            ledger
                .exchange(true, commit(&op, &stale), Vec::new())
                .is_err()
        );
        let (active, revision) = begin(&mut ledger, false, &other);
        assert_eq!(revision, rev);
        assert!(
            ledger
                .exchange(true, commit(&op, &stale), Vec::new())
                .is_err()
        );
        let (answer, _) = ledger.exchange(false, commit(&other, &active), image.clone())?;
        assert!(matches!(answer,Answer::Committed{revision,..} if revision==rev+1));
        let (interrupted, _) = begin(&mut ledger, true, &op);
        let moderation = new_secret();
        moderate_ha(&path, &route, &moderation, true)?;
        assert!(
            ledger
                .exchange(true, commit(&op, &interrupted), Vec::new())
                .is_err()
        );
        let exported = tmp.path().join("export.sqlite");
        export_ha(&path, &route, &exported)?;
        assert!(export_ha(&path, &route, &exported).is_err());
        drop(ledger);
        let restored = tmp.path().join("restored.sqlite");
        crate::backup::restore(
            &path.with_added_extension("deletion-guard"),
            &exported,
            &restored,
        )?;
        assert!(Store::open(&restored)?.hidden()?.contains(&moderation));
        assert!(Ledger::open(&path, &grant()).is_err());
        let mut ledger = Ledger::open(&path, &route)?;
        assert!(
            ledger
                .exchange(true, commit(&op, &interrupted), Vec::new())
                .is_err()
        );
        let (answer, _) = ledger.exchange(
            true,
            Control::Open {
                op: other.clone(),
                known: None,
            },
            Vec::new(),
        )?;
        assert!(matches!(answer,Answer::Committed{result,..} if result=="durable receipt"));
        let reused = Operation::new(&other.remote, other.id, b"different call")?;
        assert!(
            ledger
                .exchange(
                    false,
                    Control::Open {
                        op: reused,
                        known: None
                    },
                    Vec::new()
                )
                .is_err()
        );
        Ok(())
    }
    #[test]
    fn standby_requires_the_enrolled_home_signature() -> Result<()> {
        let mut authorization = grant();
        authorization.validate(&authorization.home)?;
        assert!(authorization.validate(&grant().home).is_err());
        authorization.standby = grant().standby;
        assert!(authorization.validate(&authorization.home).is_err());
        Ok(())
    }
}

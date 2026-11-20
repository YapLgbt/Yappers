// SPDX-License-Identifier: MIT OR Apache-2.0
#![cfg(not(target_arch = "wasm32"))]

use anyhow::{Context, Result, ensure};
use ed25519_dalek::Signer;
use iroh::{
    Endpoint,
    endpoint::{Connection, SendStream},
};
use serde_json::{Value, json};
use std::{
    ffi::{CStr, CString, c_char},
    time::Duration,
};
use yappers_netkit::{
    AccountDeleteTarget, Action, JoinConfig, MAX_FRAME, NetworkMode, PROTOCOL_VERSION, Request,
    Response, SignedEvent, ffi, new_secret,
    portability::{AccountDeletionIntent, AccountWallet, digest},
    transport,
};

fn command(handle: u64, body: Value) -> Result<Value> {
    let input = CString::new(body.to_string())?;
    // SAFETY: the input buffer lives for the call. We own the returned string.
    decode(unsafe { ffi::yappers_netkit_request(handle, input.as_ptr()) })
}
fn decode(ptr: *mut c_char) -> Result<Value> {
    ensure!(!ptr.is_null());
    // SAFETY: each pointer comes from this ABI and is freed exactly once.
    let bytes = unsafe { CStr::from_ptr(ptr).to_bytes().to_owned() };
    unsafe { ffi::yappers_netkit_string_free(ptr) };
    let result: Value = serde_json::from_slice(&bytes)?;
    ensure!(result["ok"] == true, "{}", result["error"]);
    Ok(result["data"].clone())
}
struct Handle(u64);
impl Handle {
    fn open(config: &Value) -> Result<Self> {
        let input = CString::new(config.to_string())?;
        let output = decode(unsafe { ffi::yappers_netkit_open(input.as_ptr()) })?;
        Ok(Self(output["handle"].as_u64().context("missing handle")?))
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        let _ = decode(ffi::yappers_netkit_close(self.0));
    }
}

async fn read_request(c: &Connection, wallet: &AccountWallet) -> Result<(SendStream, Request)> {
    let (send, mut recv) = c.accept_bi().await?;
    let bytes = recv.read_to_end(MAX_FRAME).await?;
    for secret in [&wallet.root_secret, &wallet.device_secret] {
        ensure!(!bytes.windows(secret.len()).any(|w| w == secret.as_bytes()));
    }
    Ok((send, serde_json::from_slice(&bytes)?))
}

async fn read_step(
    c: &Connection,
    wallet: &AccountWallet,
) -> Result<(SendStream, AccountDeletionIntent, Vec<SignedEvent>)> {
    let (send, request) = read_request(c, wallet).await?;
    let Request::AccountDelete {
        token,
        community,
        chain,
        intent,
        proof,
        deletes,
    } = request
    else {
        anyhow::bail!("expected account deletion");
    };
    ensure!(token == "fixture-token-0000000000000000000000000000" && community == "join-scope");
    ensure!(chain == wallet.chain);
    let record = wallet.validate()?;
    intent.validate(record)?;
    ensure!(proof.device == intent.device);
    proof.validate(
        record,
        "delete-account-v1",
        &intent.home,
        &digest(&(&intent, &deletes))?,
    )?;
    Ok((send, intent, deletes))
}
async fn reply(mut send: SendStream, response: Response) -> Result<()> {
    send.write_all(&serde_json::to_vec(&response)?).await?;
    send.finish()?;
    Ok(())
}
async fn mock(
    home: &Endpoint,
    wallet: &AccountWallet,
    targets: &[AccountDeleteTarget],
) -> Result<()> {
    let c = home.accept().await.context("missing client")?.await?;
    for matching in [false, true] {
        let (out, request) = read_request(&c, wallet).await?;
        let Request::AccountDeleteAvailability {
            token,
            community,
            chain,
            intent,
            proof,
        } = request
        else {
            anyhow::bail!("expected read-only availability probe");
        };
        ensure!(token == "fixture-token-0000000000000000000000000000" && community == "join-scope");
        ensure!(chain == wallet.chain && proof.device == intent.device);
        let record = wallet.validate()?;
        intent.validate(record)?;
        proof.validate(
            record,
            "check-account-delete-v1",
            &intent.home,
            &digest(&intent)?,
        )?;
        reply(
            out,
            Response::AccountDeleteAvailable {
                account: if matching {
                    intent.account
                } else {
                    new_secret()
                },
            },
        )
        .await?;
    }
    let (out, intent, empty) = read_step(&c, wallet).await?;
    ensure!(empty.is_empty());
    reply(
        out,
        Response::AccountDeleted {
            account: new_secret(),
        },
    )
    .await?;
    let (out, _, _) = read_step(&c, wallet).await?;
    reply(out, Response::AccountDeletePending { targets: vec![] }).await?;
    let (out, _, discovered) = read_step(&c, wallet).await?;
    ensure!(
        discovered.is_empty(),
        "expired batch must rediscover outstanding targets"
    );
    reply(
        out,
        Response::AccountDeletePending {
            targets: targets.to_vec(),
        },
    )
    .await?;
    let (_lost, original, deletes) = read_step(&c, wallet).await?;
    ensure!(original == intent && deletes.len() == targets.len());
    for (i, (d, t)) in deletes.iter().zip(targets).enumerate() {
        d.validate()?;
        ensure!(
            d.owner_id() == intent.account
                && d.payload.home == intent.home
                && d.payload.author == intent.device
        );
        ensure!(d.payload.community == t.community && d.payload.sequence == 15 + i as u64);
        ensure!(matches!(&d.payload.action, Action::Delete {target} if target == &t.id));
    }
    // The Home durably processed that batch, but the receipt never arrived.
    c.close(0u32.into(), b"lost terminal acknowledgment");
    let reopened = home
        .accept()
        .await
        .context("missing relaunched client")?
        .await?;
    let (out, again, retry) = read_step(&reopened, wallet).await?;
    ensure!(again == intent && retry == deletes);
    reply(
        out,
        Response::AccountDeleted {
            account: intent.account,
        },
    )
    .await?;
    reopened.closed().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ffi_persists_cross_community_proofs_and_checks_terminal_receipt_after_relaunch()
-> Result<()> {
    let home = transport::bind(NetworkMode::Local, None, true).await?;
    let wallet = AccountWallet::create(new_secret(), home.id().to_string())?;
    let targets = ["community-a", "community-b"].map(|c| AccountDeleteTarget {
        community: c.into(),
        id: new_secret(),
    });
    let cfg = json!({"join":JoinConfig{protocol_version:PROTOCOL_VERSION,network_mode:NetworkMode::Local,
        server_addr:home.addr(),community:"join-scope".into(),access_token:"fixture-token-0000000000000000000000000000".into()},
        "authorSecret":wallet.device_secret,"nextSequence":5,"accountChain":wallet.chain});
    let own = wallet.clone();
    let old_target = targets[0].clone();
    let client = tokio::task::spawn_blocking(move || -> Result<()> {
        let h = Handle::open(&cfg)?;
        let prepare = json!({"type":"prepareAccountDeletion","wallet":own});
        ensure!(
            command(h.0, prepare.clone())
                .unwrap_err()
                .to_string()
                .contains("keep using your account")
        );
        let intent = command(h.0, prepare)?["intent"].clone();
        let step =
            |proofs: Value| json!({"type":"accountDeletionStep","intent":intent,"proofs":proofs});
        ensure!(
            command(h.0, step(json!([])))
                .unwrap_err()
                .to_string()
                .contains("account mismatch")
        );
        ensure!(
            command(h.0, step(json!([])))
                .unwrap_err()
                .to_string()
                .contains("page size")
        );
        let old = command(
            h.0,
            json!({"type":"prepareAccountDeletionBatch","intent":intent,
            "targets":[old_target],"nextSequence":5}),
        )?;
        let mut expired: SignedEvent = serde_json::from_value(old["proofs"][0].clone())?;
        expired.payload.created_at -= 31 * 86400;
        expired.id = blake3::hash(
            &[
                b"yap:event:v4:profiles-1\0".as_slice(),
                &serde_json::to_vec(&expired.header()?)?,
            ]
            .concat(),
        )
        .to_hex()
        .to_string();
        expired.signature = hex::encode(
            yappers_netkit::signing_key(&own.device_secret)?
                .sign(
                    &[
                        b"yap:author:v4:profiles-1\0".as_slice(),
                        expired.id.as_bytes(),
                    ]
                    .concat(),
                )
                .to_bytes(),
        );
        expired.validate()?;
        let pending = command(h.0, step(json!([expired])))?;
        ensure!(pending["deleted"] == false);
        let batch = command(
            h.0,
            json!({"type":"prepareAccountDeletionBatch","intent":intent,
            "targets":pending["targets"],"nextSequence":15}),
        )?;
        ensure!(batch["nextSequence"] == 17);
        let durable = step(batch["proofs"].clone());
        ensure!(command(h.0, durable.clone()).is_err());
        drop(h);
        let resumed = Handle::open(&cfg)?;
        let ack = command(resumed.0, durable)?;
        ensure!(ack == json!({"deleted":true,"account":intent["account"],"home":intent["home"]}));
        Ok(())
    });
    tokio::time::timeout(Duration::from_secs(15), async {
        tokio::try_join!(mock(&home, &wallet, &targets), async { client.await? })?;
        anyhow::Ok(())
    })
    .await??;
    home.close().await;
    Ok(())
}

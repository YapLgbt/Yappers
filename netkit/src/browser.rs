// SPDX-License-Identifier: MIT OR Apache-2.0
//! Browser bindings reuse the native client's signing, validation and Iroh protocol.
use crate::{
    client::{Client, ClientConfig},
    model::new_secret,
    portability::AccountWallet,
};
use wasm_bindgen::prelude::*;

fn js_error(error: impl std::fmt::Display) -> JsError {
    JsError::new(&format!("{error:#}"))
}

#[wasm_bindgen]
pub fn create_wallet(home: &str) -> Result<String, JsError> {
    let wallet = AccountWallet::create(new_secret(), home.to_owned()).map_err(js_error)?;
    serde_json::to_string(&wallet).map_err(js_error)
}

#[wasm_bindgen]
pub struct BrowserClient(Client);

#[wasm_bindgen]
impl BrowserClient {
    pub async fn open(config: String) -> Result<BrowserClient, JsError> {
        let cfg: ClientConfig = serde_json::from_str(&config).map_err(js_error)?;
        Ok(Self(Client::open(cfg).await.map_err(js_error)?))
    }

    pub async fn request(&self, input: String) -> Result<String, JsError> {
        let command = serde_json::from_str(&input).map_err(js_error)?;
        let result = self.0.command(command).await.map_err(js_error)?;
        serde_json::to_string(&result).map_err(js_error)
    }

    pub async fn close(&self) {
        self.0.close().await;
    }
}

/// Each tab signs against the sequence reserved in the browser's shared storage lock.
#[wasm_bindgen]
pub fn prepare_action(wallet: &str, community: &str, action: &str) -> Result<String, JsError> {
    let w: AccountWallet = serde_json::from_str(wallet).map_err(js_error)?;
    let record = w.validate().map_err(js_error)?;
    let next_sequence = w
        .next_sequence
        .checked_add(1)
        .filter(|seq| *seq < i64::MAX as u64)
        .ok_or_else(|| JsError::new("Sequence exhausted"))?;
    let event = crate::SignedEvent::create_for_account(
        &crate::signing_key(&w.device_secret).map_err(js_error)?,
        record.payload.home.clone(),
        community.to_owned(),
        w.next_sequence,
        serde_json::from_str(action).map_err(js_error)?,
        record.clone(),
    )
    .map_err(js_error)?;
    serde_json::to_string(&crate::client::Prepared {
        event,
        next_sequence,
    })
    .map_err(js_error)
}

/// Recovery always adds a fresh signing device to the verified public account chain.
#[wasm_bindgen]
pub fn recover_browser_wallet(
    root_secret: &str,
    chain: &str,
    home: &str,
) -> Result<String, JsError> {
    let chain = serde_json::from_str(chain).map_err(js_error)?;
    let wallet = AccountWallet::recover(root_secret.to_owned(), chain, home.to_owned(), true)
        .map_err(js_error)?;
    serde_json::to_string(&wallet).map_err(js_error)
}

#[wasm_bindgen]
pub fn browser_recovery_key(wallet: &str) -> Result<String, JsError> {
    crate::portability::recovery_key(&serde_json::from_str(wallet).map_err(js_error)?)
        .map_err(js_error)
}

#[wasm_bindgen]
pub fn read_browser_recovery_key(input: &str, home: &str) -> Result<String, JsError> {
    Ok(crate::portability::read_recovery_key(input, home)
        .map_err(js_error)?
        .to_string())
}

/// Portable archives are encrypted, decrypted and verified inside the browser worker.
#[wasm_bindgen]
pub fn portable_account(command: &str) -> Result<String, JsError> {
    let command = serde_json::from_str(command).map_err(js_error)?;
    let result = crate::portability::command(command).map_err(js_error)?;
    serde_json::to_string(&result).map_err(js_error)
}

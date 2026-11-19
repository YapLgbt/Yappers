// SPDX-License-Identifier: MIT OR Apache-2.0
//! Versioned C ABI. No Rust layout, unwinding, runtime, or async types cross the boundary.
use crate::{
    IROH_VERSION,
    client::{Client, ClientConfig, Command},
    model::new_secret,
};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::{CStr, CString, c_char},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("create netkit runtime")
});
static CLIENTS: LazyLock<Mutex<BTreeMap<u64, Arc<Client>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
static NEXT: AtomicU64 = AtomicU64::new(1);
fn output(f: impl FnOnce() -> Result<Value>) -> *mut c_char {
    let result = catch_unwind(AssertUnwindSafe(f));
    let value = match result {
        Ok(Ok(data)) => json!({"ok":true,"data":data}),
        Ok(Err(error)) => json!({"ok":false,"error":format!("{error:#}")}),
        Err(_) => json!({"ok":false,"error":"netkit internal failure"}),
    };
    CString::new(value.to_string())
        .expect("JSON has no literal NUL")
        .into_raw()
}
unsafe fn input(ptr: *const c_char) -> Result<String> {
    unsafe { input_limit(ptr, crate::MAX_FRAME) }
}
unsafe fn input_limit(ptr: *const c_char, maximum: usize) -> Result<String> {
    if ptr.is_null() {
        bail!("null input");
    }
    // SAFETY: caller provides a readable NUL-terminated UTF-8 buffer for this call.
    let value = unsafe { CStr::from_ptr(ptr) }.to_str()?;
    if value.len() > maximum {
        bail!("input too large");
    }
    Ok(value.to_owned())
}
/// Offline recovery operation; returned secrets must be persisted only in protected storage.
/// # Safety
/// `command` is a readable NUL-terminated UTF-8 string for this call's duration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yappers_netkit_portability(command: *const c_char) -> *mut c_char {
    output(|| {
        let command = unsafe {
            input_limit(
                command,
                (crate::portability::MAX_ARCHIVE_BYTES + 65536) * 2 + 65536,
            )?
        };
        crate::portability::command(serde_json::from_str(&command)?)
    })
}

fn client(handle: u64) -> Result<Arc<Client>> {
    CLIENTS
        .lock()
        .map_err(|_| anyhow::anyhow!("client registry unavailable"))?
        .get(&handle)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("invalid or closed handle"))
}
#[unsafe(no_mangle)]
pub extern "C" fn yappers_netkit_identity() -> *mut c_char {
    output(|| Ok(json!({"secret":new_secret(),"irohVersion":IROH_VERSION,"abiVersion":1})))
}
/// # Safety
/// `config` must be a valid NUL-terminated UTF-8 buffer for this call's duration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yappers_netkit_open(config: *const c_char) -> *mut c_char {
    output(|| {
        let config: ClientConfig = serde_json::from_str(&unsafe { input(config)? })?;
        let client = Arc::new(RUNTIME.block_on(Client::open(config))?);
        let handle = NEXT.fetch_add(1, Ordering::Relaxed);
        let author = client.author_id();
        let home = client.join.server_addr.id.to_string();
        let community = client.join.community.clone();
        CLIENTS
            .lock()
            .map_err(|_| anyhow::anyhow!("client registry unavailable"))?
            .insert(handle, client);
        Ok(
            json!({"handle":handle,"author":author,"home":home,"community":community,"irohVersion":IROH_VERSION}),
        )
    })
}
// A scheduled iOS refresh owns one cancellation scope. Dropping its sender wakes any
// pending RPC, and a late request cannot subscribe after the scope has expired.
static REFRESHES: LazyLock<Mutex<BTreeMap<u64, tokio::sync::watch::Sender<()>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

#[unsafe(no_mangle)]
pub extern "C" fn yappers_netkit_begin_refresh() -> u64 {
    let Ok(mut scopes) = REFRESHES.lock() else {
        return 0;
    };
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    scopes.insert(id, tokio::sync::watch::channel(()).0);
    id
}

#[unsafe(no_mangle)]
pub extern "C" fn yappers_netkit_end_refresh(id: u64) {
    if let Ok(mut scopes) = REFRESHES.lock() {
        scopes.remove(&id);
    }
}

unsafe fn parse_request(request: *const c_char) -> Result<Command> {
    let text =
        unsafe { input_limit(request, (crate::portability::MAX_ARCHIVE_BYTES + 65536) * 2)? };
    Ok(serde_json::from_str(&text)?)
}

/// # Safety
/// `request` must be a valid NUL-terminated UTF-8 buffer for this call's duration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yappers_netkit_request(
    handle: u64,
    request: *const c_char,
) -> *mut c_char {
    output(|| RUNTIME.block_on(client(handle)?.command(unsafe { parse_request(request)? })))
}

/// # Safety
/// `request` has the same lifetime and encoding requirements as yappers_netkit_request.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yappers_netkit_refresh_request(
    scope: u64,
    handle: u64,
    request: *const c_char,
) -> *mut c_char {
    output(|| {
        let mut expired = REFRESHES
            .lock()
            .map_err(|_| anyhow::anyhow!("refresh registry unavailable"))?
            .get(&scope)
            .ok_or_else(|| anyhow::anyhow!("refresh expired"))?
            .subscribe();
        let client = client(handle)?;
        let command = unsafe { parse_request(request)? };
        RUNTIME.block_on(async {
            tokio::select! {
                biased;
                _ = expired.changed() => bail!("refresh expired"),
                reply = client.command(command) => reply
            }
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn yappers_netkit_close(handle: u64) -> *mut c_char {
    output(|| {
        let client = CLIENTS
            .lock()
            .map_err(|_| anyhow::anyhow!("client registry unavailable"))?
            .remove(&handle);
        if let Some(client) = client {
            RUNTIME.block_on(client.close());
        }
        Ok(json!({"closed":true}))
    })
}
/// # Safety
/// Free exactly once, using only pointers returned by this ABI. NULL is allowed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yappers_netkit_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(unsafe { CString::from_raw(ptr) });
    }
}

/// # Safety
/// Android's VM and global Application reference must live for the entire process.
/// The Application installs these once before any endpoint is opened.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yappers_netkit_android_initialize(
    vm: *mut std::ffi::c_void,
    application: *mut std::ffi::c_void,
) {
    unsafe { iroh::dns::install_android_jni_context(vm, application) };
}

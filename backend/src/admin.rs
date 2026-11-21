// SPDX-License-Identifier: AGPL-3.0-only
use crate::{private_file::PrivateFileLock, private_json};
use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path as RequestPath, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::ErrorKind,
    net::Ipv4Addr,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{net::TcpListener, sync::Semaphore, task::spawn_blocking};
use tower_http::services::{ServeDir, ServeFile};
use yappers_netkit::{
    model::{JoinConfig, new_secret, now, signing_key},
    report::{Evidence, Subject},
    report_inbox::Inbox,
    server::ServerConfig,
    store::{HiddenEvents, ModerationEvent, ModerationKind, Store},
};

const CODE_SECONDS: i64 = 5 * 60;
const SESSION_DURATION: Duration = Duration::from_secs(30 * 60);

#[derive(Subcommand)]
pub(crate) enum Operation {
    /// Serve a browser panel on IPv4 loopback for the selected Home.
    Serve {
        #[arg(long)]
        config: PathBuf,
        #[arg(long, default_value_t = 8787)]
        port: u16,
        /// Directory containing the SvelteKit admin build.
        #[arg(long, default_value = "website/dist-admin")]
        assets: PathBuf,
        /// Existing private report inbox. Defaults to YAP_REPORT_INBOX when set.
        #[arg(long)]
        report_inbox: Option<PathBuf>,
    },
    /// Print a one-time browser login code. Issuing a code replaces any unused code.
    Code {
        #[arg(long)]
        config: PathBuf,
    },
}

pub(crate) async fn execute(operation: Operation) -> Result<()> {
    match operation {
        Operation::Serve {
            config,
            port,
            report_inbox,
            assets,
        } => {
            ensure!(
                assets.join("index.html").is_file(),
                "admin build not found in {}. Run npm run build:admin from website/ or pass --assets",
                assets.display()
            );
            let report_inbox = report_inbox.or_else(Inbox::configured_path);
            let config = config.canonicalize().context("locate Home configuration")?;
            let settings: ServerConfig = serde_json::from_slice(&fs::read(&config)?)?;
            let id = hex::encode(
                signing_key(&settings.private_key)?
                    .verifying_key()
                    .as_bytes(),
            );
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
            let address = listener.local_addr()?;
            let host = if address.port() == 80 {
                address.ip().to_string()
            } else {
                address.to_string()
            };
            let origin = format!("http://{host}");
            let state = Arc::new(Admin {
                directory: config.with_file_name(""),
                config: settings,
                id,
                report_inbox,
                host,
                origin: origin.clone(),
                session: Mutex::new(None),
                maintenance: Arc::new(Semaphore::new(1)),
            });
            println!("Home admin listening on {origin}");
            axum::serve(listener, router(state, &assets)).await?;
        }
        Operation::Code { config } => {
            let config = config.canonicalize().context("locate Home configuration")?;
            println!(
                "{}",
                issue_code(&config.with_file_name("admin-login.json"))?
            );
        }
    }
    Ok(())
}

struct Admin {
    directory: PathBuf,
    config: ServerConfig,
    id: String,
    report_inbox: Option<PathBuf>,
    host: String,
    origin: String,
    session: Mutex<Option<Session>>,
    maintenance: Arc<Semaphore>,
}

struct Session {
    digest: blake3::Hash,
    expires: Instant,
}

#[derive(Serialize, Deserialize)]
struct LoginCode {
    digest: String,
    expires: i64,
}

fn digest(value: &str) -> blake3::Hash {
    blake3::hash(value.as_bytes())
}

fn issue_code(path: &Path) -> Result<String> {
    let _lock = PrivateFileLock::acquire(&path.with_extension("lock"))?;
    let code = new_secret();
    private_json(
        path,
        &LoginCode {
            digest: digest(&code).to_hex().to_string(),
            expires: now() + CODE_SECONDS,
        },
    )?;
    Ok(code)
}

fn consume_code(path: &Path, code: &str) -> Result<bool> {
    let _lock = PrivateFileLock::acquire(&path.with_extension("lock"))?;
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let expected: LoginCode = serde_json::from_slice(&bytes)?;
    let remaining = expected.expires - now();
    if !(1..=CODE_SECONDS).contains(&remaining)
        || digest(code) != blake3::Hash::from_hex(&expected.digest)?
    {
        return Ok(false);
    }
    fs::remove_file(path)?;
    Ok(true)
}

fn router(state: Arc<Admin>, assets: &Path) -> Router {
    let protected = Router::new()
        .route("/api/logout", post(logout))
        .route("/api/home", get(overview))
        .route("/api/peers", get(peers))
        .route("/api/peers/{peer}/stop", post(stop_peer))
        .route("/api/invitations/{index}", get(invitation))
        .route("/api/moderation", get(hidden_events).post(moderate))
        .route("/api/moderation/{event}", get(moderation_event))
        .route("/api/moderation/hidden/{after}", get(hidden_events_after))
        .route("/api/backups", post(backup))
        .route("/api/reports", get(reports))
        .route("/api/reports/{case}", get(review_report))
        .route("/api/reports/{case}/hide", post(hide_reported_event))
        .route("/api/reports/{case}/close", post(close_report))
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize));

    Router::new()
        .route_service("/", ServeFile::new(assets.join("index.html")))
        .route_service(
            "/ui-notices.txt",
            ServeFile::new(assets.join("ui-notices.txt")),
        )
        .nest_service("/_app", ServeDir::new(assets.join("_app")))
        .route("/api/login", post(login))
        .merge(protected)
        .layer(DefaultBodyLimit::max(4096))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            browser_boundary,
        ))
        .with_state(state)
}

fn exact_header(headers: &HeaderMap, name: header::HeaderName, expected: &str) -> bool {
    let values = headers.get_all(name);
    let mut values = values.iter();
    values.next().is_some_and(|value| value == expected) && values.next().is_none()
}

async fn browser_boundary(
    State(state): State<Arc<Admin>>,
    request: Request,
    next: Next,
) -> Response {
    let mut response = if !exact_header(request.headers(), header::HOST, &state.host) {
        ApiError(
            StatusCode::MISDIRECTED_REQUEST,
            "Open the exact loopback address shown by the admin command.",
        )
        .into_response()
    } else if request.method() != Method::GET
        && request.method() != Method::HEAD
        && !exact_header(request.headers(), header::ORIGIN, &state.origin)
    {
        ApiError(
            StatusCode::FORBIDDEN,
            "Open the Home admin page to perform this action.",
        )
        .into_response()
    } else {
        next.run(request).await
    };
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    // SvelteKit also applies the hashed script policy in the prerendered page.
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "base-uri 'none'; frame-ancestors 'none'; form-action 'none'; object-src 'none'",
        ),
    );
    response
}

fn authorized(state: &Admin, headers: &HeaderMap) -> bool {
    let candidate = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| is_id(value));
    let active = state.session.lock().expect("admin session lock");
    active
        .as_ref()
        .zip(candidate)
        .is_some_and(|(session, token)| {
            session.expires > Instant::now() && session.digest == digest(token)
        })
}

async fn authorize(State(state): State<Arc<Admin>>, request: Request, next: Next) -> Response {
    if !authorized(&state, request.headers()) {
        return ApiError(
            StatusCode::UNAUTHORIZED,
            "Sign in with a new code to continue.",
        )
        .into_response();
    }
    next.run(request).await
}

struct ApiError(StatusCode, &'static str);
type ApiResult<T> = std::result::Result<T, ApiError>;

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

async fn work<T: Send + 'static>(
    job: impl FnOnce() -> Result<T> + Send + 'static,
    failure: &'static str,
) -> ApiResult<T> {
    spawn_blocking(job)
        .await
        .ok()
        .and_then(Result::ok)
        .ok_or(ApiError(StatusCode::CONFLICT, failure))
}

async fn maintenance<T: Send + 'static>(
    state: Arc<Admin>,
    job: impl FnOnce(&Admin) -> Result<T> + Send + 'static,
    failure: &'static str,
) -> ApiResult<T> {
    let permit = state.maintenance.clone().try_acquire_owned().map_err(|_| {
        ApiError(
            StatusCode::CONFLICT,
            "Another maintenance action is running. Try again when it finishes.",
        )
    })?;
    work(
        move || {
            let _permit = permit;
            job(&state)
        },
        failure,
    )
    .await
}

#[derive(Deserialize)]
struct Login {
    code: String,
}

async fn login(
    State(state): State<Arc<Admin>>,
    Json(input): Json<Login>,
) -> ApiResult<Json<Value>> {
    let invalid = ApiError(
        StatusCode::UNAUTHORIZED,
        "This code is invalid or expired. Generate a new code and try again.",
    );
    let code = input.code.trim();
    if !is_id(code) {
        return Err(invalid);
    }
    let path = state.directory.join("admin-login.json");
    let code = code.to_owned();
    if !work(
        move || consume_code(&path, &code),
        "Could not check the code. Generate a new code and try again.",
    )
    .await?
    {
        return Err(invalid);
    }
    let token = new_secret();
    *state.session.lock().expect("admin session lock") = Some(Session {
        digest: digest(&token),
        expires: Instant::now() + SESSION_DURATION,
    });
    Ok(Json(
        json!({ "token": token, "sessionSeconds": SESSION_DURATION.as_secs() }),
    ))
}

async fn logout(State(state): State<Arc<Admin>>, headers: HeaderMap) -> StatusCode {
    // An older request must not clear a newer session issued just before it ran.
    let mut active = state.session.lock().expect("admin session lock");
    if let (Some(session), Some(token)) = (
        active.as_ref(),
        headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer ")),
    ) && session.digest == digest(token)
    {
        *active = None;
    }
    StatusCode::NO_CONTENT
}

async fn overview(State(state): State<Arc<Admin>>) -> ApiResult<Json<Value>> {
    maintenance(
        state,
        |state| {
            let storage_online = Store::live_admin_available(&state.config.database_path)?;
            Ok(Json(json!({
                "id": state.id,
                "storageOnline": storage_online,
                "networkMode": state.config.network_mode,
                "peerCount": state.config.peers.len(),
                "communities": state.config.communities.keys().collect::<Vec<_>>(),
                "sharedFeed": state.config.auto_federation.covenant.as_ref().map(|covenant| covenant.scope()),
                "reportsEnabled": state.report_inbox.is_some(),
            })))
        },
        "Could not check Home storage. Check that the Home and panel run as the same user.",
    )
    .await
}

async fn peers(
    State(state): State<Arc<Admin>>,
) -> ApiResult<Json<Vec<yappers_netkit::server::PeerSummary>>> {
    maintenance(
        state,
        |state| Ok(Json(Store::live_peers(&state.config.database_path)?)),
        "Could not load peers. Check that the Home is running and refresh.",
    )
    .await
}

async fn stop_peer(
    State(state): State<Arc<Admin>>,
    RequestPath(peer): RequestPath<String>,
) -> ApiResult<StatusCode> {
    if !is_id(&peer) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid peer ID."));
    }
    maintenance(
        state,
        move |state| {
            Store::stop_live_peering(&state.config.database_path, &peer)?;
            Ok(StatusCode::NO_CONTENT)
        },
        "Could not stop peering. Check that the Home is running and refresh.",
    )
    .await
}

async fn invitation(
    State(state): State<Arc<Admin>>,
    RequestPath(index): RequestPath<usize>,
) -> ApiResult<Json<JoinConfig>> {
    maintenance(
        state,
        move |state| {
            ensure!(
                Store::live_admin_available(&state.config.database_path)?,
                "Home storage is offline"
            );
            let (name, access) = state
                .config
                .communities
                .iter()
                .nth(index)
                .context("unknown community")?;
            let join: JoinConfig = serde_json::from_slice(&fs::read(
                state.directory.join(format!("join-{index}.json")),
            )?)?;
            join.validate()?;
            ensure!(
                join.community == *name
                    && join.access_token == *access
                    && join.server_addr.id.to_string() == state.id,
                "invitation belongs to another configuration"
            );
            Ok(Json(join))
        },
        "The invitation is unavailable. Check that the Home is running with this configuration.",
    )
    .await
}

fn is_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

async fn hidden_events(State(state): State<Arc<Admin>>) -> ApiResult<Json<HiddenEvents>> {
    hidden_page(state, None).await
}

async fn hidden_events_after(
    State(state): State<Arc<Admin>>,
    RequestPath(after): RequestPath<String>,
) -> ApiResult<Json<HiddenEvents>> {
    if !is_id(&after) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid hidden events cursor.",
        ));
    }
    hidden_page(state, Some(after)).await
}

async fn hidden_page(state: Arc<Admin>, after: Option<String>) -> ApiResult<Json<HiddenEvents>> {
    maintenance(
        state,
        move |state| {
            Ok(Json(Store::live_hidden_events(
                &state.config.database_path,
                after,
            )?))
        },
        "Could not load hidden events. Check that the Home is running, then try again.",
    )
    .await
}

async fn moderation_event(
    State(state): State<Arc<Admin>>,
    RequestPath(id): RequestPath<String>,
) -> ApiResult<Json<ModerationEvent>> {
    if !is_id(&id) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Enter a 64-character lowercase hexadecimal event ID.",
        ));
    }
    maintenance(
        state,
        move |state| {
            Ok(Json(Store::live_moderation_event(
                &state.config.database_path,
                &id,
            )?))
        },
        "Could not look up this event. Check that the Home is running, then try again.",
    )
    .await
}

#[derive(Deserialize)]
struct Moderation {
    event: String,
    hide: bool,
}

async fn moderate(
    State(state): State<Arc<Admin>>,
    Json(input): Json<Moderation>,
) -> ApiResult<StatusCode> {
    if !is_id(&input.event) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Enter a 64-character lowercase hexadecimal event ID.",
        ));
    }
    maintenance(
        state,
        move |state| {
            Store::moderate_live(&state.config.database_path, &input.event, input.hide)?;
            Ok(StatusCode::NO_CONTENT)
        },
        "Could not update moderation. Check that the Home is running, then try again.",
    )
    .await
}

async fn backup(State(state): State<Arc<Admin>>) -> ApiResult<(StatusCode, Json<Value>)> {
    maintenance(state, |state| {
        let directory = state.directory.join("backups");
        match fs::DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => (),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
        let metadata = directory.symlink_metadata()?;
        ensure!(metadata.is_dir() && metadata.permissions().mode() & 0o077 == 0, "backup directory must be private");
        let file = format!("home-{}-{}.yap-backup", now(), &new_secret()[..16]);
        Store::backup_live(&state.config.database_path, &directory.join(&file))?;
        Ok((StatusCode::CREATED, Json(json!({"file": format!("backups/{file}")}))))
    }, "Could not save a backup. Check that the Home is running and its backups directory is private and writable.").await
}

fn report_path(state: &Admin) -> ApiResult<PathBuf> {
    state.report_inbox.clone().ok_or(ApiError(
        StatusCode::NOT_FOUND,
        "Reports are disabled for this Home.",
    ))
}

async fn reports(State(state): State<Arc<Admin>>) -> ApiResult<Json<Vec<Value>>> {
    let path = report_path(&state)?;
    work(
        move || {
            Ok(Json(
                Inbox::open_existing_for_home(&path, &state.id)?.pending()?,
            ))
        },
        "Could not open this Home’s report inbox.",
    )
    .await
}

async fn review_report(
    State(state): State<Arc<Admin>>,
    RequestPath(case): RequestPath<String>,
) -> ApiResult<Json<Value>> {
    let path = report_path(&state)?;
    if !is_id(&case) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid report case ID."));
    }
    work(
        move || {
            Ok(Json(
                Inbox::open_existing_for_home(&path, &state.id)?.inspect(&case)?,
            ))
        },
        "Could not read this report. It may have expired.",
    )
    .await
}

async fn hide_reported_event(
    State(state): State<Arc<Admin>>,
    RequestPath(case): RequestPath<String>,
) -> ApiResult<StatusCode> {
    let path = report_path(&state)?;
    if !is_id(&case) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid report case ID."));
    }
    maintenance(
        state,
        move |state| {
            let mut report = Inbox::open_existing_for_home(&path, &state.id)?.inspect(&case)?;
            let evidence: Evidence = serde_json::from_value(report["evidence"].take())?;
            let kind = match evidence.subject {
                Subject::Post => ModerationKind::Post,
                Subject::Reply => ModerationKind::Reply,
                _ => anyhow::bail!("the report does not identify a post or reply"),
            };
            let id = evidence.reference.context("the report has no event ID")?;
            ensure!(is_id(&id), "invalid report reference");
            Store::moderate_content_live(&state.config.database_path, &id, true, Some(kind))?;
            Ok(StatusCode::NO_CONTENT)
        },
        "Could not hide this event. Check that the case is open and the matching post or reply is still on this Home.",
    ).await
}

#[derive(Deserialize)]
struct ReportOutcome {
    outcome: String,
}

async fn close_report(
    State(state): State<Arc<Admin>>,
    RequestPath(case): RequestPath<String>,
    Json(input): Json<ReportOutcome>,
) -> ApiResult<StatusCode> {
    let path = report_path(&state)?;
    if !is_id(&case) || !matches!(input.outcome.as_str(), "actionTaken" | "noAction") {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Choose a valid report and outcome.",
        ));
    }
    work(
        move || {
            Inbox::open_existing_for_home(&path, &state.id)?.close(&case, &input.outcome)?;
            Ok(StatusCode::NO_CONTENT)
        },
        "Could not close this report. Refresh the inbox to check its status.",
    )
    .await
}

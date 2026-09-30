pub mod engine;
use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    extract::Request,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chorus_federation::{APPROVAL, JOB, PROPOSAL, payload};
use chorus_nostr::RelayConfig;
use chorus_protocol::*;
use chorus_storage::{now, password_matches, token};
use engine::{Engine, Invitation};
use nostr_sdk::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone)]
pub struct Api {
    pub engine: Engine,
    pub origin: String,
    pub secure_cookies: bool,
    pub session_ttl: u64,
}
#[derive(Debug)]
pub struct ApiError(anyhow::Error);
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self(e)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":self.0.to_string()})),
        )
            .into_response()
    }
}
type ApiResult<T> = std::result::Result<T, ApiError>;
fn session_token(h: &HeaderMap) -> Option<&str> {
    h.get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|s| s.strip_prefix("chorus_session="))
}
fn authed(api: &Api, h: &HeaderMap) -> Result<bool> {
    Ok(session_token(h)
        .map(|t| api.engine.db.session(t))
        .transpose()?
        .flatten()
        .is_some())
}
fn auth(api: &Api, h: &HeaderMap, write: bool) -> Result<()> {
    let t = session_token(h).context("authentication required")?;
    let csrf = api.engine.db.session(t)?.context("session expired")?;
    if write {
        ensure!(
            h.get("x-csrf-token").and_then(|v| v.to_str().ok()) == Some(&csrf),
            "invalid CSRF token"
        );
        ensure!(
            h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) == Some(api.origin.as_str()),
            "invalid request origin"
        );
    }
    Ok(())
}
fn access(api: &Api, h: &HeaderMap, state: &FederationState) -> Result<()> {
    ensure!(state.public || authed(api, h)?, "federation not found");
    Ok(())
}
fn cookie(api: &Api, t: &str, age: u64) -> String {
    format!(
        "chorus_session={t}; Path=/; HttpOnly; SameSite=Strict; Max-Age={age}{}",
        if api.secure_cookies { "; Secure" } else { "" }
    )
}
async fn headers(request: Request, next: Next) -> Response {
    let mut r = next.run(request).await;
    let h = r.headers_mut();
    h.insert("x-content-type-options", "nosniff".parse().unwrap());
    h.insert("x-frame-options", "DENY".parse().unwrap());
    h.insert("referrer-policy", "no-referrer".parse().unwrap());
    h.insert("cache-control", "no-store".parse().unwrap());
    h.insert("content-security-policy","default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'".parse().unwrap());
    r
}
pub fn router(api: Api, web_root: Option<&str>) -> Router {
    let routes = Router::new()
        .route("/healthz", get(health))
        .route("/api/v1/session", get(session))
        .route("/api/v1/login", post(login))
        .route("/api/v1/logout", post(logout))
        .route("/api/v1/federations", get(federations).post(create))
        .route("/api/v1/federations/join", post(join))
        .route("/api/v1/federations/{id}", get(detail))
        .route("/api/v1/federations/{id}/invitation", get(invitation))
        .route("/api/v1/federations/{id}/jobs", get(jobs).post(submit_job))
        .route(
            "/api/v1/federations/{id}/proposals",
            get(proposals).post(propose),
        )
        .route(
            "/api/v1/federations/{id}/proposals/{pid}/approve",
            post(approve),
        )
        .route("/api/v1/jobs/{id}", get(job_detail))
        .route("/api/v1/jobs/{id}/proof", get(proof))
        .route("/api/v1/jobs/{id}/execute", post(execute))
        .route("/api/v1/contracts/{hash}", get(contract))
        .route("/api/v1/contracts", post(upload_contract))
        .route("/api/v1/activity", get(activity))
        .route("/api/v1/settings", get(settings))
        .route("/api/v1/settings/relays", post(relays))
        .route("/api/v1/settings/password", post(change_password))
        .layer(DefaultBodyLimit::max(MAX_WASM * 3))
        .with_state(Arc::new(api));
    let routes = if let Some(root) = web_root {
        routes.fallback_service(tower_http::services::ServeDir::new(root).fallback(
            tower_http::services::ServeFile::new(format!("{root}/200.html")),
        ))
    } else {
        routes
    };
    routes.layer(middleware::from_fn(headers))
}
async fn health(State(api): State<Arc<Api>>) -> ApiResult<Json<Value>> {
    api.engine
        .db
        .connection()?
        .query_row("SELECT 1", [], |r| r.get::<_, u32>(0))
        .map_err(anyhow::Error::from)?;
    Ok(Json(
        json!({"status":"ok","protocol":"chorus/1","vm_version":VM_VERSION}),
    ))
}
async fn session(State(api): State<Arc<Api>>, h: HeaderMap) -> ApiResult<Json<Value>> {
    if let Some(t) = session_token(&h)
        && let Some(csrf) = api.engine.db.session(t)?
    {
        return Ok(Json(
            json!({"authenticated":true,"username":api.engine.db.admin()?.0,"csrf":csrf}),
        ));
    }
    Ok(Json(json!({"authenticated":false})))
}
#[derive(Deserialize)]
struct Login {
    username: String,
    password: String,
}
async fn login(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Json(v): Json<Login>,
) -> ApiResult<Response> {
    ensure_origin(&api, &h)?;
    ensure_login_shape(&v)?;
    if !api.engine.db.begin_login()? {
        return Ok((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error":"Login temporarily locked. Wait before trying again."})),
        )
            .into_response());
    }
    let (user, encoded) = api.engine.db.admin()?;
    let password = zeroize::Zeroizing::new(v.password);
    let good = tokio::task::spawn_blocking(move || password_matches(&password, &encoded))
        .await
        .map_err(anyhow::Error::from)?;
    if !good || user != v.username {
        return Ok((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"Invalid username or password"})),
        )
            .into_response());
    }
    let (t, csrf) = api.engine.db.create_session(api.session_ttl)?;
    Ok((
        [(header::SET_COOKIE, cookie(&api, &t, api.session_ttl))],
        Json(json!({"authenticated":true,"username":user,"csrf":csrf})),
    )
        .into_response())
}
fn ensure_origin(api: &Api, h: &HeaderMap) -> Result<()> {
    ensure!(
        h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) == Some(api.origin.as_str()),
        "invalid request origin"
    );
    Ok(())
}
fn ensure_login_shape(v: &Login) -> Result<()> {
    ensure!(
        v.username.len() <= 64 && v.password.len() <= 1024,
        "invalid credentials"
    );
    Ok(())
}
async fn logout(State(api): State<Arc<Api>>, h: HeaderMap) -> ApiResult<Response> {
    auth(&api, &h, true)?;
    api.engine.db.logout(session_token(&h).unwrap_or(""))?;
    Ok((
        [(header::SET_COOKIE, cookie(&api, "", 0))],
        Json(json!({"ok":true})),
    )
        .into_response())
}
fn summary(api: &Api, s: FederationState) -> Result<Value> {
    let mut v = serde_json::to_value(&s)?;
    let o = v.as_object_mut().context("state must be an object")?;
    o.insert(
        "required".into(),
        json!(s.threshold.required(s.members.len())?),
    );
    o.insert("state_hash".into(), json!(digest(&s)?));
    o.insert(
        "status".into(),
        json!(if api
            .engine
            .db
            .get::<bool>("quarantine", &s.federation_id)?
            .is_some()
        {
            "quarantined"
        } else {
            "active"
        }),
    );
    let finals: Vec<chorus_consensus::FinalizedResult> =
        api.engine.db.list("final", Some(&s.federation_id))?;
    o.insert("finalized_jobs".into(), json!(finals.len()));
    o.insert(
        "latest_job".into(),
        json!(finals.last().map(|p| &p.commitment.job_id)),
    );
    Ok(v)
}
async fn federations(State(api): State<Arc<Api>>, h: HeaderMap) -> ApiResult<Json<Value>> {
    let logged = authed(&api, &h)?;
    let mut v = vec![];
    for s in api.engine.states()? {
        if s.public || logged {
            v.push(summary(&api, s)?);
        }
    }
    Ok(Json(json!(v)))
}
async fn detail(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let epochs = api.engine.epochs(&id)?;
    let s = epochs.last().context("federation not found")?.state.clone();
    access(&api, &h, &s)?;
    let mut v = summary(&api, s.clone())?;
    v["relays"] = json!(api.engine.transport.health().await);
    v["epochs"] = json!(epochs);
    v["current_state_hash"] = json!(api.engine.execution_head(&s)?.1.map(|v| hash(&v)));
    v["contract_size"] = json!(api.engine.db.blob(&s.wasm_hash).map(|b| b.len()).ok());
    v["member_npubs"] = json!(
        s.members
            .iter()
            .map(|m| PublicKey::from_hex(m).map(|p| p.to_bech32().unwrap_or_default()))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(anyhow::Error::from)?
    );
    Ok(Json(v))
}
#[derive(Deserialize)]
struct Create {
    name: String,
    description: String,
    public: bool,
    members: Vec<String>,
    threshold: ThresholdRule,
    wasm_hex: String,
    #[serde(default)]
    initial_state_hex: Option<String>,
    #[serde(default)]
    relays: Vec<String>,
    #[serde(default)]
    communication_keys: Vec<(String, String)>,
}
async fn create(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Json(v): Json<Create>,
) -> ApiResult<Json<Value>> {
    auth(&api, &h, true)?;
    let _guard = api.engine.mutation.lock().await;
    let wasm = hex::decode(v.wasm_hex).map_err(anyhow::Error::from)?;
    let mut members = vec![];
    for m in v.members {
        members.push(PublicKey::parse(&m).map_err(anyhow::Error::from)?.to_hex());
    }
    for (member, inbox) in v.communication_keys {
        if PublicKey::parse(&member).map_err(anyhow::Error::from)?
            != PublicKey::parse(&inbox).map_err(anyhow::Error::from)?
        {
            return Err(anyhow::anyhow!(
                "new federations use member identities for private messaging"
            )
            .into());
        }
    }
    let mappings = members.iter().map(|m| (m.clone(), m.clone())).collect();
    let state = FederationState {
        protocol_version: VERSION,
        federation_id: token(),
        epoch: 0,
        name: v.name,
        description: v.description,
        public: v.public,
        members,
        communication_keys: mappings,
        threshold: v.threshold,
        wasm_hash: hash(&wasm),
        vm_version: VM_VERSION.into(),
        initial_state: v
            .initial_state_hex
            .map(hex::decode)
            .transpose()
            .map_err(anyhow::Error::from)?,
        previous_federation_state_hash: None,
        created_at: now(),
        relays: v.relays,
        frost: None,
    };
    let state = api.engine.create(state, &wasm)?;
    drop(_guard);
    api.engine.refresh_subscriptions().await?;
    Ok(Json(summary(&api, state)?))
}
#[derive(Deserialize)]
struct Join {
    invitation: Invitation,
    genesis_hash: String,
}
async fn join(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Json(v): Json<Join>,
) -> ApiResult<Json<Value>> {
    auth(&api, &h, true)?;
    let _guard = api.engine.mutation.lock().await;
    let state = api.engine.join(&v.invitation, &v.genesis_hash)?;
    drop(_guard);
    api.engine.refresh_subscriptions().await?;
    Ok(Json(summary(&api, state)?))
}
async fn invitation(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Invitation>> {
    let s = api
        .engine
        .epochs(&id)?
        .last()
        .context("unknown federation")?
        .state
        .clone();
    access(&api, &h, &s)?;
    Ok(Json(api.engine.invitation(&id)?))
}
fn job_row(api: &Api, id: &str) -> Result<Value> {
    let (_, job, state) = api.engine.job(id)?;
    let c = api.engine.consensus(id)?;
    let status = if c.conflicting_certificates || !c.equivocators.is_empty() {
        "disputed"
    } else if c.accepted.is_some() {
        "finalized"
    } else if c.groups.len() > 1 {
        "disputed"
    } else if now() > job.expires_at {
        "expired"
    } else if api
        .engine
        .db
        .setting::<bool>(&format!("execution_failed:{id}"))?
        .unwrap_or(false)
    {
        "failed"
    } else {
        "pending"
    };
    let best = c.groups.iter().map(|g| g.signers.len()).max().unwrap_or(0);
    Ok(
        json!({"id":id,"job":job,"status":status,"agreeing":best,"member_count":state.members.len(),"required":c.required,"consensus":c,"has_proof":c.accepted.is_some() && api.engine.db.get::<chorus_consensus::FinalizedResult>("final",id)?.is_some()}),
    )
}
async fn jobs(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let s = api
        .engine
        .epochs(&id)?
        .last()
        .context("unknown federation")?
        .state
        .clone();
    access(&api, &h, &s)?;
    let mut rows = vec![];
    for e in api.engine.db.list::<Event>("job", Some(&id))? {
        let j: Job = payload(&e, JOB, &id)?;
        rows.push(job_row(&api, &digest(&j)?)?);
    }
    rows.reverse();
    Ok(Json(json!(rows)))
}
#[derive(Deserialize)]
struct Command {
    command_hex: String,
}
async fn submit_job(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(v): Json<Command>,
) -> ApiResult<Json<Value>> {
    auth(&api, &h, true)?;
    let guard = api.engine.mutation.lock().await;
    let s = api.engine.active(&id)?;
    let (id, _) = api
        .engine
        .submit_job(&s, hex::decode(v.command_hex).map_err(anyhow::Error::from)?)?;
    drop(guard);
    let e = api.engine.clone();
    let jid = id.clone();
    tokio::spawn(async move {
        let _ = e.execute(&jid).await;
    });
    Ok(Json(json!({"id":id})))
}
async fn execute(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    auth(&api, &h, true)?;
    api.engine.execute(&id).await?;
    Ok(Json(job_row(&api, &id)?))
}
async fn job_detail(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let (_, _, s) = api.engine.job(&id)?;
    access(&api, &h, &s)?;
    Ok(Json(job_row(&api, &id)?))
}
async fn proof(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<engine::ProofBundle>> {
    let (_, _, s) = api.engine.job(&id)?;
    access(&api, &h, &s)?;
    Ok(Json(api.engine.proof(&id)?))
}
async fn contract(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Path(hash): Path<String>,
) -> ApiResult<Response> {
    let mut allowed = authed(&api, &h)?;
    if !allowed {
        for s in api.engine.db.list::<engine::EpochRecord>("epoch", None)? {
            if s.state.public && s.state.wasm_hash == hash {
                allowed = true;
                break;
            }
        }
    }
    if !allowed {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    let b = api.engine.db.blob(&hash)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/wasm"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=contract.wasm",
            ),
        ],
        b,
    )
        .into_response())
}
#[derive(Deserialize)]
struct Upload {
    wasm_hex: String,
}
async fn upload_contract(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Json(v): Json<Upload>,
) -> ApiResult<Json<Value>> {
    auth(&api, &h, true)?;
    let bytes = hex::decode(v.wasm_hex).map_err(anyhow::Error::from)?;
    let hash = hash(&bytes);
    api.engine.vm.validate(&bytes, &hash)?;
    api.engine.db.store_blob(&bytes)?;
    Ok(Json(
        json!({"wasm_hash":hash,"size":bytes.len(),"vm_version":VM_VERSION}),
    ))
}
async fn proposals(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let s = api
        .engine
        .epochs(&id)?
        .last()
        .context("unknown federation")?
        .state
        .clone();
    access(&api, &h, &s)?;
    let mut rows = vec![];
    let approvals: Vec<Event> = api.engine.db.list("approval", Some(&id))?;
    for e in api.engine.db.list::<Event>("proposal", Some(&id))? {
        let p: FederationProposal = payload(&e, PROPOSAL, &id)?;
        let pid = digest(&p)?;
        let a = approvals
            .iter()
            .filter(|e| {
                payload::<FederationApproval>(e, APPROVAL, &id)
                    .is_ok_and(|a| a.proposal_hash == pid)
            })
            .collect::<Vec<_>>();
        rows.push(
            json!({"id":pid,"proposal":p,"approvals":a,"accepted":api.engine.epochs(&id)?.iter().any(|e| e.state == p.next_state)}),
        );
    }
    Ok(Json(json!(rows)))
}
async fn propose(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(mut next): Json<FederationState>,
) -> ApiResult<Json<Value>> {
    auth(&api, &h, true)?;
    let _guard = api.engine.mutation.lock().await;
    for member in &mut next.members {
        *member = PublicKey::parse(member.as_str())
            .map_err(anyhow::Error::from)?
            .to_hex();
    }
    for (member, inbox) in &mut next.communication_keys {
        *member = PublicKey::parse(member.as_str())
            .map_err(anyhow::Error::from)?
            .to_hex();
        *inbox = PublicKey::parse(inbox.as_str())
            .map_err(anyhow::Error::from)?
            .to_hex();
    }
    let pid = api.engine.propose(&id, next)?;
    Ok(Json(json!({"id":pid})))
}
async fn approve(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Path((id, pid)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    auth(&api, &h, true)?;
    let _guard = api.engine.mutation.lock().await;
    api.engine.approve(&id, &pid)?;
    Ok(Json(json!({"ok":true})))
}
async fn activity(State(api): State<Arc<Api>>, h: HeaderMap) -> ApiResult<Json<Value>> {
    auth(&api, &h, false)?;
    Ok(Json(json!(api.engine.db.activities()?)))
}
async fn settings(State(api): State<Arc<Api>>, h: HeaderMap) -> ApiResult<Json<Value>> {
    auth(&api, &h, false)?;
    Ok(Json(
        json!({"username":api.engine.db.admin()?.0,"identity_npub":api.engine.member_key().to_bech32().map_err(anyhow::Error::from)?,"node_npub":api.engine.member_key().to_bech32().map_err(anyhow::Error::from)?,"member_npub":api.engine.member_key().to_bech32().map_err(anyhow::Error::from)?,"member_hex":api.engine.member_key().to_hex(),"communication_hex":api.engine.member_key().to_hex(),"key_storage":"XChaCha20-Poly1305 / Argon2id","sessions":api.engine.db.sessions()?,"session_ttl":api.session_ttl,"relays":api.engine.db.setting::<Vec<RelayConfig>>("relays")?.unwrap_or_else(chorus_nostr::default_relays),"relay_health":api.engine.transport.health().await,"vm":{"version":VM_VERSION,"max_memory":chorus_fedvm::MEMORY_LIMIT,"max_fuel":chorus_fedvm::FUEL_LIMIT,"max_output":chorus_fedvm::OUTPUT_LIMIT,"max_wasm":MAX_WASM},"frost":"library available; distributed orchestration not enabled"}),
    ))
}
async fn relays(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Json(v): Json<Vec<RelayConfig>>,
) -> ApiResult<Json<Value>> {
    auth(&api, &h, true)?;
    validate_relays(&v)?;
    api.engine.transport.configure(&v).await?;
    api.engine.db.set_setting("relays", &v)?;
    api.engine.refresh_subscriptions().await?;
    api.engine.db.activity(
        "relays_changed",
        None,
        "settings",
        "Relay configuration updated",
    )?;
    Ok(Json(json!({"ok":true})))
}
fn validate_relays(v: &[RelayConfig]) -> Result<()> {
    ensure!(v.len() <= 32, "maximum 32 relays");
    let mut seen = std::collections::BTreeSet::new();
    for r in v {
        let u = url::Url::parse(&r.url)?;
        ensure!(
            u.scheme() == "wss"
                && u.host_str().is_some()
                && u.username().is_empty()
                && u.password().is_none()
                && r.url.len() <= 512,
            "invalid relay URL"
        );
        ensure!(seen.insert(&r.url), "duplicate relay");
        ensure!(
            !r.enabled || r.read || r.write,
            "enabled relay requires a role"
        );
    }
    Ok(())
}
#[derive(Deserialize)]
struct PasswordChange {
    current_password: String,
    new_password: String,
}
async fn change_password(
    State(api): State<Arc<Api>>,
    h: HeaderMap,
    Json(v): Json<PasswordChange>,
) -> ApiResult<Response> {
    auth(&api, &h, true)?;
    let db = api.engine.db.clone();
    tokio::task::spawn_blocking(move || {
        let old = zeroize::Zeroizing::new(v.current_password);
        let new = zeroize::Zeroizing::new(v.new_password);
        db.change_password(&old, &new)
    })
    .await
    .map_err(anyhow::Error::from)??;
    Ok((
        [(header::SET_COOKIE, cookie(&api, "", 0))],
        Json(json!({"ok":true})),
    )
        .into_response())
}

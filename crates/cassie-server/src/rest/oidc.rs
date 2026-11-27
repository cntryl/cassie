//! Configured OpenID Connect login and process-local browser sessions.

mod provider;
#[cfg(test)]
mod tests;
mod tokens;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ring::digest::{digest, SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use tokio::sync::Semaphore;
use url::Url;

use crate::app::{Cassie, CassieError, ExternalRoleAuthority};

pub(super) const CALLBACK_PATH: &str = "/api/v1/auth/oidc/callback";
pub(super) const BROWSER_COOKIE: &str = "__Host-cassie_oidc";
pub(super) const CHALLENGE_TTL: u64 = 300;
const SESSION_TTL: u64 = 8 * 60 * 60;
const MAX_CHALLENGES: usize = 512;
const MAX_SESSIONS: usize = 1_024;
const SESSION_PREFIX: &str = "oidc.";
const SETTINGS: [&str; 9] = [
    "CASSIE_OIDC_ISSUER",
    "CASSIE_OIDC_CLIENT_ID",
    "CASSIE_OIDC_CLIENT_SECRET",
    "CASSIE_OIDC_API_AUDIENCE",
    "CASSIE_OIDC_REDIRECT_URI",
    "CASSIE_OIDC_ROLE",
    "CASSIE_OIDC_PERMISSIONS_CLAIM",
    "CASSIE_OIDC_AUDIENCE_PARAMETER",
    "CASSIE_OIDC_CLIENT_AUTH_METHOD",
];

#[derive(Debug)]
pub(super) struct Rejected;

struct Settings {
    issuer: String,
    discovery: Url,
    client_id: String,
    client_secret: String,
    audience: String,
    redirect: String,
    role: String,
    permissions_claim: String,
    audience_parameter: AudienceParameter,
    client_auth: Option<provider::ClientAuth>,
}

enum AudienceParameter {
    Audience,
    Resource,
    None,
}

impl AudienceParameter {
    const fn key(&self) -> Option<&'static str> {
        match self {
            Self::Audience => Some("audience"),
            Self::Resource => Some("resource"),
            Self::None => None,
        }
    }
}

impl Settings {
    fn load() -> Result<Option<Self>, CassieError> {
        let values = SETTINGS.map(std::env::var_os);
        if values.iter().all(Option::is_none) {
            return Ok(None);
        }
        let invalid = || CassieError::Execution("invalid or incomplete OIDC configuration".into());
        let [issuer, id, secret, audience, callback, role, claim, parameter, client_auth] = values
            .map(|value| {
                value
                    .map(|value| value.into_string().map_err(|_| invalid()))
                    .transpose()
            });
        let [issuer, id, secret, audience, callback, role, claim, parameter, client_auth] = [
            issuer?,
            id?,
            secret?,
            audience?,
            callback?,
            role?,
            claim?,
            parameter?,
            client_auth?,
        ];
        let (Some(issuer), Some(id), Some(secret), Some(audience), Some(callback), Some(role)) =
            (issuer, id, secret, audience, callback, role)
        else {
            return Err(invalid());
        };
        let mut settings = Self::new(
            issuer,
            id,
            secret,
            audience,
            callback,
            role,
            claim.unwrap_or_else(|| "permissions".to_owned()),
        )
        .map_err(|_| invalid())?;
        settings.audience_parameter = match parameter.as_deref().unwrap_or("audience") {
            "audience" => AudienceParameter::Audience,
            "resource" => AudienceParameter::Resource,
            "none" => AudienceParameter::None,
            _ => return Err(invalid()),
        };
        settings.client_auth = match client_auth.as_deref() {
            None => None,
            Some("client_secret_post") => Some(provider::ClientAuth::Post),
            Some("client_secret_basic") => Some(provider::ClientAuth::Basic),
            Some(_) => return Err(invalid()),
        };
        Ok(Some(settings))
    }

    fn new(
        issuer: String,
        client_id: String,
        client_secret: String,
        audience: String,
        redirect: String,
        role: String,
        permissions_claim: String,
    ) -> Result<Self, Rejected> {
        let issuer_url = provider::https_url(&issuer)?;
        let callback = provider::https_url(&redirect)?;
        if issuer_url.query().is_some()
            || callback.path() != CALLBACK_PATH
            || callback.query().is_some()
            || client_id == audience
            || [
                &client_id,
                &client_secret,
                &audience,
                &role,
                &permissions_claim,
            ]
            .into_iter()
            .any(|value| {
                value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control)
            })
        {
            return Err(Rejected);
        }
        let discovery = provider::https_url(&format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        ))?;
        Ok(Self {
            issuer,
            discovery,
            client_id,
            client_secret,
            audience,
            redirect,
            role,
            permissions_claim,
            audience_parameter: AudienceParameter::Audience,
            client_auth: None,
        })
    }
}

pub(super) struct Oidc {
    settings: Settings,
    http: reqwest::Client,
    store: Mutex<Store>,
    outbound: Semaphore,
}

pub(super) struct LoginStart {
    pub location: Url,
    pub browser: String,
}

pub(super) struct LoginComplete {
    pub location: String,
    pub token: String,
    pub max_age: u64,
}

#[derive(Clone)]
pub(super) struct ExternalSession {
    pub authority: Arc<ExternalRoleAuthority>,
    pub user: String,
    expires_at: u64,
}

impl Oidc {
    pub(super) fn from_env(cassie: &Cassie) -> Result<Option<Arc<Self>>, CassieError> {
        let Some(settings) = Settings::load()? else {
            return Ok(None);
        };
        if !cassie.authentication_enabled()
            || !cassie.rest_secure_transport(cassie.rest_tls_enabled())
        {
            return Err(CassieError::Execution(
                "OIDC requires authenticated REST with an HTTPS deployment".into(),
            ));
        }
        let http = provider::client_builder().build().map_err(|_| {
            CassieError::Execution("could not configure the OIDC HTTP client".into())
        })?;
        Ok(Some(Arc::new(Self::new(settings, http))))
    }

    fn new(settings: Settings, http: reqwest::Client) -> Self {
        Self {
            settings,
            http,
            store: Mutex::new(Store::default()),
            outbound: Semaphore::new(16),
        }
    }

    pub(super) async fn start(&self, next: &str) -> Result<LoginStart, Rejected> {
        let _permit = self.outbound.try_acquire().map_err(|_| Rejected)?;
        let discovered = provider::discover(&self.http, &self.settings).await?;
        let (state, browser, nonce, verifier) = self
            .store
            .lock()
            .map_err(|_| Rejected)?
            .begin(safe_return(next), now()?)?;
        let mut location = discovered.authorization;
        let challenge = URL_SAFE_NO_PAD.encode(digest(&SHA256, verifier.as_bytes()).as_ref());
        location.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            ("response_mode", "query"),
            ("client_id", self.settings.client_id.as_str()),
            ("redirect_uri", self.settings.redirect.as_str()),
            ("scope", "openid"),
            ("state", &state),
            ("nonce", &nonce),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ]);
        if let Some(key) = self.settings.audience_parameter.key() {
            location
                .query_pairs_mut()
                .append_pair(key, &self.settings.audience);
        }
        Ok(LoginStart { location, browser })
    }

    pub(super) async fn complete(
        &self,
        cassie: &Cassie,
        state: &str,
        browser: &str,
        code: &str,
    ) -> Result<LoginComplete, Rejected> {
        let pending = self
            .store
            .lock()
            .map_err(|_| Rejected)?
            .take(state, browser, now()?)?;
        if code.is_empty() || code.len() > 4_096 {
            return Err(Rejected);
        }
        let _permit = self.outbound.try_acquire().map_err(|_| Rejected)?;
        let discovered = provider::discover(&self.http, &self.settings).await?;
        let pair = provider::exchange(
            &self.http,
            &self.settings,
            &discovered,
            code,
            &pending.verifier,
        )
        .await?;
        let keys = provider::get_keys(&self.http, discovered.keys).await?;
        let now = now()?;
        let verified = tokens::verify(&self.settings, &keys, &pair, &pending.nonce, now)?;
        let expires_at = verified.expires_at.min(now.saturating_add(SESSION_TTL));
        let user = digest_subject(&self.settings.issuer, &verified.subject);
        let cassie = cassie.clone();
        let role = self.settings.role.clone();
        let authority = tokio::task::spawn_blocking(move || {
            cassie.capture_external_role(&role, &verified.permissions, expires_at)
        })
        .await
        .map_err(|_| Rejected)?
        .map_err(|_| Rejected)?;
        let session = ExternalSession {
            authority,
            user,
            expires_at,
        };
        let token = self
            .store
            .lock()
            .map_err(|_| Rejected)?
            .issue(session, now)?;
        Ok(LoginComplete {
            location: pending.next,
            token,
            max_age: expires_at - now,
        })
    }

    pub(super) fn session(&self, token: &str) -> Option<ExternalSession> {
        let mut store = self.store.lock().ok()?;
        store.session(token, now().ok()?)
    }

    pub(super) fn revoke(&self, token: &str) {
        if let Ok(mut store) = self.store.lock() {
            store.sessions.remove(&fingerprint(token));
        }
    }
}

pub(super) fn is_session_token(token: &str) -> bool {
    token.starts_with(SESSION_PREFIX)
}

fn now() -> Result<u64, Rejected> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| Rejected)
}

fn random() -> Result<String, Rejected> {
    let mut bytes = [0_u8; 32];
    SystemRandom::new().fill(&mut bytes).map_err(|_| Rejected)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn fingerprint(secret: &str) -> [u8; 32] {
    digest(&SHA256, secret.as_bytes())
        .as_ref()
        .try_into()
        .expect("SHA-256 has 32 bytes")
}

fn digest_subject(issuer: &str, subject: &str) -> String {
    let mut hasher = ring::digest::Context::new(&SHA256);
    hasher.update(issuer.as_bytes());
    hasher.update(&[0]);
    hasher.update(subject.as_bytes());
    format!("oidc:{}", URL_SAFE_NO_PAD.encode(hasher.finish().as_ref()))
}

fn safe_return(path: &str) -> &str {
    if path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains('\\')
        && path.len() <= 2048
        && path.bytes().all(|byte| byte.is_ascii_graphic())
    {
        path
    } else {
        "/"
    }
}

#[derive(Default)]
struct Store {
    challenges: HashMap<[u8; 32], Challenge>,
    sessions: HashMap<[u8; 32], ExternalSession>,
}

struct Challenge {
    browser: [u8; 32],
    nonce: String,
    verifier: String,
    next: String,
    expires_at: u64,
}

impl Store {
    fn begin(
        &mut self,
        next: &str,
        now: u64,
    ) -> Result<(String, String, String, String), Rejected> {
        self.challenges
            .retain(|_, challenge| challenge.expires_at > now);
        if self.challenges.len() >= MAX_CHALLENGES {
            return Err(Rejected);
        }
        let [state, browser, nonce, verifier] = [random()?, random()?, random()?, random()?];
        self.challenges.insert(
            fingerprint(&state),
            Challenge {
                browser: fingerprint(&browser),
                nonce: nonce.clone(),
                verifier: verifier.clone(),
                next: next.to_owned(),
                expires_at: now.saturating_add(CHALLENGE_TTL),
            },
        );
        Ok((state, browser, nonce, verifier))
    }

    fn take(&mut self, state: &str, browser: &str, now: u64) -> Result<Challenge, Rejected> {
        if state.len() != 43 || browser.len() != 43 {
            return Err(Rejected);
        }
        let key = fingerprint(state);
        if self.challenges.get(&key).is_none_or(|pending| {
            pending.expires_at <= now || pending.browser != fingerprint(browser)
        }) {
            return Err(Rejected);
        }
        self.challenges.remove(&key).ok_or(Rejected)
    }

    fn issue(&mut self, session: ExternalSession, now: u64) -> Result<String, Rejected> {
        self.sessions.retain(|_, session| session.expires_at > now);
        if self.sessions.len() >= MAX_SESSIONS {
            return Err(Rejected);
        }
        let token = format!("{SESSION_PREFIX}{}", random()?);
        self.sessions.insert(fingerprint(&token), session);
        Ok(token)
    }

    fn session(&mut self, token: &str, now: u64) -> Option<ExternalSession> {
        let key = fingerprint(token);
        let session = self.sessions.get(&key)?;
        if session.expires_at <= now {
            self.sessions.remove(&key);
            return None;
        }
        Some(session.clone())
    }
}

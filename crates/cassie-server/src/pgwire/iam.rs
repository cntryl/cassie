//! AWS OIDC trust and JWT verification for the pgwire token selector.

use std::collections::{HashMap, HashSet};
use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use tokio::sync::Mutex;

mod jwks;
mod token;

use jwks::IssuerKeys;
use token::Token;
pub(super) use token::{VerifiedIdentity, MAX_TOKEN_BYTES};

#[cfg(test)]
mod tests;

const MAX_TRUST_BYTES: usize = 32 * 1024;
const MAX_JWKS_BYTES: usize = 64 * 1024;
const MAX_TOKEN_LIFETIME_SECONDS: u64 = 3_600;
const KEY_LIFETIME: Duration = Duration::from_secs(300);
const REFRESH_INTERVAL: Duration = Duration::from_secs(15);
const FETCH_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
enum Algorithm {
    Rs256,
    Es384,
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("AWS IAM authentication failed")]
pub(super) struct Rejected;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustDocument {
    audience: String,
    #[serde(default = "default_token_lifetime")]
    max_token_lifetime_seconds: u64,
    accounts: Vec<AccountDocument>,
}

const fn default_token_lifetime() -> u64 {
    300
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountDocument {
    account_id: String,
    issuer: String,
    algorithms: Vec<Algorithm>,
}

#[derive(Default)]
struct KeyCache {
    keys: Option<(Instant, Arc<IssuerKeys>)>,
    refreshed_at: Option<Instant>,
}

struct AccountTrust {
    account_id: String,
    jwks: reqwest::Url,
    algorithms: HashSet<Algorithm>,
    cache: Mutex<KeyCache>,
}

struct TrustSet {
    audience: String,
    max_token_lifetime_seconds: u64,
    accounts: HashMap<String, AccountTrust>,
}

impl TrustSet {
    fn from_json(bytes: &[u8]) -> Result<Self, Rejected> {
        if bytes.len() > MAX_TRUST_BYTES {
            return Err(Rejected);
        }
        let doc: TrustDocument = serde_json::from_slice(bytes).map_err(|_| Rejected)?;
        if doc.audience.is_empty()
            || doc.audience.len() > 1_000
            || doc.audience.chars().any(char::is_control)
            || doc.accounts.is_empty()
            || doc.accounts.len() > 32
            || !(60..=MAX_TOKEN_LIFETIME_SECONDS).contains(&doc.max_token_lifetime_seconds)
        {
            return Err(Rejected);
        }
        let mut accounts = HashMap::new();
        let mut account_ids = HashSet::new();
        for account in doc.accounts {
            if !valid_account_id(&account.account_id) || account.algorithms.is_empty() {
                return Err(Rejected);
            }
            let jwks = jwks_endpoint(&account.issuer)?;
            if !account_ids.insert(account.account_id.clone())
                || accounts
                    .insert(
                        account.issuer,
                        AccountTrust {
                            account_id: account.account_id,
                            jwks,
                            algorithms: account.algorithms.into_iter().collect(),
                            cache: Mutex::default(),
                        },
                    )
                    .is_some()
            {
                return Err(Rejected);
            }
        }
        Ok(Self {
            audience: doc.audience,
            max_token_lifetime_seconds: doc.max_token_lifetime_seconds,
            accounts,
        })
    }

    fn account_for(&self, token: &Token<'_>) -> Result<&AccountTrust, Rejected> {
        self.accounts
            .get(token.issuer())
            .filter(|account| account.algorithms.contains(&token.algorithm()))
            .ok_or(Rejected)
    }
}

fn valid_account_id(id: &str) -> bool {
    id.len() == 12 && id.bytes().all(|b| b.is_ascii_digit())
}

fn jwks_endpoint(issuer: &str) -> Result<reqwest::Url, Rejected> {
    let mut url = reqwest::Url::parse(issuer).map_err(|_| Rejected)?;
    let host = url.host_str().ok_or(Rejected)?;
    let account_label = host
        .strip_suffix(".tokens.sts.global.api.aws")
        .ok_or(Rejected)?;
    if !(1..=63).contains(&account_label.len())
        || account_label.starts_with('-')
        || account_label.ends_with('-')
        || !account_label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || issuer.strip_prefix("https://") != Some(host)
    {
        return Err(Rejected);
    }
    url.set_path("/.well-known/jwks.json");
    Ok(url)
}

pub(super) struct Verifier {
    trust: TrustSet,
    client: reqwest::Client,
    #[cfg(test)]
    fixture_clock: Option<Arc<std::sync::atomic::AtomicU64>>,
}

impl Verifier {
    pub(super) fn from_path(path: &Path) -> Result<Self, Rejected> {
        let file = std::fs::File::open(path).map_err(|_| Rejected)?;
        let mut contents = Vec::new();
        file.take(u64::try_from(MAX_TRUST_BYTES + 1).map_err(|_| Rejected)?)
            .read_to_end(&mut contents)
            .map_err(|_| Rejected)?;
        let trust = TrustSet::from_json(&contents)?;
        let client = reqwest::Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(FETCH_TIMEOUT)
            .build()
            .map_err(|_| Rejected)?;
        Ok(Self {
            trust,
            client,
            #[cfg(test)]
            fixture_clock: None,
        })
    }

    pub(super) async fn verify(&self, credential: &str) -> Result<VerifiedIdentity, Rejected> {
        let token = Token::parse(credential)?;
        let account = self.trust.account_for(&token)?;
        let keys = self.keys_for(account, &token).await?;
        token.verify(&self.trust, account, &keys, self.current_seconds()?)
    }

    fn current_seconds(&self) -> Result<u64, Rejected> {
        #[cfg(test)]
        if let Some(clock) = self.fixture_clock.as_ref() {
            return Ok(clock.load(std::sync::atomic::Ordering::Relaxed));
        }
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .map_err(|_| Rejected)
    }

    async fn keys_for(
        &self,
        account: &AccountTrust,
        token: &Token<'_>,
    ) -> Result<Arc<IssuerKeys>, Rejected> {
        let mut cache = account.cache.lock().await;
        if let Some((loaded, keys)) = cache.keys.as_ref() {
            if loaded.elapsed() < KEY_LIFETIME && keys.has_key(token.kid(), token.algorithm()) {
                return Ok(Arc::clone(keys));
            }
        }
        if cache
            .refreshed_at
            .is_some_and(|start| start.elapsed() < REFRESH_INTERVAL)
        {
            return Err(Rejected);
        }
        cache.refreshed_at = Some(Instant::now());
        let mut response = self
            .client
            .get(account.jwks.clone())
            .send()
            .await
            .map_err(|_| Rejected)?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|bytes| bytes > u64::try_from(MAX_JWKS_BYTES).unwrap_or(u64::MAX))
        {
            return Err(Rejected);
        }
        let mut contents = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Rejected)? {
            if chunk.len() > MAX_JWKS_BYTES - contents.len() {
                return Err(Rejected);
            }
            contents.extend_from_slice(&chunk);
        }
        let keys = Arc::new(IssuerKeys::from_json(&contents)?);
        cache.keys = Some((Instant::now(), Arc::clone(&keys)));
        Ok(keys)
    }
}

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
    audience: Option<String>,
    #[serde(default = "default_token_lifetime")]
    max_token_lifetime_seconds: u64,
    #[serde(default)]
    accounts: Vec<AccountDocument>,
    #[serde(default)]
    azure: Vec<AzureDocument>,
    google: Option<GoogleDocument>,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AzureDocument {
    tenant_id: String,
    token_version: AzureVersion,
    audience: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GoogleDocument {
    audience: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
enum AzureVersion {
    #[serde(rename = "1.0")]
    V1,
    #[serde(rename = "2.0")]
    V2,
}

enum Provider {
    Aws {
        account_id: String,
    },
    Azure {
        tenant_id: String,
        version: AzureVersion,
    },
    Google,
}

#[derive(Default)]
struct KeyCache {
    keys: Option<(Instant, Arc<IssuerKeys>)>,
    refreshed_at: Option<Instant>,
}

struct AccountTrust {
    provider: Provider,
    audience: String,
    max_token_lifetime_seconds: u64,
    jwks_url: String,
    algorithms: HashSet<Algorithm>,
    cache: Mutex<KeyCache>,
}

struct TrustSet {
    accounts: HashMap<String, AccountTrust>,
}

impl TrustSet {
    fn from_json(bytes: &[u8]) -> Result<Self, Rejected> {
        if bytes.len() > MAX_TRUST_BYTES {
            return Err(Rejected);
        }
        Self::from_document(serde_json::from_slice(bytes).map_err(|_| Rejected)?)
    }

    fn from_env(env: &crate::config::PgwireWorkloadEnv) -> Result<Self, Rejected> {
        let mut doc = TrustDocument {
            audience: None,
            max_token_lifetime_seconds: default_token_lifetime(),
            accounts: Vec::new(),
            azure: Vec::new(),
            google: env
                .google_audience
                .clone()
                .map(|audience| GoogleDocument { audience }),
        };
        if let Some(aws) = &env.aws {
            doc.audience = Some(aws.audience.clone());
            doc.max_token_lifetime_seconds = aws.max_token_lifetime_seconds;
            let algorithms = aws
                .algorithms
                .iter()
                .map(|value| match value.as_str() {
                    "RS256" => Ok(Algorithm::Rs256),
                    "ES384" => Ok(Algorithm::Es384),
                    _ => Err(Rejected),
                })
                .collect::<Result<_, _>>()?;
            doc.accounts.push(AccountDocument {
                account_id: aws.account_id.clone(),
                issuer: aws.issuer.clone(),
                algorithms,
            });
        }
        if let Some(azure) = &env.azure {
            doc.azure.push(AzureDocument {
                tenant_id: azure.tenant_id.clone(),
                audience: azure.audience.clone(),
                token_version: match azure.token_version.as_str() {
                    "1.0" => AzureVersion::V1,
                    "2.0" => AzureVersion::V2,
                    _ => return Err(Rejected),
                },
            });
        }
        Self::from_document(doc)
    }

    fn from_document(doc: TrustDocument) -> Result<Self, Rejected> {
        let issuer_count = doc.accounts.len() + doc.azure.len() + usize::from(doc.google.is_some());
        if !(1..=32).contains(&issuer_count)
            || !(60..=MAX_TOKEN_LIFETIME_SECONDS).contains(&doc.max_token_lifetime_seconds)
            || (doc.accounts.is_empty() != doc.audience.is_none())
        {
            return Err(Rejected);
        }
        let mut accounts = HashMap::new();
        let mut account_ids = HashSet::new();
        for account in doc.accounts {
            if !valid_account_id(&account.account_id) || account.algorithms.is_empty() {
                return Err(Rejected);
            }
            let host = validate_issuer_host(&account.issuer)?;
            if !account_ids.insert(account.account_id.clone()) {
                return Err(Rejected);
            }
            insert_issuer(
                &mut accounts,
                account.issuer,
                AccountTrust {
                    provider: Provider::Aws {
                        account_id: account.account_id,
                    },
                    audience: doc.audience.clone().ok_or(Rejected)?,
                    max_token_lifetime_seconds: doc.max_token_lifetime_seconds,
                    jwks_url: format!("https://{host}/.well-known/jwks.json"),
                    algorithms: account.algorithms.into_iter().collect(),
                    cache: Mutex::default(),
                },
            )?;
        }
        for azure in doc.azure {
            if !valid_guid(&azure.tenant_id) {
                return Err(Rejected);
            }
            let azure_endpoint = format!("https://login.microsoftonline.com/{}", azure.tenant_id);
            let (issuer, jwks_url) = match azure.token_version {
                AzureVersion::V1 => (
                    format!("https://sts.windows.net/{}/", azure.tenant_id),
                    format!("{azure_endpoint}/discovery/keys"),
                ),
                AzureVersion::V2 => (
                    format!("{azure_endpoint}/v2.0"),
                    format!("{azure_endpoint}/discovery/v2.0/keys"),
                ),
            };
            insert_issuer(
                &mut accounts,
                issuer,
                AccountTrust {
                    provider: Provider::Azure {
                        tenant_id: azure.tenant_id,
                        version: azure.token_version,
                    },
                    audience: azure.audience,
                    max_token_lifetime_seconds: 86_400,
                    jwks_url,
                    algorithms: HashSet::from([Algorithm::Rs256]),
                    cache: Mutex::default(),
                },
            )?;
        }
        if let Some(google) = doc.google {
            insert_issuer(
                &mut accounts,
                "https://accounts.google.com".to_owned(),
                AccountTrust {
                    provider: Provider::Google,
                    audience: google.audience,
                    max_token_lifetime_seconds: MAX_TOKEN_LIFETIME_SECONDS,
                    jwks_url: "https://www.googleapis.com/oauth2/v3/certs".to_owned(),
                    algorithms: HashSet::from([Algorithm::Rs256]),
                    cache: Mutex::default(),
                },
            )?;
        }
        Ok(Self { accounts })
    }

    fn account_for(&self, token: &Token<'_>) -> Result<&AccountTrust, Rejected> {
        self.accounts
            .get(token.issuer())
            .filter(|account| account.algorithms.contains(&token.algorithm()))
            .ok_or(Rejected)
    }
}

fn insert_issuer(
    accounts: &mut HashMap<String, AccountTrust>,
    issuer: String,
    account: AccountTrust,
) -> Result<(), Rejected> {
    if account.audience.is_empty()
        || account.audience.len() > 1_000
        || account.audience.chars().any(char::is_control)
        || accounts.insert(issuer, account).is_some()
    {
        return Err(Rejected);
    }
    Ok(())
}

fn valid_guid(value: &str) -> bool {
    value.len() == 36 && uuid::Uuid::parse_str(value).is_ok_and(|guid| guid.to_string() == value)
}

fn valid_google_subject(value: &str) -> bool {
    (1..=32).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_account_id(id: &str) -> bool {
    id.len() == 12 && id.bytes().all(|b| b.is_ascii_digit())
}

fn validate_issuer_host(issuer: &str) -> Result<String, Rejected> {
    let url = reqwest::Url::parse(issuer).map_err(|_| Rejected)?;
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
    Ok(host.to_owned())
}

pub(super) fn configure(
    cassie: &crate::Cassie,
    config: &crate::config::CassieRuntimeConfig,
    tls_available: bool,
) -> Result<Option<Arc<Verifier>>, crate::CassieError> {
    let sources = config.pgwire_workload_trust_source_count();
    if sources > 1 {
        return Err(crate::CassieError::Execution(
            "configure only one pgwire workload trust source".into(),
        ));
    }
    let path = config
        .pgwire_workload_trust_file
        .as_deref()
        .or(config.pgwire_aws_iam_trust_file.as_deref())
        .map(Path::new);
    cassie.validate_aws_iam_pgwire_selector(sources == 1)?;
    if sources == 1 && (!tls_available || !cassie.authentication_enabled()) {
        return Err(crate::CassieError::Execution(
            "pgwire workload authentication requires TLS and a root password".into(),
        ));
    }
    let verifier = if let Some(env) = &config.pgwire_workload_env {
        let result = TrustSet::from_env(env).and_then(Verifier::from_trust);
        Some(result)
    } else {
        path.map(|path| Verifier::from_path(path, config.pgwire_workload_trust_file.is_some()))
    };
    verifier
        .transpose()
        .map(|verifier| verifier.map(Arc::new))
        .map_err(|_| {
            crate::CassieError::Execution("invalid pgwire workload trust configuration".into())
        })
}

fn client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(FETCH_TIMEOUT)
}

#[cfg(test)]
struct FixtureClock {
    start_instant: Instant,
    start_seconds: u64,
    seconds: std::sync::atomic::AtomicU64,
}

#[cfg(test)]
impl FixtureClock {
    fn new(seconds: u64) -> Self {
        Self {
            start_instant: Instant::now(),
            start_seconds: seconds,
            seconds: std::sync::atomic::AtomicU64::new(seconds),
        }
    }

    fn set(&self, seconds: u64) {
        self.seconds
            .store(seconds, std::sync::atomic::Ordering::Relaxed);
    }

    fn seconds(&self) -> u64 {
        self.seconds.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn instant(&self) -> Instant {
        self.start_instant + Duration::from_secs(self.seconds().saturating_sub(self.start_seconds))
    }
}

fn system_seconds() -> Result<u64, Rejected> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| Rejected)
}

pub(super) struct Verifier {
    trust: TrustSet,
    client: reqwest::Client,
    #[cfg(test)]
    fixture_clock: Option<Arc<FixtureClock>>,
}

impl Verifier {
    fn from_path(path: &Path, allow_federated: bool) -> Result<Self, Rejected> {
        let file = std::fs::File::open(path).map_err(|_| Rejected)?;
        let mut contents = Vec::new();
        file.take(u64::try_from(MAX_TRUST_BYTES + 1).map_err(|_| Rejected)?)
            .read_to_end(&mut contents)
            .map_err(|_| Rejected)?;
        let trust = TrustSet::from_json(&contents)?;
        if !allow_federated
            && trust
                .accounts
                .values()
                .any(|account| !matches!(account.provider, Provider::Aws { .. }))
        {
            return Err(Rejected);
        }
        Self::from_trust(trust)
    }

    fn from_trust(trust: TrustSet) -> Result<Self, Rejected> {
        let client = client_builder().build().map_err(|_| Rejected)?;
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
        #[cfg(not(test))]
        let current_seconds = system_seconds;
        #[cfg(test)]
        let current_seconds = || {
            self.fixture_clock
                .as_ref()
                .map_or_else(system_seconds, |clock| Ok(clock.seconds()))
        };
        token.verify(account, &keys, current_seconds()?)
    }

    async fn keys_for(
        &self,
        account: &AccountTrust,
        token: &Token<'_>,
    ) -> Result<Arc<IssuerKeys>, Rejected> {
        let mut cache = account.cache.lock().await;
        #[cfg(not(test))]
        let current_instant = Instant::now;
        #[cfg(test)]
        let current_instant = || {
            self.fixture_clock
                .as_ref()
                .map_or_else(Instant::now, |clock| clock.instant())
        };
        let now = current_instant();
        if let Some((loaded, keys)) = cache.keys.as_ref() {
            if now.saturating_duration_since(*loaded) < KEY_LIFETIME
                && keys.has_key(token.kid(), token.algorithm())
            {
                return Ok(Arc::clone(keys));
            }
        }
        if cache
            .refreshed_at
            .is_some_and(|start| now.saturating_duration_since(start) < REFRESH_INTERVAL)
        {
            return Err(Rejected);
        }
        cache.refreshed_at = Some(now);
        let keys = self.fetch_issuer_keys(&account.jwks_url).await?;
        cache.keys = Some((current_instant(), Arc::clone(&keys)));
        Ok(keys)
    }

    async fn fetch_issuer_keys(&self, jwks_url: &str) -> Result<Arc<IssuerKeys>, Rejected> {
        let mut response = self
            .client
            .get(jwks_url)
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
        IssuerKeys::from_json(&contents).map(Arc::new)
    }
}

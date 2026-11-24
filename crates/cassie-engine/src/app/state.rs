use super::auth::LazyPasswordHash;
use super::auth_rate_limit::AuthRateLimiter;
use super::vector_search::VectorResultCache;
use super::{
    Arc, AtomicBool, BTreeMap, Catalog, Midge, Mutex, NormalizedVectorCacheEntry,
    NormalizedVectorCacheKey, QueryEmbeddingCacheKey, RuntimeState, Serialize,
};
use crate::embeddings::EmbeddingProvider;

#[derive(Debug, Clone, Serialize)]
pub struct CassieRuntimeConfigState {
    pub pgwire_listen: String,
    pub rest_listen: String,
}

/// Passive REST TLS deployment settings for the HTTP adapter.
#[derive(Debug, Clone, Copy)]
#[doc(hidden)]
pub struct RestTlsOptions<'a> {
    pub certificate_path: Option<&'a str>,
    pub private_key_path: Option<&'a str>,
    pub external_https: bool,
    pub allow_insecure_non_loopback: bool,
}

#[derive(Clone)]
pub struct Cassie {
    pub midge: Arc<Midge>,
    pub catalog: Catalog,
    /// Current publication authority survives temporary query catalog scopes.
    pub(crate) publication_catalog: Catalog,
    /// Runtime side effects target the existing mutable transaction stage or Main.
    pub(crate) metadata_write_catalog: Catalog,
    /// Every alias of a controlled descriptor catalog retains its admitted owner.
    pub(crate) catalog_owner: Option<Arc<crate::catalog::ControlledCatalogSnapshot>>,
    /// Read recursion lets post-commit maintenance re-enter a statement's
    /// retained publication snapshot while a writer is waiting.
    pub(crate) catalog_publication: Arc<parking_lot::RecursiveRwLock<()>>,
    /// Lets a writer waiting for `catalog_publication` hold back new statements.
    pub(crate) publication_intent: Arc<super::publication_intent::PublicationIntent>,
    pub(crate) transaction_unique_waits: Arc<super::TransactionUniqueWaits>,
    pub embedding_provider: Arc<dyn EmbeddingProvider>,
    pub(crate) runtime: Arc<RuntimeState>,
    pub(super) feedback_persistence_lock: Arc<Mutex<()>>,
    pub(super) feedback_reconciliation_required: Arc<AtomicBool>,
    pub(super) normalized_vector_cache:
        Arc<Mutex<BTreeMap<NormalizedVectorCacheKey, Arc<NormalizedVectorCacheEntry>>>>,
    pub(super) query_embedding_cache: Arc<Mutex<BTreeMap<QueryEmbeddingCacheKey, Arc<Vec<f32>>>>>,
    pub(super) vector_search_result_cache: Arc<Mutex<VectorResultCache>>,
    pub(crate) auth_user: String,
    pub(crate) auth_password: String,
    pub(crate) bootstrap_password_hash: LazyPasswordHash,
    pub(crate) dummy_password_hash: LazyPasswordHash,
    pub(crate) auth_rate_limiter: Arc<AuthRateLimiter>,
    pub(crate) default_database: String,
    pub(crate) rest_tls_cert_file: Option<String>,
    pub(crate) rest_tls_key_file: Option<String>,
    pub(crate) rest_external_https: bool,
    pub(crate) allow_insecure_non_loopback_listen: bool,
    pub started: Arc<AtomicBool>,
}

impl Cassie {
    /// Returns the configured default database name.
    #[must_use]
    #[doc(hidden)]
    pub fn default_database(&self) -> &str {
        &self.default_database
    }

    /// Returns the configured unauthenticated network principal name.
    #[must_use]
    #[doc(hidden)]
    pub fn default_network_user(&self) -> &str {
        &self.auth_user
    }

    /// Returns the runtime service used by the network adapters.
    #[must_use]
    #[doc(hidden)]
    pub fn runtime(&self) -> &Arc<RuntimeState> {
        &self.runtime
    }

    /// Reports whether the REST listener has a configured TLS identity.
    #[must_use]
    pub fn rest_tls_enabled(&self) -> bool {
        self.rest_tls_cert_file.is_some()
    }

    /// Reports whether direct TLS or the explicit external-HTTPS deployment
    /// contract requires secure browser response attributes.
    #[must_use]
    pub fn rest_secure_transport(&self, direct_tls: bool) -> bool {
        direct_tls || self.rest_external_https
    }

    /// Returns passive REST TLS settings without loading key material.
    #[must_use]
    #[doc(hidden)]
    pub fn rest_tls_options(&self) -> RestTlsOptions<'_> {
        RestTlsOptions {
            certificate_path: self.rest_tls_cert_file.as_deref(),
            private_key_path: self.rest_tls_key_file.as_deref(),
            external_https: self.rest_external_https,
            allow_insecure_non_loopback: self.allow_insecure_non_loopback_listen,
        }
    }
}

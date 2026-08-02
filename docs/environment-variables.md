# Environment Variables

This is the canonical reference for environment variables read by Cassie. Use
the tables here instead of inferring settings from Compose files, tests, or
source code.

## Reading the Tables

- **Unset** means Cassie does not supply a value.
- Boolean settings accept `1`, `true`, `yes`, or `on` for true and `0`, `false`,
  `no`, or `off` for false, case-insensitively.
- Byte values are base-10 integer byte counts.
- Provider-specific embedding settings are read only when their provider is
  selected with `CASSIE_EMBEDDINGS_PROVIDER`.
- Never put passwords or provider API keys directly in committed Compose files.

## Listeners, TLS, and Browser Security

| Variable | Allowed value | Default | Purpose |
| --- | --- | --- | --- |
| `CASSIE_PGWIRE_LISTEN` | Socket address | `127.0.0.1:5432` | PostgreSQL wire listener. |
| `CASSIE_REST_LISTEN` | Socket address | `127.0.0.1:8080` | Administrative REST and Admin UI listener. |
| `CASSIE_PGWIRE_TLS_CERT_FILE` | PEM certificate-chain path | Unset | Pgwire TLS certificate chain. Must be paired with `CASSIE_PGWIRE_TLS_KEY_FILE`. |
| `CASSIE_PGWIRE_TLS_KEY_FILE` | PEM private-key path | Unset | Pgwire TLS private key. Must be paired with `CASSIE_PGWIRE_TLS_CERT_FILE`. |
| `CASSIE_REST_TLS_CERT_FILE` | PEM certificate-chain path | Unset | REST TLS certificate chain. Must be paired with `CASSIE_REST_TLS_KEY_FILE`. |
| `CASSIE_REST_TLS_KEY_FILE` | PEM private-key path | Unset | REST TLS private key. Must be paired with `CASSIE_REST_TLS_CERT_FILE`. |
| `CASSIE_REST_EXTERNAL_HTTPS` | Boolean | `false` | Declares that a trusted external edge provides HTTPS. Enables HSTS and `Secure` login/logout cookies; forwarding headers are not trusted. |
| `CASSIE_ALLOW_INSECURE_NON_LOOPBACK_LISTEN` | Boolean | `false` | Allows a plaintext non-loopback listener only for a trusted private hop behind a TLS terminator. |
| `CASSIE_ADMIN_UI_DIR` | Directory path | `./ui/dist` | Directory containing the built Admin UI assets. |

Listener values must be literal socket addresses such as `127.0.0.1:5432`,
`0.0.0.0:8080`, or `[::1]:5432`. Hostnames are not supported, and malformed
listener values fail startup configuration preflight even when insecure
non-loopback listeners are explicitly allowed.

Non-loopback listeners fail closed unless their transport is protected by
Cassie TLS or `CASSIE_ALLOW_INSECURE_NON_LOOPBACK_LISTEN=true` explicitly
declares a trusted private hop. The default `postgres` bootstrap password is
also rejected on non-loopback listeners.

## Bootstrap Identity and Authentication Limits

| Variable | Allowed value | Default | Purpose |
| --- | --- | --- | --- |
| Root login username | `root` | `root` | Fixed administrative identity; it is not configurable. |
| `CASSIE_DEFAULT_DATABASE` | Non-empty database name | `postgres` | Database selected during bootstrap and by default connections. |
| `CASSIE_ROOT_PASSWORD` | Non-empty secret | `postgres` | Password accepted only for the fixed `root` administrative identity. Set this from a secret in deployed environments; an explicitly empty value fails startup. |
| `CASSIE_AUTH_USER_ATTEMPTS_PER_MINUTE` | Integer, minimum `1` | `10` | Process-local login token-bucket capacity per normalized user. |
| `CASSIE_AUTH_IP_ATTEMPTS_PER_MINUTE` | Integer, minimum `1` | `60` | Process-local login token-bucket capacity per peer IP. |
| `CASSIE_AUTH_RATE_LIMIT_MAX_ENTRIES` | Integer, minimum `1` | `4096` | Maximum tracked user/IP rate-limit entries before bounded overflow buckets are used. |

## Storage

Cassie and Fitz use the same storage-mode and provider vocabulary. Provider
names belong in `CASSIE_STORAGE_PROVIDER`, never in `CASSIE_STORAGE_MODE`.

| Variable | Allowed value | Default | Purpose |
| --- | --- | --- | --- |
| `CASSIE_STORAGE_MODE` | `memory`, `local`, or `cloud` | `local` | Selects ephemeral memory, persistent local disk, or cloud-backed storage. |
| `CASSIE_STORAGE_PATH` | Directory path | `./.cassie` | Persistent data directory when `CASSIE_STORAGE_MODE=local`. |
| `CASSIE_STORAGE_PROVIDER` | Provider identifier listed below | Unset | Required when `CASSIE_STORAGE_MODE=cloud`. |
| `CASSIE_STORAGE_CLOUD_DURABILITY` | `background` or `strict` | `background` | Cloud commit durability. `background` acknowledges after the local barrier and uploads asynchronously; `strict` waits for cloud-provider acknowledgement. Used only when `CASSIE_STORAGE_MODE=cloud`. |
| `CASSIE_STORAGE_PREFIX` | Object-key prefix | Empty | Optional namespace prefix for cloud objects. |
| `CASSIE_STORAGE_CACHE_PATH` | Directory path | `./.cassie-cloud-cache` | Local cache for cloud-backed storage. |
| `CASSIE_STORAGE_BUCKET` | Bucket name | Unset | Required by bucket-shaped providers unless an emulator default applies. |
| `CASSIE_STORAGE_CONTAINER` | Container name | Unset | Required by Azure Blob unless an emulator default applies. |
| `CASSIE_STORAGE_ENDPOINT` | URL | Unset | Custom or emulator provider endpoint. |
| `CASSIE_STORAGE_REGION` | Region name | Unset | Provider region. AWS also accepts `AWS_REGION` or `AWS_DEFAULT_REGION`. |
| `CASSIE_STORAGE_NAMESPACE` | Namespace name | Unset | Required by `oci-s3`. |
| `CASSIE_STORAGE_FORCE_PATH_STYLE` | Boolean | Provider-specific | Controls path-style addressing for S3-compatible providers. Defaults to `true` for `s3-compatible` and `false` for `oci-s3`. |

### Valid Storage Providers

These are the only valid values for both `CASSIE_STORAGE_PROVIDER` and
`FITZ_STORAGE_PROVIDER`.

| Provider | Required Cassie settings | Credential source and notes |
| --- | --- | --- |
| `aws-s3` | `CASSIE_STORAGE_BUCKET`; `CASSIE_STORAGE_REGION`, `AWS_REGION`, or `AWS_DEFAULT_REGION` | AWS SDK credential chain. |
| `s3-compatible` | `CASSIE_STORAGE_BUCKET`, `CASSIE_STORAGE_ENDPOINT` | Region defaults to `us-east-1`; path style defaults to true; S3 environment credentials. |
| `minio` | `CASSIE_STORAGE_BUCKET`, `CASSIE_STORAGE_ENDPOINT` | S3-compatible environment credentials. |
| `wasabi` | `CASSIE_STORAGE_BUCKET`, `CASSIE_STORAGE_REGION` | Endpoint defaults to `https://s3.<region>.wasabisys.com`; environment credentials. |
| `oci-s3` | `CASSIE_STORAGE_BUCKET`, `CASSIE_STORAGE_NAMESPACE`, `CASSIE_STORAGE_REGION` | OCI endpoint is derived unless overridden; environment credentials. |
| `azure-blob` | `CASSIE_STORAGE_CONTAINER` | Uses `AZURE_STORAGE_CONNECTION_STRING`, or `AZURE_STORAGE_ACCOUNT_NAME` with account key, SAS token, or default credentials. |
| `gcs` | `CASSIE_STORAGE_BUCKET` | Uses GCS HMAC variables, `GOOGLE_APPLICATION_CREDENTIALS`, or default credentials. |
| `sqrzl-s3` | None | Local emulator S3 front door; bucket defaults to `cassie`, endpoint to `http://127.0.0.1:9000`. |
| `sqrzl-azure` | None | Local emulator Azure front door with the same emulator defaults. |
| `sqrzl-gcs` | None | Local emulator GCS front door with the same emulator defaults. |

Any other provider identifier fails startup. The `sqrzl-*` providers are
development emulators, not production provider choices.

All Cassie-owned bootstrap, schema, document, index, session, cache, recovery,
and maintenance commits use the selected cloud durability. Local and memory
storage retain their existing synchronous or buffered policies; cloud storage
never receives Midge's local-only `sync` or `buffered` policies.

### Cloud Credential Variables

| Variable | Used by | Purpose |
| --- | --- | --- |
| `AWS_REGION`, `AWS_DEFAULT_REGION` | `aws-s3` | Region fallback when `CASSIE_STORAGE_REGION` is unset. |
| Provider-native AWS/S3 credential variables | S3-family providers | Credential chain consumed by the storage provider. |
| `AZURE_STORAGE_CONNECTION_STRING` | `azure-blob` | Complete Azure connection configuration. |
| `AZURE_STORAGE_ACCOUNT_NAME` | `azure-blob` | Account name when no connection string is supplied. |
| `AZURE_STORAGE_ACCOUNT_KEY` | `azure-blob` | Shared-key credential. |
| `AZURE_STORAGE_SAS_TOKEN` | `azure-blob` | SAS credential alternative. |
| `GCS_HMAC_ACCESS_ID`, `GCS_HMAC_SECRET` | `gcs` | HMAC credential pair; both must be set together. |
| `GOOGLE_APPLICATION_CREDENTIALS` | `gcs` | Service-account file used when HMAC credentials are absent. |
| `GOOGLE_CLOUD_PROJECT` | `gcs` | Optional project identifier override. |

## Query and Resource Limits

| Variable | Allowed value | Default | Purpose |
| --- | --- | --- | --- |
| `CASSIE_QUERY_TIMEOUT_MS` | Unsigned milliseconds; `0` disables the deadline | `30000` | Per-query execution deadline. |
| `CASSIE_MAX_RESULT_ROWS` | Unsigned integer | `100000` | Maximum rows returned by a query. |
| `CASSIE_CTE_RECURSION_DEPTH` | Unsigned integer | `64` | Recursive CTE depth limit. |
| `CASSIE_QUERY_MEMORY_BUDGET_BYTES` | Unsigned bytes | `10485760` | Accounted memory budget for one query. |
| `CASSIE_MAX_QUERY_WORKERS` | Integer, minimum `1` | `64` | Global concurrent query-worker budget. |
| `CASSIE_PARALLEL_SCAN_WORKERS` | Unsigned integer | `1` | Scan worker setting used by eligible plans. |
| `CASSIE_PARALLEL_SCORING_WORKERS` | Unsigned integer | `1` | Vector/full-text scoring worker setting used by eligible plans. |
| `CASSIE_PARALLEL_AGGREGATION_WORKERS` | Unsigned integer | `1` | Aggregation worker setting used by eligible plans. |
| `CASSIE_PGWIRE_MAX_CONNECTIONS` | Integer, minimum `1` | `256` | Pgwire connection limit. |
| `CASSIE_REST_MAX_CONNECTIONS` | Integer, minimum `1` | `512` | REST connection limit. |
| `CASSIE_REST_MAX_SESSIONS_PER_USER` | Integer, minimum `1` | `16` | Active REST sessions allowed per normalized user. |
| `CASSIE_REST_WRITE_TIMEOUT_MS` | Milliseconds, minimum `1` | `10000` | REST response-write idle timeout. |
| `CASSIE_PGWIRE_WRITE_TIMEOUT_MS` | Milliseconds, minimum `1` | `10000` | Pgwire response-write idle timeout. |

Invalid numeric and Boolean values currently fall back to the documented
default. Minimum-bounded settings clamp smaller values to their minimum.

## Caches, Feedback, and Adaptive Execution

| Variable | Allowed value | Default | Purpose |
| --- | --- | --- | --- |
| `CASSIE_EXECUTION_RESULT_CACHE_ENABLED` | Boolean | `true` | Enables the execution-result cache. |
| `CASSIE_EXECUTION_RESULT_CACHE_MAX_ENTRIES` | Unsigned integer | `64` | Maximum result-cache entries. |
| `CASSIE_EXECUTION_RESULT_CACHE_MAX_BYTES` | Unsigned bytes | `67108864` | Maximum accounted result-cache bytes. |
| `CASSIE_PLAN_CACHE_ENTRIES` | Unsigned integer | `128` | Maximum cached plans. |
| `CASSIE_CF2_PLAN_TTL_SECONDS` | Unsigned seconds | `900` | Stable CF2 plan-cache lifetime. |
| `CASSIE_CF2_PLAN_CANDIDATE_TTL_SECONDS` | Unsigned seconds | `300` | Candidate CF2 plan-cache lifetime. |
| `CASSIE_CF2_FULLTEXT_STATS_TTL_SECONDS` | Unsigned seconds | `300` | Cached full-text statistics lifetime. |
| `CASSIE_FEEDBACK_ENTRIES` | Unsigned integer | `128` | Maximum retained operator-feedback entries. |
| `CASSIE_FEEDBACK_TTL_SECONDS` | Unsigned seconds | `900` | Operator-feedback entry lifetime. |
| `CASSIE_OPERATOR_FEEDBACK_ENABLED` | Boolean | `false` | Enables feedback-informed planning. |
| `CASSIE_VECTORIZED_JOINS_ENABLED` | Boolean | `false` | Enables eligible vectorized join execution. |
| `CASSIE_VECTORIZED_JOIN_BATCH_SIZE` | Unsigned integer | `1024` | Vectorized join batch size. |
| `CASSIE_ADAPTIVE_EXECUTION_ENABLED` | Boolean | `false` | Enables adaptive execution decisions. |
| `CASSIE_ADAPTIVE_MIN_COST_SAVINGS_BPS` | Unsigned basis points | `500` | Minimum estimated savings required for an adaptive candidate. |
| `CASSIE_ADAPTIVE_MIN_CONFIDENCE_BPS` | Unsigned `u16` basis points | `0` | Minimum confidence required for an adaptive candidate. |
| `CASSIE_OPERATOR_SWITCHING_ENABLED` | Boolean | `false` | Enables runtime operator switching. |
| `CASSIE_OPERATOR_SWITCH_JOIN_ROW_THRESHOLD` | Unsigned rows | `4096` | Join-row threshold considered by operator switching. |
| `CASSIE_ADAPTIVE_CANDIDATE_MIN` | Unsigned integer | `16` | Minimum adaptive candidate count. |
| `CASSIE_ADAPTIVE_CANDIDATE_MAX` | Unsigned integer | `100000` | Maximum adaptive candidate count. |

## Embedding Provider Selection

| Variable | Allowed value | Default | Purpose |
| --- | --- | --- | --- |
| `CASSIE_EMBEDDINGS_PROVIDER` | `disabled`, `fallback`, `openai`, `openai_compatible`, `tei`, `ollama`, `voyage`, `cohere`, or `local` | `disabled` | Selects the embedding provider. `fallback` is accepted as a disabled-provider compatibility value. |
| `CASSIE_EMBEDDINGS_MAX_RESPONSE_BYTES` | Integer bytes, minimum `1` | `8388608` | Maximum success or error response body accepted from a remote embedding provider. |

An unknown provider fails startup. Settings for providers other than the
selected provider have no effect.

### OpenAI

| Variable | Allowed value | Default | Purpose |
| --- | --- | --- | --- |
| `CASSIE_OPENAI_API_KEY` | Secret string | Empty | OpenAI API key. Required for successful remote requests. |
| `CASSIE_OPENAI_MODEL` | Model name | `text-embedding-3-small` | Embedding model. |
| `CASSIE_OPENAI_BASE_URL` | URL | Provider default | Optional endpoint override. |
| `CASSIE_OPENAI_TIMEOUT_SECONDS` | Unsigned seconds | `30` | Request timeout. |
| `CASSIE_OPENAI_MAX_BATCH_SIZE` | Unsigned integer | `16` | Maximum documents per request batch. |
| `CASSIE_OPENAI_MAX_RETRIES` | Unsigned integer | `3` | Maximum retry count. |

### OpenAI-Compatible

| Variable | Allowed value | Default | Purpose |
| --- | --- | --- | --- |
| `CASSIE_EMBEDDINGS_BASE_URL` | URL | Empty | OpenAI-compatible endpoint; must be configured for successful requests. |
| `CASSIE_EMBEDDINGS_API_KEY` | Optional secret string | Unset | Bearer credential for the compatible endpoint. |
| `CASSIE_EMBEDDINGS_MODEL` | Model name | `BAAI/bge-small-en-v1.5` | Embedding model. |
| `CASSIE_EMBEDDINGS_DIMENSIONS` | Unsigned integer | `384` | Output vector dimensions. |
| `CASSIE_EMBEDDINGS_TIMEOUT_SECONDS` | Unsigned seconds | `30` | Request timeout. |
| `CASSIE_EMBEDDINGS_MAX_BATCH_SIZE` | Unsigned integer | `16` | Maximum documents per request batch. |
| `CASSIE_EMBEDDINGS_MAX_RETRIES` | Unsigned integer | `3` | Maximum retry count. |

### TEI

| Variable | Allowed value | Default | Purpose |
| --- | --- | --- | --- |
| `CASSIE_TEI_BASE_URL` | URL | `http://127.0.0.1:8080` | Provider endpoint. |
| `CASSIE_TEI_MODEL` | Model name | `BAAI/bge-small-en-v1.5` | Embedding model. |
| `CASSIE_TEI_DIMENSIONS` | Unsigned integer | `384` | Output vector dimensions. |
| `CASSIE_TEI_TIMEOUT_SECONDS` | Unsigned seconds | `30` | Request timeout. |
| `CASSIE_TEI_MAX_BATCH_SIZE` | Unsigned integer | `32` | Maximum documents per request batch. |
| `CASSIE_TEI_MAX_RETRIES` | Unsigned integer | `3` | Maximum retry count. |

### Ollama

| Variable | Allowed value | Default | Purpose |
| --- | --- | --- | --- |
| `CASSIE_OLLAMA_BASE_URL` | URL | `http://127.0.0.1:11434` | Provider endpoint. |
| `CASSIE_OLLAMA_MODEL` | Model name | `nomic-embed-text` | Embedding model. |
| `CASSIE_OLLAMA_DIMENSIONS` | Unsigned integer | `768` | Output vector dimensions. |
| `CASSIE_OLLAMA_TIMEOUT_SECONDS` | Unsigned seconds | `30` | Request timeout. |
| `CASSIE_OLLAMA_MAX_BATCH_SIZE` | Unsigned integer | `16` | Maximum documents per request batch. |
| `CASSIE_OLLAMA_MAX_RETRIES` | Unsigned integer | `3` | Maximum retry count. |

### Voyage

| Variable | Allowed value | Default | Purpose |
| --- | --- | --- | --- |
| `CASSIE_VOYAGE_API_KEY` | Secret string | Empty | Provider API key; required for successful remote requests. |
| `CASSIE_VOYAGE_BASE_URL` | URL | `https://api.voyageai.com` | Provider endpoint. |
| `CASSIE_VOYAGE_MODEL` | Model name | `voyage-3.5-lite` | Embedding model. |
| `CASSIE_VOYAGE_DIMENSIONS` | Unsigned integer | `1024` | Output vector dimensions. |
| `CASSIE_VOYAGE_TIMEOUT_SECONDS` | Unsigned seconds | `30` | Request timeout. |
| `CASSIE_VOYAGE_MAX_BATCH_SIZE` | Unsigned integer | `16` | Maximum documents per request batch. |
| `CASSIE_VOYAGE_MAX_RETRIES` | Unsigned integer | `3` | Maximum retry count. |

### Cohere

| Variable | Allowed value | Default | Purpose |
| --- | --- | --- | --- |
| `CASSIE_COHERE_API_KEY` | Secret string | Empty | Provider API key; required for successful remote requests. |
| `CASSIE_COHERE_BASE_URL` | URL | `https://api.cohere.com` | Provider endpoint. |
| `CASSIE_COHERE_MODEL` | Model name | `embed-v4.0` | Embedding model. |
| `CASSIE_COHERE_DIMENSIONS` | Unsigned integer | `1536` | Output vector dimensions. |
| `CASSIE_COHERE_TIMEOUT_SECONDS` | Unsigned seconds | `30` | Request timeout. |
| `CASSIE_COHERE_MAX_BATCH_SIZE` | Unsigned integer | `96` | Maximum documents per request batch. |
| `CASSIE_COHERE_MAX_RETRIES` | Unsigned integer | `3` | Maximum retry count. |

### Local

| Variable | Allowed value | Default | Purpose |
| --- | --- | --- | --- |
| `CASSIE_LOCAL_MODEL` | Model label | `cassie-local-hash-v1` | Local deterministic provider model label. |
| `CASSIE_LOCAL_DIMENSIONS` | Unsigned integer | `384` | Output vector dimensions. |

## Benchmark and Compatibility Harness Variables

These variables are read by repository benchmarks or optional compatibility
tests. The Cassie server runtime does not read them.

| Variable | Allowed value | Default | Consumer |
| --- | --- | --- | --- |
| `CASSIE_BENCH_SOAK_DURATION_SECONDS` | Positive seconds | `3600` | Tier 6 benchmark duration; the command-line option takes precedence. |
| `CASSIE_BENCH_DEPLOYMENT_PROFILE_ID` | Deployment-profile identifier | Unset | Benchmark evidence metadata. |
| `CASSIE_BENCH_RUN_ID` | Unique run identifier | Unset | Groups complete benchmark evidence. |
| `CASSIE_RUN_PSQL_COMPAT` | Exactly `1` | Unset | Enables the ignored local `psql` compatibility probe. |
| `CASSIE_PSQL_BIN` | Executable path/name | `psql` | `psql` executable used by the compatibility probe. |
| `CASSIE_RUN_PRISMA_COMPAT` | Exactly `1` | Unset | Enables the ignored local Prisma compatibility probe. |
| `CASSIE_PRISMA_BIN` | Executable path/name | `prisma` | Prisma executable used by the compatibility probe. |
| `CASSIE_RUN_SQLALCHEMY_COMPAT` | Exactly `1` | Unset | Enables the ignored SQLAlchemy compatibility probe. |
| `CASSIE_SQLALCHEMY_PYTHON` | Executable path/name | `python3` | Python executable used by the SQLAlchemy probe. |

## Removed or Unsupported Variables

| Variable | Status | Replacement |
| --- | --- | --- |
| `CASSIE_ADMIN_USER` | Removed; the root identity is fixed | Use username `root`. |
| `CASSIE_ADMIN_PASSWORD` | Removed | Use `CASSIE_ROOT_PASSWORD`. |
| `CASSIE_ADMIN_PASSWORD_FILE` | Removed | Inject `CASSIE_ROOT_PASSWORD` through the deployment secret mechanism. |
| `CASSIE_MIDGE_DATA_DIR` | Removed | Use `CASSIE_STORAGE_PATH`. |
| `CASSIE_MIDGE_ALLOW_FALLBACK` | Removed | Use `CASSIE_STORAGE_MODE=memory` explicitly. |
| `CASSIE_TEMP_SPILL_BUDGET_BYTES` | Ignored; not a runtime setting | Use `CASSIE_QUERY_MEMORY_BUDGET_BYTES`. |

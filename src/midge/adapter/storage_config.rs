use std::env;
use std::path::Path;

use cntryl_midge::{
    AzureCredentialSource, CloudProviderConfig, GcsApiStyle, GcsCredentialSource, OpenOptions,
    S3CredentialSource,
};

use crate::app::CassieError;

const DEFAULT_CLOUD_CACHE_PATH: &str = "./.cassie-cloud-cache";
const DEFAULT_SQRZL_ENDPOINT: &str = "http://127.0.0.1:9000";
const DEFAULT_SQRZL_ACCESS_KEY: &str = "admin";
const DEFAULT_SQRZL_SECRET_KEY: &str = "sqrzl-secret";
const DEFAULT_SQRZL_BUCKET: &str = "cassie";

pub(super) fn open_options(data_dir: &Path) -> Result<OpenOptions, CassieError> {
    open_options_from(data_dir, &|key| env::var(key).ok())
}

fn open_options_from(
    data_dir: &Path,
    read_env: &impl Fn(&str) -> Option<String>,
) -> Result<OpenOptions, CassieError> {
    let mode = env_non_empty(read_env, "CASSIE_STORAGE_MODE")
        .unwrap_or_else(|| "local".to_string())
        .to_ascii_lowercase();
    let builder = match mode.as_str() {
        "memory" => OpenOptions::in_memory(),
        "local" => OpenOptions::local(data_dir),
        "cloud" => {
            let provider_name = required_env(read_env, "CASSIE_STORAGE_PROVIDER")?;
            let provider =
                build_cloud_provider_config(read_env, &provider_name.to_ascii_lowercase())?;
            let cache_path = env_non_empty(read_env, "CASSIE_STORAGE_CACHE_PATH")
                .unwrap_or_else(|| DEFAULT_CLOUD_CACHE_PATH.to_string());
            let prefix = env_non_empty(read_env, "CASSIE_STORAGE_PREFIX").unwrap_or_default();
            OpenOptions::cloud(cache_path, provider, prefix)
        }
        _ => {
            return Err(unsupported(format!(
                "unsupported CASSIE_STORAGE_MODE='{mode}'; expected memory, local, or cloud"
            )))
        }
    };
    builder.build().map_err(CassieError::from)
}

fn build_cloud_provider_config(
    read_env: &impl Fn(&str) -> Option<String>,
    provider: &str,
) -> Result<CloudProviderConfig, CassieError> {
    match provider {
        "sqrzl-s3" => Ok(CloudProviderConfig::s3_compatible_static(
            env_non_empty(read_env, "CASSIE_STORAGE_BUCKET")
                .unwrap_or_else(|| DEFAULT_SQRZL_BUCKET.to_string()),
            env_non_empty(read_env, "CASSIE_STORAGE_ENDPOINT")
                .unwrap_or_else(|| DEFAULT_SQRZL_ENDPOINT.to_string()),
            DEFAULT_SQRZL_ACCESS_KEY,
            DEFAULT_SQRZL_SECRET_KEY,
        )),
        "sqrzl-azure" => Ok(CloudProviderConfig::AzureBlob {
            account: DEFAULT_SQRZL_ACCESS_KEY.to_string(),
            container: env_non_empty(read_env, "CASSIE_STORAGE_CONTAINER")
                .unwrap_or_else(|| DEFAULT_SQRZL_BUCKET.to_string()),
            endpoint: Some(
                env_non_empty(read_env, "CASSIE_STORAGE_ENDPOINT")
                    .unwrap_or_else(|| DEFAULT_SQRZL_ENDPOINT.to_string()),
            ),
            credential: AzureCredentialSource::shared_key(DEFAULT_SQRZL_SECRET_KEY),
        }),
        "sqrzl-gcs" => Ok(CloudProviderConfig::Gcs {
            bucket: env_non_empty(read_env, "CASSIE_STORAGE_BUCKET")
                .unwrap_or_else(|| DEFAULT_SQRZL_BUCKET.to_string()),
            project_id: "sqrzl".to_string(),
            endpoint: Some(
                env_non_empty(read_env, "CASSIE_STORAGE_ENDPOINT")
                    .unwrap_or_else(|| DEFAULT_SQRZL_ENDPOINT.to_string()),
            ),
            api: GcsApiStyle::Xml,
            credential: GcsCredentialSource::hmac_key(
                DEFAULT_SQRZL_ACCESS_KEY,
                DEFAULT_SQRZL_SECRET_KEY,
            ),
        }),
        "aws-s3" => Ok(CloudProviderConfig::aws_s3(
            required_env(read_env, "CASSIE_STORAGE_BUCKET")?,
            required_region(read_env)?,
        )),
        "s3-compatible" => Ok(CloudProviderConfig::S3Compatible {
            bucket: required_env(read_env, "CASSIE_STORAGE_BUCKET")?,
            region: env_non_empty(read_env, "CASSIE_STORAGE_REGION")
                .unwrap_or_else(|| "us-east-1".to_string()),
            endpoint: required_env(read_env, "CASSIE_STORAGE_ENDPOINT")?,
            path_style: env_bool(read_env, "CASSIE_STORAGE_FORCE_PATH_STYLE", true)?,
            credentials: S3CredentialSource::environment(),
        }),
        "minio" => Ok(CloudProviderConfig::s3_compatible_env(
            required_env(read_env, "CASSIE_STORAGE_BUCKET")?,
            required_env(read_env, "CASSIE_STORAGE_ENDPOINT")?,
        )),
        "wasabi" => {
            let bucket = required_env(read_env, "CASSIE_STORAGE_BUCKET")?;
            let region = required_env(read_env, "CASSIE_STORAGE_REGION")?;
            let endpoint = env_non_empty(read_env, "CASSIE_STORAGE_ENDPOINT")
                .unwrap_or_else(|| format!("https://s3.{region}.wasabisys.com"));
            Ok(CloudProviderConfig::S3Compatible {
                bucket,
                region,
                endpoint,
                path_style: true,
                credentials: S3CredentialSource::environment(),
            })
        }
        "oci-s3" => {
            let bucket = required_env(read_env, "CASSIE_STORAGE_BUCKET")?;
            let namespace = required_env(read_env, "CASSIE_STORAGE_NAMESPACE")?;
            let region = required_env(read_env, "CASSIE_STORAGE_REGION")?;
            let endpoint = env_non_empty(read_env, "CASSIE_STORAGE_ENDPOINT").unwrap_or_else(|| {
                format!("https://{namespace}.compat.objectstorage.{region}.oraclecloud.com")
            });
            Ok(CloudProviderConfig::S3Compatible {
                bucket,
                region,
                endpoint,
                path_style: env_bool(read_env, "CASSIE_STORAGE_FORCE_PATH_STYLE", false)?,
                credentials: S3CredentialSource::environment(),
            })
        }
        "azure-blob" => build_azure_blob_provider(read_env),
        "gcs" => build_gcs_provider(read_env),
        other => Err(unsupported(format!(
            "unsupported CASSIE_STORAGE_PROVIDER='{other}'; expected sqrzl-s3, sqrzl-azure, sqrzl-gcs, aws-s3, s3-compatible, minio, wasabi, oci-s3, azure-blob, or gcs"
        ))),
    }
}

fn build_azure_blob_provider(
    read_env: &impl Fn(&str) -> Option<String>,
) -> Result<CloudProviderConfig, CassieError> {
    let container = required_env(read_env, "CASSIE_STORAGE_CONTAINER").map_err(|_| {
        unsupported("azure-blob storage requires CASSIE_STORAGE_CONTAINER".to_string())
    })?;
    let mut provider = if let Some(connection_string) =
        env_non_empty(read_env, "AZURE_STORAGE_CONNECTION_STRING")
    {
        CloudProviderConfig::azure_blob_connection_string(container, connection_string)
    } else {
        let account = env_non_empty(read_env, "AZURE_STORAGE_ACCOUNT_NAME").ok_or_else(|| {
                unsupported(
                    "azure-blob storage requires AZURE_STORAGE_ACCOUNT_NAME or AZURE_STORAGE_CONNECTION_STRING"
                        .to_string(),
                )
            })?;
        if let Some(account_key) = env_non_empty(read_env, "AZURE_STORAGE_ACCOUNT_KEY") {
            CloudProviderConfig::azure_blob_shared_key(account, container, account_key)
        } else if let Some(sas_token) = env_non_empty(read_env, "AZURE_STORAGE_SAS_TOKEN") {
            CloudProviderConfig::azure_blob_sas(account, container, sas_token)
        } else {
            CloudProviderConfig::azure_blob(account, container)
        }
    };

    if let Some(endpoint) = env_non_empty(read_env, "CASSIE_STORAGE_ENDPOINT") {
        provider = provider
            .with_endpoint(endpoint)
            .map_err(|error| unsupported(error.to_string()))?;
    }
    Ok(provider)
}

fn build_gcs_provider(
    read_env: &impl Fn(&str) -> Option<String>,
) -> Result<CloudProviderConfig, CassieError> {
    let bucket = required_env(read_env, "CASSIE_STORAGE_BUCKET")?;
    let mut provider = match (
        env_non_empty(read_env, "GCS_HMAC_ACCESS_ID"),
        env_non_empty(read_env, "GCS_HMAC_SECRET"),
    ) {
        (Some(access_id), Some(secret)) => CloudProviderConfig::gcs_hmac(bucket, access_id, secret),
        (Some(_), None) | (None, Some(_)) => {
            return Err(unsupported(
                "gcs HMAC storage requires both GCS_HMAC_ACCESS_ID and GCS_HMAC_SECRET".to_string(),
            ))
        }
        (None, None) => {
            if let Some(path) = env_non_empty(read_env, "GOOGLE_APPLICATION_CREDENTIALS") {
                CloudProviderConfig::gcs_service_account_file(bucket, path)
            } else {
                CloudProviderConfig::gcs(bucket)
            }
        }
    };
    if let Some(project_id) = env_non_empty(read_env, "GOOGLE_CLOUD_PROJECT") {
        provider = provider
            .with_gcs_project_id(project_id)
            .map_err(|error| unsupported(error.to_string()))?;
    }
    if let Some(endpoint) = env_non_empty(read_env, "CASSIE_STORAGE_ENDPOINT") {
        provider = provider
            .with_endpoint(endpoint)
            .map_err(|error| unsupported(error.to_string()))?;
    }
    Ok(provider)
}

fn required_region(read_env: &impl Fn(&str) -> Option<String>) -> Result<String, CassieError> {
    env_non_empty(read_env, "CASSIE_STORAGE_REGION")
        .or_else(|| env_non_empty(read_env, "AWS_REGION"))
        .or_else(|| env_non_empty(read_env, "AWS_DEFAULT_REGION"))
        .ok_or_else(|| {
            unsupported(
                "aws-s3 storage requires CASSIE_STORAGE_REGION, AWS_REGION, or AWS_DEFAULT_REGION"
                    .to_string(),
            )
        })
}

fn required_env(
    read_env: &impl Fn(&str) -> Option<String>,
    key: &str,
) -> Result<String, CassieError> {
    env_non_empty(read_env, key).ok_or_else(|| unsupported(format!("cloud storage requires {key}")))
}

fn env_non_empty(read_env: &impl Fn(&str) -> Option<String>, key: &str) -> Option<String> {
    read_env(key).and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn env_bool(
    read_env: &impl Fn(&str) -> Option<String>,
    key: &str,
    default: bool,
) -> Result<bool, CassieError> {
    let Some(value) = env_non_empty(read_env, key) else {
        return Ok(default);
    };
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(unsupported(format!(
            "{key} must be true or false; received '{value}'"
        ))),
    }
}

fn unsupported(message: String) -> CassieError {
    CassieError::Unsupported(message)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use cntryl_midge::Storage;

    use super::open_options_from;

    fn reader(values: HashMap<&'static str, &'static str>) -> impl Fn(&str) -> Option<String> {
        move |key| values.get(key).map(ToString::to_string)
    }

    #[test]
    fn should_build_cloud_storage_with_the_same_provider_vocabulary_as_fitz() {
        // Arrange
        let values = HashMap::from([
            ("CASSIE_STORAGE_MODE", "cloud"),
            ("CASSIE_STORAGE_PROVIDER", "aws-s3"),
            ("CASSIE_STORAGE_BUCKET", "cassie-production"),
            ("CASSIE_STORAGE_REGION", "us-east-1"),
        ]);

        // Act
        let options =
            open_options_from(Path::new("./unused"), &reader(values)).expect("cloud options");

        // Assert
        assert!(matches!(options.storage(), Storage::Cloud { .. }));
    }

    #[test]
    fn should_reject_a_provider_name_in_storage_mode() {
        // Arrange
        let values = HashMap::from([("CASSIE_STORAGE_MODE", "aws-s3")]);

        // Act
        let error = open_options_from(Path::new("./unused"), &reader(values))
            .expect_err("provider names are not storage modes");

        // Assert
        assert!(error.to_string().contains("memory, local, or cloud"));
    }
}

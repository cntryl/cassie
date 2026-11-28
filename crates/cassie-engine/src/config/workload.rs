use super::CassieRuntimeConfigError;

#[derive(Debug, Clone)]
pub struct PgwireWorkloadEnv {
    pub aws: Option<PgwireAwsWorkloadEnv>,
    pub azure: Option<PgwireAzureWorkloadEnv>,
    pub google_audience: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PgwireAwsWorkloadEnv {
    pub account_id: String,
    pub issuer: String,
    pub audience: String,
    pub algorithms: Vec<String>,
    pub max_token_lifetime_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct PgwireAzureWorkloadEnv {
    pub tenant_id: String,
    pub audience: String,
    pub token_version: String,
}

const AWS_ACCOUNT: &str = "CASSIE_PGWIRE_AWS_IAM_ACCOUNT_ID";
const AWS_ISSUER: &str = "CASSIE_PGWIRE_AWS_IAM_ISSUER";
const AWS_AUDIENCE: &str = "CASSIE_PGWIRE_AWS_IAM_AUDIENCE";
const AWS_ALGORITHMS: &str = "CASSIE_PGWIRE_AWS_IAM_ALGORITHMS";
const AWS_LIFETIME: &str = "CASSIE_PGWIRE_AWS_IAM_MAX_TOKEN_LIFETIME_SECONDS";
const AZURE_TENANT: &str = "CASSIE_PGWIRE_AZURE_TENANT_ID";
const AZURE_AUDIENCE: &str = "CASSIE_PGWIRE_AZURE_AUDIENCE";
const AZURE_VERSION: &str = "CASSIE_PGWIRE_AZURE_TOKEN_VERSION";
const GOOGLE_AUDIENCE: &str = "CASSIE_PGWIRE_GOOGLE_AUDIENCE";

pub(super) fn from_env(
    reader: &impl Fn(&str) -> Option<String>,
) -> Result<Option<PgwireWorkloadEnv>, CassieRuntimeConfigError> {
    let aws = if [
        AWS_ACCOUNT,
        AWS_ISSUER,
        AWS_AUDIENCE,
        AWS_ALGORITHMS,
        AWS_LIFETIME,
    ]
    .into_iter()
    .any(|key| reader(key).is_some())
    {
        Some(PgwireAwsWorkloadEnv {
            account_id: required(reader, AWS_ACCOUNT)?,
            issuer: required(reader, AWS_ISSUER)?,
            audience: required(reader, AWS_AUDIENCE)?,
            algorithms: reader(AWS_ALGORITHMS)
                .as_deref()
                .unwrap_or("RS256,ES384")
                .split(',')
                .map(|algorithm| algorithm.trim().to_owned())
                .collect(),
            max_token_lifetime_seconds: reader(AWS_LIFETIME)
                .map_or(Ok(300), |value| value.trim().parse::<u64>())
                .map_err(|_| CassieRuntimeConfigError::InvalidWorkloadEnv { key: AWS_LIFETIME })?,
        })
    } else {
        None
    };
    let azure = if [AZURE_TENANT, AZURE_AUDIENCE, AZURE_VERSION]
        .into_iter()
        .any(|key| reader(key).is_some())
    {
        Some(PgwireAzureWorkloadEnv {
            tenant_id: required(reader, AZURE_TENANT)?,
            audience: required(reader, AZURE_AUDIENCE)?,
            token_version: reader(AZURE_VERSION).unwrap_or_else(|| "2.0".into()),
        })
    } else {
        None
    };
    let google_audience = reader(GOOGLE_AUDIENCE)
        .map(|value| nonempty(value, GOOGLE_AUDIENCE))
        .transpose()?;
    Ok(
        (aws.is_some() || azure.is_some() || google_audience.is_some()).then_some(
            PgwireWorkloadEnv {
                aws,
                azure,
                google_audience,
            },
        ),
    )
}

fn required(
    reader: &impl Fn(&str) -> Option<String>,
    key: &'static str,
) -> Result<String, CassieRuntimeConfigError> {
    nonempty(reader(key).unwrap_or_default(), key)
}

fn nonempty(value: String, key: &'static str) -> Result<String, CassieRuntimeConfigError> {
    if value.trim().is_empty() {
        return Err(CassieRuntimeConfigError::InvalidWorkloadEnv { key });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_load_flat_trust_for_all_three_providers() {
        // Arrange
        let settings = [
            (AWS_ACCOUNT, "123456789012"),
            (AWS_ISSUER, "https://fixture.tokens.sts.global.api.aws"),
            (AWS_AUDIENCE, "aws-audience"),
            (AZURE_TENANT, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
            (AZURE_AUDIENCE, "azure-audience"),
            (GOOGLE_AUDIENCE, "google-audience"),
        ];

        // Act
        let trust = from_env(&|key| {
            settings
                .iter()
                .find_map(|(name, value)| (*name == key).then(|| (*value).to_owned()))
        })
        .unwrap()
        .unwrap();

        // Assert
        let aws = trust.aws.unwrap();
        let azure = trust.azure.unwrap();
        assert_eq!(aws.audience, "aws-audience");
        assert_eq!(aws.max_token_lifetime_seconds, 300);
        assert_eq!(aws.algorithms, ["RS256", "ES384"]);
        assert_eq!(azure.audience, "azure-audience");
        assert_eq!(azure.token_version, "2.0");
        assert_eq!(trust.google_audience.as_deref(), Some("google-audience"));
    }

    #[test]
    fn should_reject_an_incomplete_flat_provider_group() {
        // Arrange
        let reader = |key: &str| (key == AWS_AUDIENCE).then(|| "aws-audience".to_owned());

        // Act
        let result = from_env(&reader);

        // Assert
        assert!(matches!(
            result,
            Err(CassieRuntimeConfigError::InvalidWorkloadEnv { key: AWS_ACCOUNT })
        ));
    }

    #[test]
    fn should_reject_flat_trust_with_a_trust_file() {
        // Arrange
        let reader = |key: &str| match key {
            GOOGLE_AUDIENCE => Some("google-audience".to_owned()),
            "CASSIE_PGWIRE_WORKLOAD_TRUST_FILE" => Some("trust.json".to_owned()),
            _ => None,
        };

        // Act
        let result = super::super::CassieRuntimeConfig::from_env_reader(reader);

        // Assert
        assert!(matches!(
            result,
            Err(CassieRuntimeConfigError::ConflictingWorkloadTrustSources)
        ));
    }
}

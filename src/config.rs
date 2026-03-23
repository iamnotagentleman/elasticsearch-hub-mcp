use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static ENV_VAR_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\{([^}]+)}").expect("invalid regex"));

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub enum QueryRule {
    #[serde(rename = "ONLY_READ_OPERATIONS")]
    OnlyReadOperations,
    #[serde(rename = "ALL_ACCESS")]
    AllAccess,
}

impl QueryRule {
    pub fn as_str(&self) -> &'static str {
        match self {
            QueryRule::OnlyReadOperations => "ONLY_READ_OPERATIONS",
            QueryRule::AllAccess => "ALL_ACCESS",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum Credentials {
    #[serde(rename = "basic")]
    Basic { username: String, password: String },
    #[serde(rename = "api_key")]
    ApiKey { api_key: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct SslConfig {
    #[serde(default = "default_true")]
    pub verify_certs: bool,
    pub ca_certs: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct ElasticsearchInstance {
    pub name: String,
    pub url: String,
    #[serde(default = "default_environment")]
    pub environment: String,
    pub query_rule: QueryRule,
    pub index_patterns: Vec<String>,
    pub credentials: Credentials,
    pub ssl: Option<SslConfig>,
    #[serde(default = "default_timeout")]
    pub default_timeout: u64,
}

fn default_environment() -> String {
    "default".to_string()
}

fn default_timeout() -> u64 {
    30
}

/// Replace `${ENV_VAR}` patterns with environment variable values.
fn resolve_env_vars(text: &str) -> Result<String> {
    let mut result = text.to_string();
    // We need to process all matches. Collect them first to avoid borrow issues.
    let matches: Vec<(String, String)> = ENV_VAR_PATTERN
        .captures_iter(text)
        .map(|cap| {
            let full = cap.get(0).unwrap().as_str().to_string();
            let var_name = cap.get(1).unwrap().as_str().to_string();
            (full, var_name)
        })
        .collect();

    for (full_match, var_name) in matches {
        let value = env::var(&var_name).with_context(|| {
            format!(
                "Environment variable '{}' is not set (referenced in config)",
                var_name
            )
        })?;
        result = result.replace(&full_match, &value);
    }

    Ok(result)
}

/// Load and validate ES instance configuration.
///
/// Path resolution: ES_MCP_CONFIG env var > explicit path > ./config.json
pub fn load_config(path: Option<&Path>) -> Result<Vec<ElasticsearchInstance>> {
    let config_path: PathBuf = match path {
        Some(p) => p.to_path_buf(),
        None => {
            if let Ok(env_path) = env::var("ES_MCP_CONFIG") {
                PathBuf::from(env_path)
            } else {
                // Try ./config.json first, then ~/.elasticsearch-hub-mcp/config.json
                let local = PathBuf::from("./config.json");
                if local.exists() {
                    local
                } else {
                    dirs::home_dir()
                        .map(|h| h.join(".elasticsearch-hub-mcp").join("config.json"))
                        .unwrap_or(local)
                }
            }
        }
    };

    if !config_path.exists() {
        bail!(
            "Config file not found: {}",
            config_path.canonicalize().unwrap_or(config_path.clone()).display()
        );
    }

    let raw = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config file: {}", config_path.display()))?;

    let resolved = resolve_env_vars(&raw)?;

    // First parse as generic JSON to validate it's an array
    let value: serde_json::Value =
        serde_json::from_str(&resolved).context("Config file is not valid JSON")?;

    if !value.is_array() {
        bail!("Config must be a JSON array of instance definitions");
    }

    let instances: Vec<ElasticsearchInstance> =
        serde_json::from_value(value).context("Failed to parse instance definitions")?;

    // Enforce unique names
    let names: Vec<&str> = instances.iter().map(|i| i.name.as_str()).collect();
    let mut seen = std::collections::HashSet::new();
    let mut dupes = Vec::new();
    for name in &names {
        if !seen.insert(*name) {
            dupes.push(*name);
        }
    }
    if !dupes.is_empty() {
        dupes.sort();
        dupes.dedup();
        bail!("Duplicate instance names: {:?}", dupes);
    }

    Ok(instances)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_config(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn test_load_basic_config() {
        let config_json = r#"[
            {
                "name": "test-cluster",
                "url": "http://localhost:9200",
                "environment": "QA",
                "query_rule": "ONLY_READ_OPERATIONS",
                "index_patterns": ["logs-*", "metrics-*"],
                "credentials": {"type": "basic", "username": "elastic", "password": "test123"},
                "default_timeout": 10
            },
            {
                "name": "dev-cluster",
                "url": "http://localhost:9201",
                "environment": "DEV",
                "query_rule": "ALL_ACCESS",
                "index_patterns": ["dev-*"],
                "credentials": {"type": "api_key", "api_key": "test-api-key"},
                "ssl": {"verify_certs": false},
                "default_timeout": 5
            }
        ]"#;
        let f = write_config(config_json);
        let instances = load_config(Some(f.path())).unwrap();
        assert_eq!(instances.len(), 2);

        let test = &instances[0];
        assert_eq!(test.name, "test-cluster");
        assert_eq!(test.url, "http://localhost:9200");
        assert_eq!(test.query_rule, QueryRule::OnlyReadOperations);
        assert_eq!(test.index_patterns, vec!["logs-*", "metrics-*"]);
        match &test.credentials {
            Credentials::Basic { username, password } => {
                assert_eq!(username, "elastic");
                assert_eq!(password, "test123");
            }
            _ => panic!("Expected basic credentials"),
        }
        assert_eq!(test.default_timeout, 10);

        let dev = &instances[1];
        assert_eq!(dev.name, "dev-cluster");
        assert_eq!(dev.query_rule, QueryRule::AllAccess);
        match &dev.credentials {
            Credentials::ApiKey { api_key } => {
                assert_eq!(api_key, "test-api-key");
            }
            _ => panic!("Expected api_key credentials"),
        }
        assert!(dev.ssl.is_some());
        assert!(!dev.ssl.as_ref().unwrap().verify_certs);
    }

    #[test]
    fn test_env_var_substitution() {
        // SAFETY: test-only, single-threaded
        unsafe {
            env::set_var("TEST_RS_ES_USER", "admin");
            env::set_var("TEST_RS_ES_PASS", "secret");
        }

        let config_json = r#"[{
            "name": "env-test",
            "url": "http://localhost:9200",
            "environment": "QA",
            "query_rule": "ALL_ACCESS",
            "index_patterns": ["*"],
            "credentials": {
                "type": "basic",
                "username": "${TEST_RS_ES_USER}",
                "password": "${TEST_RS_ES_PASS}"
            }
        }]"#;
        let f = write_config(config_json);
        let instances = load_config(Some(f.path())).unwrap();
        match &instances[0].credentials {
            Credentials::Basic { username, password } => {
                assert_eq!(username, "admin");
                assert_eq!(password, "secret");
            }
            _ => panic!("Expected basic credentials"),
        }

        // SAFETY: test-only, single-threaded
        unsafe {
            env::remove_var("TEST_RS_ES_USER");
            env::remove_var("TEST_RS_ES_PASS");
        }
    }

    #[test]
    fn test_missing_env_var() {
        let config_json = r#"[{
            "name": "env-test",
            "url": "http://localhost:9200",
            "query_rule": "ALL_ACCESS",
            "index_patterns": ["*"],
            "credentials": {
                "type": "basic",
                "username": "${NONEXISTENT_RS_VAR_12345}",
                "password": "test"
            }
        }]"#;
        let f = write_config(config_json);
        let err = load_config(Some(f.path())).unwrap_err();
        assert!(
            err.to_string().contains("NONEXISTENT_RS_VAR_12345"),
            "Error should mention the missing var: {}",
            err
        );
    }

    #[test]
    fn test_duplicate_instance_names() {
        let config_json = r#"[
            {
                "name": "dupe",
                "url": "http://localhost:9200",
                "query_rule": "ALL_ACCESS",
                "index_patterns": ["*"],
                "environment": "QA",
                "credentials": {"type": "api_key", "api_key": "key1"}
            },
            {
                "name": "dupe",
                "url": "http://localhost:9201",
                "query_rule": "ALL_ACCESS",
                "index_patterns": ["*"],
                "environment": "QA",
                "credentials": {"type": "api_key", "api_key": "key2"}
            }
        ]"#;
        let f = write_config(config_json);
        let err = load_config(Some(f.path())).unwrap_err();
        assert!(
            err.to_string().contains("Duplicate instance names"),
            "Error: {}",
            err
        );
    }

    #[test]
    fn test_config_file_not_found() {
        let err = load_config(Some(Path::new("/nonexistent/config.json"))).unwrap_err();
        assert!(err.to_string().contains("not found"), "Error: {}", err);
    }

    #[test]
    fn test_invalid_json_array() {
        let f = write_config(r#"{"not": "an array"}"#);
        let err = load_config(Some(f.path())).unwrap_err();
        assert!(
            err.to_string().contains("JSON array"),
            "Error: {}",
            err
        );
    }
}

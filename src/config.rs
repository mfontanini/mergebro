use config::{Config, ConfigError, Environment, File};
use serde_derive::Deserialize;

#[derive(Deserialize, Debug)]
pub struct MergebroConfig {
    pub github: GithubConfig,
    #[serde(default)]
    pub workflows: WorkflowsConfig,
}

#[derive(Deserialize, Debug)]
pub struct GithubConfig {
    pub username: String,
    pub token: String,
}

#[derive(Deserialize, Debug, Default)]
pub struct WorkflowsConfig {
    pub circleci: Option<CircleCiConfig>,
}

#[derive(Deserialize, Debug)]
pub struct CircleCiConfig {
    pub token: String,
}

impl MergebroConfig {
    pub fn new(config_file_path: &str) -> Result<Self, ConfigError> {
        let mut config = Config::new();
        let config_file_path = shellexpand::tilde(config_file_path);
        config.merge(File::with_name(&config_file_path).required(false))?;
        config.merge(Environment::with_prefix("mergebro").separator("_"))?;
        config.try_into()
    }
}

use env_logger::Env;
use log::{error, info};
use mergebro::{
    circleci::{CircleCiWorkflowRunner, DefaultCircleCiClient},
    github::{DefaultGithubClient, MergeMethod, PullRequestIdentifier},
    Director, DirectorState, MergeConfig, MergebroConfig, PullRequestMerger, WorkflowRunner,
};
use reqwest::Url;
use std::process::exit;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let config = match MergebroConfig::new("~/.mergebro/config.yaml") {
        Ok(config) => config,
        Err(e) => {
            error!("Error parsing config: {}", e);
            exit(1);
        }
    };

    let github_client = DefaultGithubClient::new(config.github.username, config.github.token);
    let url = Url::parse(&std::env::args().nth(1).expect("Missing PR")).expect("invalid url");
    let identifier = PullRequestIdentifier::from_app_url(&url).unwrap();

    let mut workflow_runners: Vec<Arc<dyn WorkflowRunner>> = Vec::new();
    if config.workflows.circleci.is_some() {
        let circleci_client = Arc::new(DefaultCircleCiClient::new(
            config.workflows.circleci.unwrap().token,
        ));
        workflow_runners.push(Arc::new(CircleCiWorkflowRunner::new(circleci_client)));
    }

    // TODO: configurable
    let merger = PullRequestMerger::new(MergeConfig {
        default_merge_method: MergeMethod::Squash,
    });
    let sleep_duration = Duration::from_secs(30);

    let mut director = Director::new(github_client, workflow_runners, identifier, merger);
    loop {
        info!("Running checks on pull request...");
        match director.run().await {
            Ok(DirectorState::Pending) => {
                info!("Sleeping for {} seconds", sleep_duration.as_secs());
                sleep(sleep_duration).await;
            }
            Ok(DirectorState::Done) => {
                break;
            }
            Err(e) => {
                error!("Error processing pull request: {}", e);
                break;
            }
        }
    }
}

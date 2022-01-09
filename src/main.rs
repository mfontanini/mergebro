use env_logger::Env;
use log::{error, info};
use mergebro::{
    circleci::{CircleCiWorkflowRunner, DefaultCircleCiClient},
    github::{DefaultGithubClient, MergeMethod, PullRequestIdentifier},
    Director, DirectorState, MergeConfig, PullRequestMerger, WorkflowRunner,
};
use reqwest::Url;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // TODO: parametrize
    let github_user = env::var("GITHUB_API_USER").unwrap();
    let github_token = env::var("GITHUB_API_TOKEN").unwrap();
    let circleci_token = env::var("CIRCLECI_API_TOKEN").unwrap();

    let github_client = DefaultGithubClient::new(github_user, github_token);
    let circleci_client = DefaultCircleCiClient::new(circleci_token);
    let url = Url::parse(&std::env::args().nth(1).expect("Missing PR")).expect("invalid url");
    let identifier = PullRequestIdentifier::from_app_url(&url).unwrap();

    let workflow_runners: Vec<Arc<dyn WorkflowRunner>> = vec![Arc::new(
        CircleCiWorkflowRunner::new(circleci_client.into()),
    )];

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

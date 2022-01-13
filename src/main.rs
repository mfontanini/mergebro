use env_logger::Env;
use log::{error, info};
use mergebro::{
    circleci::{CircleCiWorkflowRunner, DefaultCircleCiClient},
    github::{DefaultGithubClient, PullRequestIdentifier},
    Director, DirectorState, MergeConfig, MergebroConfig, PullRequestMerger, WorkflowRunner,
};
use reqwest::Url;
use std::process::exit;
use std::sync::Arc;
use std::time::Duration;
use structopt::StructOpt;
use tokio::time::sleep;

#[derive(StructOpt, Debug)]
#[structopt(name = "mergebro")]
struct Options {
    #[structopt(short, long, default_value = "~/.mergebro/config.yaml")]
    config_file: String,

    #[structopt(name = "pull_request_url")]
    pull_request_url: String,
}

fn parse_pull_request_url(url: &str) -> Result<PullRequestIdentifier, Box<dyn std::error::Error>> {
    let url = Url::parse(url)?;
    let pull_request_id = PullRequestIdentifier::from_app_url(&url)?;
    Ok(pull_request_id)
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let options = Options::from_args();
    let config = match MergebroConfig::new(&options.config_file) {
        Ok(config) => config,
        Err(e) => {
            error!("Error parsing config: {}", e);
            exit(1);
        }
    };

    let github_client = DefaultGithubClient::new(&config.github.username, config.github.token);
    let identifier = match parse_pull_request_url(&options.pull_request_url) {
        Ok(identifier) => identifier,
        Err(e) => {
            error!("Error parsing pull request URL: {}", e);
            exit(1);
        }
    };

    let mut workflow_runners: Vec<Arc<dyn WorkflowRunner>> = Vec::new();
    if config.workflows.circleci.is_some() {
        let circleci_client = Arc::new(DefaultCircleCiClient::new(
            config.workflows.circleci.unwrap().token,
        ));
        workflow_runners.push(Arc::new(CircleCiWorkflowRunner::new(circleci_client)));
    }

    if workflow_runners.is_empty() {
        info!("No external workflow runners configured");
    } else {
        info!("Using {} external workflow runners", workflow_runners.len());
    }

    let merger = PullRequestMerger::new(MergeConfig {
        default_merge_method: config.merge.default_method,
    });

    let sleep_duration = Duration::from_secs(config.poll.delay_seconds as u64);
    info!(
        "Starting loop on pull request: {}/{}/pulls/{} using github user {}",
        identifier.owner, identifier.repo, identifier.pull_number, config.github.username
    );
    let mut director = Director::new(github_client, workflow_runners, identifier, merger);
    loop {
        info!("Running checks on pull request...");
        match director.run().await {
            Ok(DirectorState::Waiting) => {
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

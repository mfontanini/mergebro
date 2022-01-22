use env_logger::Env;
use log::{error, info};
use mergebro::{
    circleci::{CircleCiWorkflowRunner, DefaultCircleCiClient},
    common::{RepoIdentifier, RepoMap},
    config::{ReviewsConfig, StatusFailuresConfig},
    github::{DefaultGithubClient, GithubClient, PullRequestIdentifier},
    processing::{
        steps::{
            CheckBehindMaster, CheckBuildFailed, CheckCurrentStateStep, CheckReviewsStep, Step,
        },
        DefaultPullRequestMerger, DummyPullRequestMerger, PullRequestMerger,
    },
    Director, DirectorState, MergebroConfig, WorkflowRunner,
};
use reqwest::Url;
use std::collections::HashMap;
use std::error::Error;
use std::process::exit;
use std::sync::Arc;
use std::time::Duration;
use structopt::StructOpt;
use tokio::time::sleep;

#[derive(StructOpt, Debug)]
#[structopt(name = "mergebro")]
struct Options {
    /// The path to the YAML configuration file
    #[structopt(short, long, default_value = "~/.mergebro/config.yaml")]
    config_file: String,

    /// Whether to simply run checks but not actually merge the pull request
    #[structopt(short, long)]
    dry_run: bool,

    /// Whether to ignore checks for pull request reviews
    #[structopt(short = "r")]
    ignore_reviews: bool,

    /// The pull request to be processed
    #[structopt(name = "pull_request_url")]
    pull_request_url: String,
}

fn parse_pull_request_url(url: &str) -> Result<PullRequestIdentifier, Box<dyn Error>> {
    let url = Url::parse(url)?;
    let pull_request_id = PullRequestIdentifier::from_app_url(&url)?;
    Ok(pull_request_id)
}

struct SplitRepoConfigs {
    reviews_config: RepoMap<ReviewsConfig>,
    status_failures_config: RepoMap<HashMap<String, StatusFailuresConfig>>,
}

fn split_repo_configs(config: &MergebroConfig) -> Result<SplitRepoConfigs, Box<dyn Error>> {
    let mut reviews_config = RepoMap::new(config.reviews.clone());
    let mut status_failures_config = RepoMap::default();
    for repo_config in &config.repos {
        let repo: RepoIdentifier = repo_config.repo.parse()?;
        if let Some(reviews) = &repo_config.reviews {
            reviews_config.insert(repo.clone(), reviews.clone())?;
        }
        if !repo_config.statuses.is_empty() {
            let mut status_config = HashMap::new();
            for status in &repo_config.statuses {
                // TODO: dedup
                status_config.insert(status.name.clone(), status.failures.clone());
            }
            status_failures_config.insert(repo.clone(), status_config)?;
        }
    }
    Ok(SplitRepoConfigs {
        reviews_config,
        status_failures_config,
    })
}

fn build_steps(
    id: &PullRequestIdentifier,
    github_client: Arc<dyn GithubClient>,
    workflow_runners: Vec<Arc<dyn WorkflowRunner>>,
    config: &MergebroConfig,
    ignore_reviews: bool,
) -> Result<Vec<Box<dyn Step>>, Box<dyn Error>> {
    let split_repo_configs = split_repo_configs(config)?;
    let mut steps: Vec<Box<dyn Step>> = vec![
        Box::new(CheckCurrentStateStep::default()),
        Box::new(CheckBehindMaster::new(id.clone(), github_client.clone())),
        Box::new(CheckBuildFailed::new(
            github_client.clone(),
            workflow_runners,
            split_repo_configs
                .status_failures_config
                .get(&id.owner, &id.repo)
                .clone(),
        )?),
    ];
    if !ignore_reviews {
        steps.push(Box::new(CheckReviewsStep::new(
            id.clone(),
            github_client.clone(),
            split_repo_configs
                .reviews_config
                .get(&id.owner, &id.repo)
                .clone(),
        )?));
    }
    Ok(steps)
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

    let github_client = Arc::new(DefaultGithubClient::new(
        &config.github.username,
        config.github.token.clone(),
    ));
    let identifier = match parse_pull_request_url(&options.pull_request_url) {
        Ok(identifier) => identifier,
        Err(e) => {
            error!("Error parsing pull request URL: {}", e);
            exit(1);
        }
    };

    let mut workflow_runners: Vec<Arc<dyn WorkflowRunner>> = Vec::new();
    if config.workflows.circleci.is_some() {
        let token = config.workflows.circleci.as_ref().unwrap().token.clone();
        let circleci_client = Arc::new(DefaultCircleCiClient::new(token));
        workflow_runners.push(Arc::new(CircleCiWorkflowRunner::new(circleci_client)));
    }

    if workflow_runners.is_empty() {
        info!("No external workflow runners configured");
    } else {
        info!("Using {} external workflow runners", workflow_runners.len());
    }

    let merger: Arc<dyn PullRequestMerger> = if options.dry_run {
        info!("Running in dry-run mode");
        Arc::new(DummyPullRequestMerger::default())
    } else {
        Arc::new(DefaultPullRequestMerger::new(config.merge.clone()))
    };

    let sleep_duration = Duration::from_secs(config.poll.delay_seconds as u64);
    info!(
        "Starting loop on pull request: {}/{}/pulls/{} using github user {}",
        identifier.owner, identifier.repo, identifier.pull_number, config.github.username
    );
    let steps = build_steps(
        &identifier,
        github_client.clone(),
        workflow_runners,
        &config,
        options.ignore_reviews,
    );
    let steps = match steps {
        Ok(steps) => steps,
        Err(e) => {
            error!("Failed to initialize step checks: {}", e);
            exit(1);
        }
    };
    let mut director = Director::new(github_client, merger, steps, identifier);
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

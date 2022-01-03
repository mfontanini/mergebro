use env_logger::Env;
use log::{debug, error};
use mergebro::{
    github::{DefaultGithubClient, PullRequestIdentifier},
    Director, DirectorState,
};
use reqwest::Url;
use std::env;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();

    // TODO: parametrize
    let github_user = env::var("GITHUB_API_USER").unwrap();
    let github_token = env::var("GITHUB_API_TOKEN").unwrap();

    let client = DefaultGithubClient::new(github_user, github_token);
    let url = Url::parse(&std::env::args().nth(1).expect("Missing PR")).expect("invalid url");
    let identifier = PullRequestIdentifier::from_app_url(&url).unwrap();

    let mut director = Director::new(client, identifier);
    loop {
        match director.run().await {
            Ok(DirectorState::Pending) => {
                debug!("Waiting for pull request to be ready");
                sleep(Duration::from_secs(30)).await;
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

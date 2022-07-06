# mergebro

A utility that merges pull requests for you.

This deals with:
* Updating the latest upstream changes into the pull request's branch, if needed.
* Re-triggering CI jobs when they fail. Github actions and CircleCI workflows are supported so far.
* Merging the pull request when all the checks have passed.

---

This tool was born out of the hassles of merging a pull request into repositories that:

* Have many people touching them and require your branch to be up to date with master before merging. This means if your pull request is ready to be merged but someone merges another one _right before you_, then you need to re-run your entire CI pipeline before merging yours.
* Have fairly long CI workflows. This combined with the point above makes for a tedious cycle: if you want to merge your pull request, you need to wait for a while until the CI build ends but not too long as otherwise someone could merge another change, which would mean you'd have to wait for the CI to run again.
* Have flaky tests.

## Building

In order to build:

1. Install [Rust](https://www.rust-lang.org/learn/get-started).
2. Run `cargo build`

## Configuration


The configuration for `mergebro` can either be stored in a `yaml` (the sample `config.sample.yaml` file) or via environment variables.

The configuration file will be looked up by default in `~/.mergebro/config.yaml` but this path can be modified by passing in the `-c` command line argument.

### Github

The only required configuration property is your Github username and an API token with `repo` scope. You can get the token here: https://github.com/settings/tokens

In order to pass these in via environment variables, use the following:

```bash
export MERGEBRO_GITHUB_USERNAME=mfontanini
export MERGEBRO_GITHUB_TOKEN=my_secret_api_token
```

### CircleCI

By configuring a CircleCI API token, failed jobs for that service can be re-ran. You can get the token here: https://app.circleci.com/settings/user/tokens

In order to pass it in via an environment variable, use the following:

```bash
export MERGEBRO_WORKFLOWS_CIRCLECI_TOKEN=my_secret_api_token
```

### Merge method

The default pull request merge method is to create a merge commit, but this can be configured via the configuration file. Note that this is simply a default, and other methods will be attempted if the target repo is configured to only allow a subset of the available merge methods.

## Running

Once you have configured `mergebro`, just run it with the URL of the pull request you want to merge:


```
cargo run https://github.com/mfontanini/mergebro/pull/1337
```

## Notes

There's definitely edge cases this does not yet handle, which will be fixed as they come up. For example, I'm fairly certain something won't go well if you run this on a forked repo PR, but I haven't had the chance to test that scenario.

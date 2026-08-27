# Qovery Engine

The Qovery Engine is an abstraction layer to deploy stateless and stateful applications on any Cloud providers.
It also bootstraps Kubernetes clusters and mandatory elements (network) for clients.

## Packages

### qovery-engine

Qovery engine is able to deploy complete clusters environments and deploy client's applications inside a deployed clusters.

### docker

Files to make images that should run for builds or Qovery application run.

### app

Qovery binary application

## Prerequisites

### Binaries

- docker
- terraform
- helm
- kubectl
- aws cli

## Get Started

## Setup git hook

In order to get your next MR validated, linter, fmt etc...there is a pre commit hook we suggest to install:

```shell
./helper.sh install_hook
```

The pre-commit hook runs `./helper.sh lint` (fast lint).  
For the full integration-feature clippy matrix, run:

```shell
mise run lint-matrix
```

### Run locally

1. Install terraform binary to be used by the engine.

   ```shell
   TERRAFORM_VERSION=$(grep 'TERRAFORM_VERSION' docker/bin_versions | cut -d= -f2 | tr -d '"')
   OS=$(uname -s | tr '[:upper:]' '[:lower:]')
   ARCH=$(uname -m | sed 's/x86_64/amd64/;s/arm64/arm64/')

   curl -fsSL "https://releases.hashicorp.com/terraform/${TERRAFORM_VERSION}/terraform_${TERRAFORM_VERSION}_${OS}_${ARCH}.zip" -o /tmp/terraform.zip
   sudo unzip -o /tmp/terraform.zip -d /usr/local/bin/ && rm /tmp/terraform.zip

   # Ensure /usr/local/bin is in your PATH (add to ~/.zshrc or ~/.bashrc if missing)
   export PATH="$PATH:/usr/local/bin"
   ```

1. Set up the `LIB_ROOT_DIR` and `WORKSPACE_ROOT_DIR` variable

```shell
export lib_root_dir="<path-to-engine-repo>/engine/lib-engine/lib/"
export WORKSPACE_ROOT_DIR="<up-to-your-preferences>"
```

- `LIB_ROOT_DIR`: The absolute path of the engine's lib folder
- WORKSPACE_ROOT_DIR: The absolute path to the location where `.qovery-workspace` folder will be located, this folder contains the rendered config

3. Run cargo

```shell
cargo run --package app --bin engine
```

- You can find the terraform version in [bin_versions](docker/bin_versions) under `TERRAFORM_VERSION`.

### Generate a new image version

To generate a new Engine image version, you have to use Gitlab and GitHub:

1. On GitHub, ensure your wished commits are stored in dev or master branch.
2. On Gitlab, run a dev or main pipeline to generate images and push to repository

Note: naming image tags is made of the first 7 chars Github commit id + a dash + 7 first chars Gitlab commit id

## Release process

### Production clusters

#### Identify version to deploy

- Check engine pipelines <https://gitlab.com/qovery/backend/engine/-/pipelines?scope=tags&page=1>
- Find the latest version deployed on non-production clusters for at least 48 hours

#### Promote the version to production channel

- Open the pipeline linked to the version to deploy
- Trigger the job: `2-deploy-qovery-infra-engines-prod`

#### Dry-run deployment

- Go to engine pipelines <https://gitlab.com/qovery/backend/engine/-/pipelines?scope=tags&page=1>, filter by tag and select the version to deploy
- Trigger the job: `4-dry-run-deploy-prod-clusters`
- The AI check job `5-ai-check-prod-clusters` runs automatically — review its findings before proceeding
- Analyse terraform & helm diff for unexpected change: <https://qortal.qovery.com/grafana/d/ae51ecxhq2tj4a/infra-cluster-diff?orgId=1&from=now-3h&to=now&var-cluster=&var-tffilter=%28-%20%7C~%20%29>.
- If everything is fine, proceed to the next step

#### Deployment

- Execute the following command: `qovery admin cluster deploy  --parallel-run 50 --filters IsProduction=true --execution-mode on-the-fly --disable-dry-run`
- Monitor deployments on grafana: <https://qortal.qovery.com/grafana/d/e9365ed8-1bca-4aea-a010-44a05fe64a68/deployments?orgId=1&refresh=30s>

### Non-production clusters

#### Dry-run deployment

- Go to engine pipelines <https://gitlab.com/qovery/backend/engine/-/pipelines?scope=tags&page=1>, filter by tag and select the version to deploy
- Trigger the job: `3-dry-run-deploy-dev-clusters`
- The AI check job `4-ai-check-dev-clusters` runs automatically — review its findings before proceeding
- Analyse terraform & helm diff for unexpected change: <https://qortal.qovery.com/grafana/d/ae51ecxhq2tj4a/infra-cluster-diff?orgId=1&from=now-3h&to=now&var-cluster=&var-tffilter=%28-%20%7C~%20%29>.
- If everything is fine, proceed to the next step

#### Deployment

- Execute the following command: `qovery admin cluster deploy  --parallel-run 50 --filters IsProduction=false --execution-mode on-the-fly --disable-dry-run`
- Monitor deployments on grafana: <https://qortal.qovery.com/grafana/d/e9365ed8-1bca-4aea-a010-44a05fe64a68/deployments?orgId=1&refresh=30s>

## Hot fix process

1. Create a branch whose name **starts with** `hot-fix` (the build jobs gate on `$CI_COMMIT_BRANCH =~ /^hot-fix/`), i.e:

- `hot-fix-staging` for staging: useful if we don't want some commits already merged in `main`
- `hot-fix-prod` for prod: the branch should be based on last prod tag / commit

```sh
git co -b hot-fix-staging
git add .
git commit
git push origin HEAD:hot-fix-staging
```

1. Once the target branch has been pushed, a **branch pipeline** should be created with the following jobs:

- release-image
- release-image-slim
- create-multi-arch-image

  > ⚠️ Do **not** open a merge request for this branch. An MR creates a `merge_request_event` pipeline where `$CI_COMMIT_BRANCH` is unset, so `create-multi-arch-image` is skipped — and without it the plain `engine:<sha>` image is never built, which makes the later `docker-tag` job fail with `engine:<sha>: not found`.

1. Wait for `create-multi-arch-image` to finish (it pushes `engine:<sha>`), then push a tag on the HEAD of the target branch

   ```sh
   git tag vX.Y.X
   git push origin vX.Y.X
   ```

2. Once the tag has been pushed, a new pipeline should be created with the jobs we use for regular release:

- docker-tag
- gitlab-release
- ...

1. Trigger the necessary jobs to deploy either the staging or the production infra engines

## AI Check

After each dry-run, Claude automatically reviews the Terraform and Helm plan diffs across all clusters and flags anything worth attention before the actual deploy. It is not meant to replace human review but to assist it and reduce the risk of missing something.

Under the hood, `scripts/ci_release_ai_check.py` fetches the diff logs from Loki for each cluster, normalizes sensitive data (UUIDs, ARNs, IPs, account IDs), and sends the Terraform/Helm diffs to Claude for analysis. Findings are returned as structured JSON and categorized by severity (`critical`, `review`, `info`). The job is non-blocking (`allow_failure: true`) — always review its output before proceeding to the actual deploy.

## Supported connectors

### Build Platforms

TODO

### Container Registry

TODO

### Cloud Providers

TODO

## Run Tests

### How to deploy new test cluster

_Note_: Make sure `LIB_ROOT_DIR` and `WORKSPACE_ROOT_DIR` are set.

#### GKE

1. `gcloud auth login`
2. `gcloud components install gke-gcloud-auth-plugin`
3. `cargo nextest run --features test-gcp-infra -E 'test(create_and_destroy_gke_cluster_in_europe_west_10)' --no-capture`
   create_and_destroy_eks_cluster

#### EKS

`cargo nextest run --features test-aws-infra -E 'test(create_and_destroy_eks_cluster)' --no-capture`

##### Rendered configuration

1. You can find the cluster's rendered configuration at `$WORKSPACE_ROOT_DIR/<Excution date>/bootstrap/<Cluster-name>/terraform`
   e.g: `~/.qovery-workspace/2026-04-02T09-52-18-819264-00-00/bootstrap/zf03426ac/terraform`
2. To get the connection info to the cluster run `./helper.sh get_connection_details`

## How to deploy an application

```shell script
RUST_LOG=info LIB_ROOT_DIR=~/qovery-engine/lib WORKSPACE_ROOT_DIR=~/.qovery-workspace cargo test --package qovery-engine --test aws_environment deploy_a_working_development_environment_with_all_options_on_aws_eks -- --exact --nocapture
```

## Add a new test

How to add a test in a fast or long process? Simply add "#[ignore]" as a test annotation (I know it's not really convenient to get it, but it's how it works in Rust). If the annotation is missing, it will be considered as a fast test.

# Debug

If you have a json context, and you want to deploy for investigation, you need to set 2 environment variables:

```bash
DEPLOY_FROM_FILE=<path_tojson_file>
DEPLOY_FROM_FILE_KIND=<env|infra> # choose between infra (infrastructure deployment) and env (environment deployment)
```

## FAQ

- How to activate the debugger: add the `RUST_LOG=qovery_engine=debug` env variable
- How to update the rust-toolchain. The CI image is authoritative: the toolchain it bakes is the one
  everything runs, and `RUSTUP_AUTO_INSTALL=0` makes a job fail rather than download a different one.
  So the image comes first, the pins follow:
  1. In the `container-image-mirror` repository, set `RUST_VERSION` in the `build-ci-image` job to the new
     version and run that manual job. It publishes `qovery-ci:rust-<version>-<date>`.
  1. Point `ARG RUST_IMAGE` in the `Dockerfile` at that tag, then build the engine CI image with the
     `build-engine-ci-image` pipeline job and set the resulting `qovery-ci:engine-<date>` tag as `image:`
     in `.gitlab-ci.yml`.
  1. Set the same version in `rust-toolchain.toml`:

      ```toml
      [toolchain]
      channel = "1.98.0"
      components = ["rustfmt", "clippy"]
      ```

     This file is what rustup and `mise` both read (`mise` via `idiomatic_version_file_enable_tools` in
     `mise.toml`), so `mise run <task>` uses the same toolchain as CI. It must name the version baked in
     the image, never another one.
  1. Expect new clippy findings: `-D warnings` promotes every lint the new release adds. `cargo clippy
     --fix --all --all-features --tests` handles the mechanical ones, then run `mise run lint`.

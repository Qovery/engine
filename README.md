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

### qovery-engine-task-manager
Task manager is made to handle coming tasks from NATS and run them with the engine.

## Prerequisites
### Binaries
* docker
* terraform
* helm
* kubectl
* aws-iam-authenticator

## Get Started

## Setup git hook
In order to get your next MR validated, linter, fmt etc...there is a pre commit hook we suggest to install:
```shell
./helper.sh install_hook
```

### Run locally
```shell
# Modify version if necessary to match your bin_versions file
curl https://releases.hashicorp.com/terraform/[TERRAFORM_REQUIRED_VERSION]/terraform_[TERRAFORM_REQUIRED_VERSION]_linux_amd64.zip -o /tmp/terraform.zip
sudo unzip /tmp/terraform.zip -d /usr/local/bin/

# Ensure /usr/local/bin is in your path
cargo run 
```
- `[TERRAFORM_REQUIRED_VERSION]` to be replaced by the version listed in [bin_versions](docker/bin_versions) under `TERRAFORM_VERSION`.

### Generate a new image version
To generate a new Engine image version, you have to use Gitlab and GitHub:
1. On GitHub, ensure your wished commits are stored in dev or master branch.
2. On Gitlab, run a dev or main pipeline to generate images and push to repository

Note: naming image tags is made of the first 7 chars Github commit id + a dash + 7 first chars Gitlab commit id

## Supported connectors
### Build Platforms
TODO

### Container Registry
TODO

### Cloud Providers
TODO

## Run Tests

You can deploy a new cluster:
```shell script
RUST_LOG=info LIB_ROOT_DIR=~/qovery-engine/lib WORKSPACE_ROOT_DIR=~/.qovery-workspace cargo test --package qovery-engine --test aws_kubernetes create_eks_cluster_in_us_east_2 -- --exact --nocapture
```

And deploy an application:
```shell script
RUST_LOG=info LIB_ROOT_DIR=~/qovery-engine/lib WORKSPACE_ROOT_DIR=~/.qovery-workspace cargo test --package qovery-engine --test aws_environment deploy_a_working_development_environment_with_all_options_on_aws_eks -- --exact --nocapture
```

* RUST_LOG: log level
* LIB_ROOT_DIR: where the lib folder is located
* WORKSPACE_ROOT_DIR: where the rendered config will be located

## Add a new test

How to add a test in a fast or long process? Simply add "#[ignore]" as a test annotation (I know it's not really convenient to get it, but it's how it works in Rust). If the annotation is missing, it will be considered as a fast test.

## How to deploy new test cluster

```
token=$(cat ~/.qovery/access_token)
curl --request POST \
     --url https://api.qovery.com/api/v1/infrastructure/init \
     --header "authorization: Bearer $token" \
     --header 'content-type: application/json' \
     --data '{
       "build_platform": {
           "id": "oxqlm3r99vwcmvuj"
       },
       "container_registry": {
           "id": "ea59qe62xaw3wjai"
       },
       "kubernetes": {
           "id": "dmubm9agk7sr8a8r"
       },
       "dns_provider": {
           "id": "qoverytestdnsclo"
       }
   }'

```

# Debug

If you have a json context, and you want to deploy for investigation, you need to set 2 environment variables:
```bash
DEPLOY_FROM_FILE=<path_tojson_file>
DEPLOY_FROM_FILE_KIND=<env|infra> # choose between infra (infrastructure deployment) and env (environment deployment)
```

## Contribute

To activate the debugger add the `RUST_LOG=qovery_engine=debug` env var


# Qovery Engine

The Qovery Engine is an abstraction layer to deploy stateless and stateful applications on any Cloud providers.delete
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

### qovery-engine-shared
TDOO

## Prerequisites
### Binaries
* docker
* terraform
* helm
* kubectl
* aws-iam-authenticator

## Get Started

### Run locally
```shell
# somewhere on your computer
# git clone git@github.com:Qovery/engine.git
ln -s path_to_qovery/engine cloned-engine
cp docker/bin_versions .

# Modify version if necessary to match your bin_versions file
curl https://releases.hashicorp.com/terraform/0.13.5/terraform_0.13.5_linux_amd64.zip -o /tmp/terraform.zip
sudo unzip /tmp/terraform.zip -d /usr/local/bin/

# Ensure /usr/local/bin is in your path
cargo run 
```

### Generate a new image version
To generate a new Engine image version, you have to use Gitlab and GitHub:
1. On GitHub, ensure your wished commits are stored in dev or master branch.
2. On Gitlab, run a dev or mmain pipeline to generate images and push to repository

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

## Contribute

To active the debugger add the `RUST_LOG=qovery_engine=debug` env var

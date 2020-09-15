# Qovery Engine

The Qovery Engine is an abstraction layer to deploy stateless and stateful applications on any Cloud providers.delete
It also bootstraps Kubernetes clusters and mandatory elements (network) for clients.

## Features
* TODO
* TODO

## Packages
### qovery-engine
Qovery engine is able to deploy complete clusters environments and deploy client's applications inside a deployed clusters.

### docker
Files to make images that should run for builds or Qovery application run.

### app
TODO

### qovery-engine-task-manager
TODO

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
TODO

### Generate a new image version
To generate a new Engine image version, you have to use Gitlab. Simply push on master and run a build job on Gitlab:
https://gitlab.com/qovery/qovery-engine/-/jobs

At the end of the job, the image and tag will be shown. Replace the Terraform "qovery_engine_version" variable with the
image tag and push.

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

## Contribute

To active the debugger add the `RUST_LOG=qovery_engine=debug` env var 
# Qovery Engine

The Qovery Engine is an abstraction layer to deploy stateless and stateful applications on any Cloud providers.delete
It also bootstraps Kubernetes clusters and mandatory elements (network) for clients.

## Features
* TODO
* TODO

## Packages
### qovery-engine
TODO

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

## Supported connectors
### Build Platforms
TODO

### Container Registry
TODO

### Cloud Providers
TODO

## Tests

You can deploy a new cluster:
```shell script
RUST_LOG=info LIB_ROOT_DIR=~/qovery-engine/lib cargo test --package qovery-engine --test aws_kubernetes create_eks_cluster_in_us_east_2 -- --exact --nocapture
```

And deploy an application:
```shell script
RUST_LOG=info LIB_ROOT_DIR=~/qovery-engine/lib cargo test --package qovery-engine --test aws_environment deploy_a_working_development_environment_with_all_options_on_aws_eks -- --exact --nocapture
```

## Contribute

To active the debugger add the `RUST_LOG=qovery_engine=debug` env var 
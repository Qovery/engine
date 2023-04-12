#!/usr/bin/env bash

#set -x

awk=awk
sed=sed
grep=grep
if [ "$(uname)" == "Darwin" ] ; then
  grep='ggrep'
  awk='gawk'
  sed='gsed'
fi

trap "exit 1" 10
ARGS_NUM=$#
PROC="$$"
QOVERY_API="api.qovery.com"
QOVERY_ADMIN_API="api-admin.qovery.com"
TMP_LIB_DIR="/tmp/qovery-libs/"
ENGINE_DIR=lib-engine

#export AWS_DEFAULT_REGION="eu-west-3"
#export AWS_ACCESS_KEY_ID="$AWS_PROD_DEPLOY_ACCESS_KEY"
#export AWS_SECRET_ACCESS_KEY="$AWS_PROD_DEPLOY_SECRET_KEY"

export DOCKER_BUILDKIT=0
export GITLAB_LOG_UTILITIES_DIR="$CI_PROJECT_DIR/gitlab-log-utilities"
export GITLAB_LOG_OUTPUT_DIR="$CI_PROJECT_DIR/gitlab-log-utilities/output"
export LIB_ROOT_DIR=$(pwd)/$ENGINE_DIR/lib
export RUNNING_ON_CI=0
export ENGINE_BRANCH=""
export DEFAULT_ENGINE_IMAGE_NAME="qoveryrd/engine"

##################
# Main functions #
##################

function print_help() {
  echo "Usage: $0 <option>"
  $grep '##' $0 | $grep 'function' | $grep -v grep | $sed -r "s/^function\s(\w+).+##\s*(.+)/\1| \2/g" | $awk 'BEGIN {FS = "|"}; {printf "\033[36m%-30s\033[0m %s\n", $1, $2}' | sort
  exit 1
}

function fatal(){
  echo "$@" >&2
  kill -9 $PROC
}

function check_num_args() {
  desired_number=$1
  if [ $ARGS_NUM -ne ${desired_number} ]; then
    echo "Illegal number of parameters, required $desired_number"
    exit 1
  fi
}

function print_title() {
  title=$1
  echo "###################################################"
  echo "          $title"
  echo "###################################################"
}

#################
# Git functions #
#################

function check_untracked_files() {
  if [ $RUNNING_ON_CI -eq 0 ] ; then
    no_commit=1
    if [ $(git diff --exit-code | wc -l) -ne 0 ] ; then
      no_commit=0
    fi

    if [ $(git diff --cached --exit-code | wc -l) -ne 0 ] ; then
      no_commit=0
    fi

    if [ $(git ls-files --other --exclude-standard --directory | wc -l) -ne 0 ] ; then
      no_commit=0
    fi

    if [ $no_commit -eq 0 ] ; then
      echo "There are some untracked files changes by git. Ensure you've commited all your files first"
      git status
      exit 1
    fi
  fi
}

function get_gitlab_engine_commit_id() {
  # Ensure we're in the correct folder
  if [ "$(git config --get remote.origin.url | $grep -c "gitlab.com:qovery/backend/engine.git")" != "1" ] && [ -z $CI_REPOSITORY_URL ] ; then
    (fatal "You're not in the correct directory and should be in the gitlab repo: $(pwd)")
  fi
  git rev-parse HEAD
}

function generate_image_tag() {
  gitlab_commit_id=$(get_gitlab_engine_commit_id)
  echo "${gitlab_commit_id:0:7}"
}

#############################
# Build and image functions #
#############################

# shellcheck disable=SC2120
function build() { ## Build engine app with engine lib
  build_options=""
  if [ ! -z "$1" ] ; then
    build_options="$1"
  fi

  echo "Building with cargo options: $build_options"
  use_sccache
  set -e

  echo "=> Run app tests"
  cargo test $build_options --manifest-path app/Cargo.toml

  echo "=> Run build"
  cargo build $build_options --all-features --tests --color=always
  sccache -s
}

function set_release_ga() { ## Release a new engine version and mark it as globally available
  tag=$(generate_image_tag)
  curl -s -X PUT -H 'Content-Type: application/json' -H "X-Qovery-Signature: $CI_ENGINE_VERSION_CONTROLLER_TOKEN" "https://${QOVERY_ADMIN_API}/engine/serviceVersion?serviceType=ENGINE&version=${tag}" || exit 1
}

function get_release_ga() { ## Get globally available release version
  echo -e "Last defined GA version: "
  curl -s -H 'Content-Type: application/json' "https://${QOVERY_API}/engine/serviceVersion?serviceType=ENGINE"  || exit 1
}

function deploy_engines_infra() { ## Release GA to prod
  tag=$(generate_image_tag)
  AWS_ACCESS_KEY_ID="$AWS_PROD_DEPLOY_ACCESS_KEY" \
  AWS_SECRET_ACCESS_KEY="$AWS_PROD_DEPLOY_SECRET_KEY" \
  AWS_DEFAULT_REGION="$AWS_PROD_DEFAULT_REGION" \
  helm upgrade --kubeconfig="$AWS_PROD_KUBECONFIG" --install --history-max 50 --wait --timeout 3600s --namespace qovery-prod qovery-engine \
  $ENGINE_DIR/lib/common/bootstrap/charts/qovery-engine \
  --set image.tag="$tag",\
environmentVariables.CLOUD_PROVIDER="aws",\
environmentVariables.LIB_ROOT_DIR="/home/qovery/lib",\
environmentVariables.DOCKER_HOST="tcp://0.0.0.0:2375",\
environmentVariables.WORKSPACE_ROOT_DIR="/home/qovery",\
environmentVariables.DEPLOYMENT_TYPE="INFRASTRUCTURE",\
environmentVariables.VAULT_ADDR="$CI_VAULT_ADDR",\
environmentVariables.VAULT_ROLE_ID="$CI_VAULT_ENGINE_PROD_ROLE_ID",\
environmentVariables.VAULT_SECRET_ID="$CI_VAULT_ENGINE_PROD_SECRET_ID",\
environmentVariables.WORKSPACE_ROOT_DIR="/home/qovery",\
environmentVariables.GRPC_SERVER="https://engine.qovery.com:443",\
environmentVariables.ORGANIZATION_ID="51937012-8377-4e0f-84cf-7f5f38a0154b",\
environmentVariables.CLUSTER_ID="cb13209d-4e36-48b0-80e2-07e55c414b63",\
environmentVariables.CLUSTER_JWT_TOKEN="$INFRA_CLUSTER_JWT_TOKEN",\
buildContainer.enabled="false",\
metrics.enabled="true",\
terminationGracePeriodSeconds="14400",\
autoscaler.enabled="true",\
autoscaler.maxReplicas="30",\
autoscaler.minReplicas="1",\
autoscaler.averageValue="0.5",\
engineResources.limits.cpu="1",\
engineResources.limits.memory="750Mi",\
engineResources.requests.cpu="300m",\
engineResources.requests.memory="750Mi"
}

function deploy_engines_envs() { ## Release GA to prod
  tag=$(generate_image_tag)
  AWS_ACCESS_KEY_ID="$AWS_PROD_DEPLOY_ACCESS_KEY" \
  AWS_SECRET_ACCESS_KEY="$AWS_PROD_DEPLOY_SECRET_KEY" \
  AWS_DEFAULT_REGION="$AWS_PROD_DEFAULT_REGION" \
  helm upgrade --kubeconfig="$CI_KUBECONFIG_ENGINES_AWS" --install --create-namespace --history-max 50 --wait --timeout 3600s --namespace qovery-env qovery-engine \
  $ENGINE_DIR/lib/common/bootstrap/charts/qovery-engine \
  --set-string \
  --set image.tag="$tag",\
buildContainer.enabled="true",\
environmentVariables.BUILDER_KUBE_ENABLED='true',\
environmentVariables.BUILDER_CPU_ARCHITECTURES='AMD64\,ARM64',\
environmentVariables.BUILDER_CPU_REQUEST='3',\
environmentVariables.BUILDER_CPU_LIMIT='4',\
environmentVariables.BUILDER_MEMORY_REQUEST_GIB='6',\
environmentVariables.BUILDER_MEMORY_LIMIT_GIB='7',\
environmentVariables.LIB_ROOT_DIR="/home/qovery/lib",\
environmentVariables.DOCKER_HOST="tcp://0.0.0.0:2375",\
environmentVariables.WORKSPACE_ROOT_DIR="/home/qovery",\
environmentVariables.DEPLOYMENT_TYPE="ENVIRONMENT",\
environmentVariables.VAULT_ADDR="$CI_VAULT_ADDR",\
environmentVariables.VAULT_ROLE_ID="$CI_VAULT_ENGINE_PROD_ROLE_ID",\
environmentVariables.VAULT_SECRET_ID="$CI_VAULT_ENGINE_PROD_SECRET_ID",\
environmentVariables.GRPC_SERVER="https://engine.qovery.com:443",\
environmentVariables.ORGANIZATION_ID="51937012-8377-4e0f-84cf-7f5f38a0154b",\
environmentVariables.CLUSTER_ID="4ceb7649-ed84-4c52-a27b-e7fca06afaa5",\
environmentVariables.CLUSTER_JWT_TOKEN="$ENV_CLUSTER_JWT_TOKEN",\
metrics.enabled="true",\
autoscaler.enabled="true",\
autoscaler.minReplicas="2",\
autoscaler.maxReplicas="50",\
autoscaler.averageValue="0.9",\
overprovisionning.enabled="true",\
overprovisionning.replicas="9",\
engineResources.limits.cpu="1",\
engineResources.limits.memory="2Gi",\
engineResources.limits.ephemeral-storage="20Gi",\
engineResources.requests.cpu="300m",\
engineResources.requests.memory="2Gi",\
engineResources.requests.ephemeral-storage="20Gi"
}

## Tests

function prepare_tests() { ## Update all CHANGE-ME fields from lib-engine
  set -e

  print_title "Generating Vault Token"
  if [ ! -z $CI_VAULT_ADDR ] ; then
    export VAULT_ADDR=$CI_VAULT_ADDR
  else
    if [ -z $VAULT_ADDR ] ; then
      echo "VAULT_ADDR or CI_VAULT_ADDR were not found, can't continue"
      exit 1
    fi
  fi

  # if VAULT_TOKEN env var is already present, skip
  if [ -z $VAULT_TOKEN ] ; then
    export VAULT_TOKEN=$(vault write -format=json auth/approle/login role_id=$CI_VAULT_ROLE_ID secret_id=$CI_VAULT_SECRET_ID | jq -r ".auth.client_token")
  fi
}

function single_test() { ## Run a single test. Arg, test name: aws::aws_environment::deploy_a_working_environment_with_domain
  test_name=$1
  export RUST_LOG=info prepare_tests

  cargo build --color=always --all --all-targets --tests
  sccache -s
  cd $ENGINE_DIR
  cargo test --package qovery-engine --test lib $test_name -- --ignored --exact
}

function use_sccache() {
  if [ ! -z $DISABLE_SCCACHE ] && [ $DISABLE_SCCACHE -eq 1 ]; then
    echo "SCCACHE disabled"
    return
  fi
  export RUSTC_WRAPPER=/usr/bin/sccache
  if [ ! -z $CI_SCCACHE_REDIS ] ; then
    export SCCACHE_REDIS=$CI_SCCACHE_REDIS
  fi
  sccache --version
  sccache -s
}

function destroy_kube_cluster() {
    if [ -z $DOCKER_HOST ]; then unset $DOCKER_HOST; fi
    docker kill engine-registry
    k3d cluster delete $1
}

function test_local_stack() {
    prepare_tests
    use_sccache
    if [ -z $DOCKER_HOST ]; then unset $DOCKER_HOST; fi
    docker run -d --rm -p 5000:5000 --name engine-registry public.ecr.aws/r3m4q3r9/pub-mirror-registry:2.8.1

    kube_cluster_name="kube-test-cluster-$(date +%S%N)"

    # We can't use our ECR public repo yet :/
    k3d cluster create -a 0 \
        --image rancher/k3s:v1.23.17-k3s1 \
        --no-lb \
        --k3s-arg "--disable=traefik" \
        --wait $kube_cluster_name || k3d cluster start --wait $kube_cluster_name

    sleep 60
    kubectl -n kube-system wait pod --for=condition=Ready --selector app=local-path-provisioner || sleep 60
    kubectl -n kube-system wait pod --for=condition=Ready --selector app=local-path-provisioner

    echo "==========================TEST WITH LOCAL STACK==========================="
    trap "destroy_kube_cluster $kube_cluster_name" EXIT
    if [ -z $DOCKER_HOST ]; then unset $DOCKER_HOST; fi
    cargo test -j 2 --manifest-path lib-engine/Cargo.toml --features test-all-local
}

function run_tests(){ ## Run tests on qovery-engine. Args: cargo filter, GH branch name, threads
  filter_tests=$1
  nb_treads=$3
  print_title "RUNNING TESTS - $filter_tests"
  export RUST_LOG=debug
  prepare_tests
  use_sccache

  if [ $filter_tests = "unit-tests" ] ; then
   features_to_test_option="" # will execute only default features (unit tests)
  else
   features_to_test_option="--features $filter_tests --no-default-features"
  fi

  STARTTIME=$(date +%s)
  cd $ENGINE_DIR

  mkdir -p $GITLAB_LOG_OUTPUT_DIR
  touch $GITLAB_LOG_OUTPUT_DIR/tests.logs

  # Note: keep release, we don't waste time because of multiple cache and it drastically help to speed up prod build
  set -x
  cargo test $features_to_test_option --manifest-path Cargo.toml -- --color always --test-threads=$nb_treads -Z unstable-options --format json 2>&1 | tee $GITLAB_LOG_OUTPUT_DIR/output.log
  TESTS_STATUS="${PIPESTATUS[0]}"

  ENDTIME=$(date +%s)
  echo -e "\e[95mIt takes $(($ENDTIME - $STARTTIME)) seconds to complete cargo build and test..."
  # Log management part
  cd $GITLAB_LOG_UTILITIES_DIR
  STARTTIME=$(date +%s)
  # sorts logs into multiple files
  ./sorter.sh $GITLAB_LOG_OUTPUT_DIR/output.log
  # print failed tests
  ./print_tests_status.sh
  ENDTIME=$(date +%s)
  echo -e "\e[95mIt takes $(($ENDTIME - $STARTTIME)) seconds to complete sort and print failed tests"

  return $TESTS_STATUS
}

# shellcheck disable=SC2120
function lint() { ## Run rust linter
  nb_treads=$2
  export RUST_LOG=info
  use_sccache

  set -e

  print_title "CARGO FMT"
  cargo fmt --all -- --check --color=always  || (echo "Use cargo fmt to format your code"; exit 1)

  export RUSTC_WRAPPER=""
  export RUSTC_WORKSPACE_WRAPPER="sccache"
  cargo clippy --all --all-features --tests --locked -- -D warnings || (echo "Solve your clippy errors to succeed"; exit 1)
}

function unused_dependencies() { ## Check rust unused dependencies
  export RUST_LOG=info
  use_sccache

  set -e

  print_title "CARGO CHECK UNUSED DEPENDENCIES"
  # https://blog.benj.me/2022/04/27/cargo-machete/
  find . -name Cargo.toml ! -path '*/.cargo/*' -exec cargo machete {} \;
}

function await_docker() {
    if [ ! -z $DOCKER_HOST ] ; then
      return_code=1
      while [ $return_code -ne 0 ] ; do
      echo "waiting docker port 2375 to be available..."
      sleep 2
      nc -zv localhost 2375 2>/dev/null
      return_code=$?
      done
    fi
}


function update_engine_protobuf() {
  rm -rf /tmp/rust-backend
  git clone --depth 1 git@gitlab.com:qovery/backend/rust-backend.git /tmp/rust-backend
  cp /tmp/rust-backend/common/proto/engine.proto app/proto/
  rm -rf /tmp/rust-backend
}

function deploy_all_clusters() {
  token=$(curl -X POST -H 'Content-Type: application/json' --data-raw "{\"username\": \"qovery-admin\", \"password\": \"$CI_ADMIN_PASSWORD\"}" https://api-admin.qovery.com/auth)
  curl -X POST -H 'Content-Type: application/json' -H "Authorization: Bearer $token" --data-raw '{ "metadata" : { "dry_run_deploy": false } }' https://api-admin.qovery.com/cluster/deploy
}

function install_hook() { ## install git hook
  echo "$(pwd)/helper.sh lint" > .git/hooks/pre-commit
  chmod 755 $(pwd)/.git/hooks/pre-commit
}

# need to debug?
if [ ! -z $DEBUG_REQUIRED ] ; then
  echo "DEBUG MODE ENABLED FOR 1H"
  sleep 3600
fi

if [ $ARGS_NUM -eq 0 ] ; then
  print_help
fi

# Check if running manually
if [ ! -z $GITLAB_USER_ID ] ; then
  commit_id=$CI_COMMIT_SHA
  RUNNING_ON_CI=1
else
  commit_id="$(git rev-parse HEAD)"
  export GITLAB_LOG_UTILITIES_DIR="logs_output"
  export GITLAB_LOG_OUTPUT_DIR="logs_output"
fi
echo "Detected commit ID: $commit_id"

case $1 in
await_docker)
  await_docker
  ;;
build)
  build
  ;;
set_release_ga)
  set_release_ga
  ;;
# Deploy the engines dedicated for infra deployments
deploy_engines_infra)
  deploy_engines_infra
  ;;
# Deploy on the engines dedicated for customer's environments deployments
deploy_engines_envs)
  deploy_engines_envs
  ;;
get_release_ga)
  get_release_ga
  ;;
aws_self_hosted)
  run_tests test-aws-self-hosted $commit_id 20
  ;;
aws_ec2_self_hosted)
  run_tests test-aws-ec2-self-hosted $commit_id 1
  ;;
scw_self_hosted)
  run_tests test-scw-self-hosted $commit_id 20
  ;;
do_self_hosted)
  run_tests test-do-self-hosted $commit_id 20
  ;;
all_self_hosted)
  run_tests test-all-self-hosted $commit_id 20
  ;;
all_minimal_tests)
  run_tests test-all-minimal $commit_id 20
  ;;
aws_managed_services)
  run_tests test-aws-managed-services $commit_id 20
  ;;
aws_ec2_managed_services)
  run_tests test-aws-ec2-managed-services $commit_id 1
  ;;
scw_managed_services)
  run_tests test-scw-managed-services $commit_id 20
  ;;
do_managed_services)
  run_tests test-do-managed-services $commit_id 20
  ;;
all_managed_services)
  run_tests test-all-managed-services $commit_id 20
  ;;
aws_whole_enchilada)
  run_tests test-aws-whole-enchilada $commit_id 20
  ;;
aws_ec2_whole_enchilada)
  run_tests test-aws-whole-enchilada $commit_id 20
  ;;
scw_whole_enchilada)
  run_tests test-scw-whole-enchilada $commit_id 20
  ;;
do_whole_enchilada)
  run_tests test-do-whole-enchilada $commit_id 20
  ;;
aws_infra)
  run_tests test-aws-infra $commit_id 20
  ;;
aws_ec2_infra)
  run_tests test-aws-ec2-infra $commit_id 20
  ;;
scw_infra)
  run_tests test-scw-infra $commit_id 20
  ;;
do_infra)
  run_tests test-do-infra $commit_id 20
  ;;
test_all)
  run_tests test-all $commit_id 20
  ;;
unit_tests)
  run_tests unit-tests $commit_id 20
  ;;
single_test)
  check_num_args 2
  single_test $commit_id
  ;;
prepare_tests)
  prepare_tests
  ;;
lint)
  lint
  ;;
unused_dependencies)
  unused_dependencies
  ;;
install_hook)
  install_hook
  ;;
test_local_stack)
  test_local_stack "$2"
  ;;
deploy_all_clusters)
  deploy_all_clusters
  ;;
update_engine_protobuf)
  update_engine_protobuf
  ;;
*)
  print_help
  ;;
esac

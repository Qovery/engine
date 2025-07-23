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
QOVERY_ADMIN_DEV_API="api-admin-dev.qovery.com"
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

function generate_image_tag() {
  # Ensure we're in the correct folder
  if [ "$(git config --get remote.origin.url | $grep -c "gitlab.com:qovery/backend/engine.git")" != "1" ] && [ -z $CI_REPOSITORY_URL ] ; then
    (fatal "You're not in the correct directory and should be in the gitlab repo: $(pwd)")
  fi

  git describe --exact-match --tags
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

function update_engine_version() {
  set -e
  echo "Updating Rust Engine version to $CI_COMMIT_TAG"
  sed -i "s/0.0.0/$CI_COMMIT_TAG/g" lib-engine/Cargo.toml
  echo "Updating Engine Helm chart version to $CI_COMMIT_TAG"
  sed -i "s/0.0.0/$CI_COMMIT_TAG/g" lib-engine/lib/common/bootstrap/charts/qovery-engine/Chart.yaml
}

function set_release_ga() { ## Release a new engine version and mark it as globally available
  tag=$(generate_image_tag)
  curl -s -X PUT -H 'Content-Type: application/json' -H "X-Qovery-Signature: $CI_ENGINE_VERSION_CONTROLLER_TOKEN" "https://${QOVERY_ADMIN_API}/engine/serviceVersion?serviceType=ENGINE&version=${tag}" || exit 1
  curl -s -X PUT -H 'Content-Type: application/json' -H "X-Qovery-Signature: $CI_ENGINE_VERSION_CONTROLLER_TOKEN" "https://${QOVERY_ADMIN_DEV_API}/engine/serviceVersion?serviceType=ENGINE&version=${tag}" || exit 1
}

function get_release_ga() { ## Get globally available release version
  echo -e "Last defined GA version: "
  curl -s -H 'Content-Type: application/json' "https://${QOVERY_API}/engine/serviceVersion?serviceType=ENGINE"  || exit 1
}

function deploy_engines_infra_static_ip() { ## Release GA to prod
  tag=$(generate_image_tag)
  case $1 in
    "prod")
      name="qovery-engine-infra"
      jwt_token="$INFRA_CLUSTER_INFRA_STATIC_IP_JWT_TOKEN"
      ;;
    "staging")
      name="qovery-engine-infra-staging"
      jwt_token="$INFRA_STAGING_CLUSTER_INFRA_STATIC_IP_JWT_TOKEN"
      ;;
    *)
      # it doesn't exists but can be useful for tests
      name="qovery-engine-infra-dev"
      jwt_token="$INFRA_DEV_CLUSTER_INFRA_STATIC_IP_JWT_TOKEN"
      ;;
  esac

  AWS_ACCESS_KEY_ID="$AWS_PROD_INFRA_STATIC_IP_DEPLOY_ACCESS_KEY" \
  AWS_SECRET_ACCESS_KEY="$AWS_PROD_INFRA_STATIC_IP_DEPLOY_SECRET_KEY" \
  AWS_DEFAULT_REGION="$AWS_PROD_INFRA_STATIC_IP_DEFAULT_REGION" \
  helm upgrade --kubeconfig="$AWS_PROD_INFRA_STATIC_IP_KUBECONFIG" --install --create-namespace --history-max 50 --wait --timeout 3600s --namespace $name qovery-engine \
  $ENGINE_DIR/lib/common/bootstrap/charts/qovery-engine \
  --set image.tag="$tag",\
fullnameOverride="$name",\
environmentVariables.CLOUD_PROVIDER="aws",\
environmentVariables.LIB_ROOT_DIR="/home/qovery/lib",\
environmentVariables.DOCKER_HOST="tcp://0.0.0.0:2375",\
environmentVariables.WORKSPACE_ROOT_DIR="/home/qovery",\
environmentVariables.DEPLOYMENT_TYPE="INFRASTRUCTURE",\
environmentVariables.WORKSPACE_ROOT_DIR="/home/qovery",\
environmentVariables.GRPC_SERVER="https://engine.qovery.com:443",\
environmentVariables.ORGANIZATION_ID="51937012-8377-4e0f-84cf-7f5f38a0154b",\
environmentVariables.CLUSTER_ID="6d9f665a-c203-4b02-8d49-ee05ad3f1137",\
environmentVariables.CLUSTER_JWT_TOKEN="$jwt_token",\
rbac.clusterPermission="none",\
buildContainer.enabled="false",\
metrics.enabled="true",\
terminationGracePeriodSeconds="14400",\
autoscaler.enabled="true",\
autoscaler.maxReplicas="50",\
autoscaler.minReplicas="1",\
autoscaler.averageValue="0.5",\
engineResources.limits.cpu="1",\
engineResources.limits.memory="1Gi",\
engineResources.requests.cpu="300m",\
engineResources.requests.memory="1Gi"
}

function deploy_engines_environment_static_ip() { ## Release GA to prod
  tag=$(generate_image_tag)
  AWS_ACCESS_KEY_ID="$AWS_PROD_ENVIRONMENT_STATIC_IP_DEPLOY_ACCESS_KEY" \
  AWS_SECRET_ACCESS_KEY="$AWS_PROD_ENVIRONMENT_STATIC_IP_DEPLOY_SECRET_KEY" \
  AWS_DEFAULT_REGION="$AWS_PROD_ENVIRONMENT_STATIC_IP_DEFAULT_REGION" \
  helm upgrade --kubeconfig="$AWS_PROD_ENVIRONMENT_STATIC_IP_KUBECONFIG" --install --create-namespace --history-max 50 --wait --timeout 3600s --namespace qovery-env qovery-engine \
  $ENGINE_DIR/lib/common/bootstrap/charts/qovery-engine \
  --set-string \
image.tag="$tag",\
buildContainer.enabled="true",\
buildContainer.environmentVariables.BUILDER_KUBE_ENABLED="true",\
buildContainer.environmentVariables.BUILDER_CPU_ARCHITECTURES="AMD64\,ARM64",\
buildContainer.environmentVariables.BUILDER_ROOTLESS_ENABLED="false",\
environmentVariables.LIB_ROOT_DIR="/home/qovery/lib",\
environmentVariables.DOCKER_HOST="tcp://0.0.0.0:2375",\
environmentVariables.WORKSPACE_ROOT_DIR="/home/qovery",\
environmentVariables.DEPLOYMENT_TYPE="ENVIRONMENT",\
environmentVariables.GRPC_SERVER="https://engine.qovery.com:443",\
environmentVariables.ORGANIZATION_ID="51937012-8377-4e0f-84cf-7f5f38a0154b",\
environmentVariables.CLUSTER_ID="8a14ee85-a66b-46f7-83e3-5cbeb4d5ed8b",\
environmentVariables.CLUSTER_JWT_TOKEN="$ENVIRONMENT_CLUSTER_STATIC_IP_JWT_TOKEN",\
networkPolicies.enabled="true",\
metrics.enabled="true",\
rbac.clusterPermission="deployer",\
autoscaler.enabled="true",\
autoscaler.minReplicas="2",\
autoscaler.maxReplicas="50",\
autoscaler.averageValue="0.9",\
overprovisionning.enabled="true",\
overprovisionning.replicas="5",\
overprovisionning.resources.requests.cpu="4",\
overprovisionning.resources.limits.cpu="4",\
overprovisionning.resources.requests.memory="8Gi",\
overprovisionning.resources.limits.memory="8Gi",\
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

  echo "SCCACHE enabled"
  export RUSTC_WRAPPER=/usr/bin/sccache
  if [ ! -z $CI_SCCACHE_REDIS_ENDPOINT ] ; then
    export SCCACHE_REDIS_ENDPOINT=$CI_SCCACHE_REDIS_ENDPOINT
    export SCCACHE_REDIS_USERNAME=default
    export SCCACHE_REDIS_PASSWORD=$CI_SCCACHE_REDIS_PASSWORD
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
    docker run -d --rm -p 5000:5000 --name engine-registry -e REGISTRY_STORAGE_DELETE_ENABLED=true public.ecr.aws/r3m4q3r9/pub-mirror-registry:2.8.1

    kube_cluster_name="kube-test-cluster-$(date +%S%N)"

    # We can't use our ECR public repo yet :/
    k3d cluster create -a 0 \
        --agents 0 \
        --image rancher/k3s:v1.32.6-k3s1 \
        --no-lb \
        --k3s-arg "--disable=traefik" \
        --wait $kube_cluster_name || k3d cluster start --wait $kube_cluster_name

    sleep 60
    kubectl -n kube-system wait pod --for=condition=Ready --selector app=local-path-provisioner || sleep 60
    kubectl -n kube-system wait pod --for=condition=Ready --selector app=local-path-provisioner

    echo "==========================TEST WITH LOCAL STACK==========================="
    trap "destroy_kube_cluster $kube_cluster_name" EXIT
    if [ -z $DOCKER_HOST ]; then unset $DOCKER_HOST; fi

    mkdir -p "$GITLAB_LOG_OUTPUT_DIR"

    # Note: keep release, we don't waste time because of multiple cache and it drastically help to speed up prod build
    # set -x
    # Removing from Gitlab pipeline output non ERROR / WARN logs avoiding to have output over 100MB has it's the limit
    # and we cannot increase it properly for the time being CF: https://docs.gitlab.com/ee/administration/instance_limits.html#maximum-file-size-for-job-logs
    output_log_file="$GITLAB_LOG_OUTPUT_DIR/${filter_tests}.log"
    touch "$output_log_file"
    tail -f "$output_log_file" &

    # Using nextest to run tests (mainly because its compatibility with junit)
    # https://nexte.st/docs/machine-readable/junit/
    cargo nextest run --lib --tests --features test-all-local --no-default-features --message-format human --manifest-path Cargo.toml --tool-config-file ci:"$(pwd)"/lib-engine/nextest.config.toml --no-fail-fast --profile default --no-tests=pass -- >>"$output_log_file" 2>&1
}

function run_tests(){ ## Run tests on qovery-engine. Args: cargo filter, GH branch name, threads
  filter_tests=$1
  nb_treads=$3
  print_title "RUNNING TESTS - $filter_tests"
  export RUST_LOG=info
  prepare_tests
  use_sccache

  if [ "$filter_tests" = "unit-tests" ]; then
    # will execute only default features (unit tests)
    features_to_test_option=(
      --lib
      --bins
    )
  else
    # will execute only the features specified form tests/ folder (integration tests)
    features_to_test_option=(
      --lib
      --tests
      -E 'kind(test)'
      --features "$filter_tests"
      --no-default-features
    )
  fi

  STARTTIME=$(date +%s)
  cd $ENGINE_DIR

  mkdir -p "$GITLAB_LOG_OUTPUT_DIR"

  # Note: keep release, we don't waste time because of multiple cache and it drastically help to speed up prod build
  # set -x
  # Removing from Gitlab pipeline output non ERROR / WARN logs avoiding to have output over 100MB has it's the limit
  # and we cannot increase it properly for the time being CF: https://docs.gitlab.com/ee/administration/instance_limits.html#maximum-file-size-for-job-logs
  output_log_file="$GITLAB_LOG_OUTPUT_DIR/${filter_tests}.log"
  touch "$output_log_file"
  tail -f "$output_log_file" &

  # Using nextest to run tests (mainly because its compatibility with junit)
  # https://nexte.st/docs/machine-readable/junit/
  cargo nextest run "${features_to_test_option[@]}" --message-format human --manifest-path Cargo.toml --tool-config-file ci:"$(pwd)"/nextest.config.toml --no-fail-fast --profile default --no-tests=pass -- >>"$output_log_file" 2>&1
  TESTS_STATUS="${PIPESTATUS[0]}"
  echo "Test status: $TESTS_STATUS"

  ENDTIME=$(date +%s)
  echo -e "\e[95mIt takes $((ENDTIME - STARTTIME)) seconds to complete cargo build and test..."

  ENDTIME=$(date +%s)
  echo -e "\e[95mIt takes $((ENDTIME - STARTTIME)) seconds to complete sort and print failed tests"

  return "$TESTS_STATUS"
}

function cargo_version() {
  print_title "CARGO VERSION"
  cargo --version
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

function update_qovery_chart() {
  prepare_tests
  use_sccache
  set -e

  apt-get update && apt-get install -y git
  mkdir /root/.ssh && chmod 700 /root/.ssh

  echo $SSH_PRIVATE_KEY_GITHUB_HELM_CHART | base64 -d > /root/.ssh/id_ed25519 && chmod 600 /root/.ssh/id_ed25519
  echo 'Host *' > /root/.ssh/config
  echo '  HashKnownHosts no' >> /root/.ssh/config
  echo '  StrictHostKeyChecking no' >> /root/.ssh/config
  echo '  UserKnownHostsFile /dev/null' >> /root/.ssh/config

  git clone git@github.com:Qovery/qovery-chart.git qovery-chart
  # generate chart
  WORKSPACE_ROOT_DIR=/builds/qovery/backend/engine/lib-engine LIB_ROOT_DIR=$WORKSPACE_ROOT_DIR/lib cargo test --package qovery-engine --lib --all-features -- byok_chart_gen::tests::generate_helm_chart --exact --nocapture --ignored
  # copy chart to github chart repo
  set -x
  rm -Rf qovery-chart/charts
  mkdir -p qovery-chart/charts/qovery
  cp -Rf lib-engine/.qovery-workspace/qovery_chart/* qovery-chart/charts/qovery
  cd qovery-chart
  #test $(git diff --shortstat | wc -l) -eq 0 && exit 0
  git config --global user.email "noreply@qovery.com"
  git config --global user.name "Qovery"
  git add .
  git status

  # Check if a commit & push is necessary
  if ! git diff-index --quiet HEAD; then
    current_date=$(date "+%x %X") && git commit -a -m "update $current_date"
    git status
    # push to github
    git push
  else
    echo "Nothing to push"
  fi
}

function update_engine_protobuf() {
  rm -rf /tmp/rust-backend
  git clone --depth 1 git@gitlab.com:qovery/backend/rust-backend.git /tmp/rust-backend
  cp /tmp/rust-backend/common/proto/engine.proto app/proto/
  rm -rf /tmp/rust-backend
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

# for channels if specified
case $2 in 
  "prod")
    channel="prod"
    ;;
  "staging")
    channel="staging"
    ;;
  *)
    channel="dev"
    ;;
esac

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
# Deploy the engines dedicated for infra deployments on cluster with static ip
deploy_engines_infra_static_ip)
  deploy_engines_infra_static_ip $channel
  ;;
# Deploy on the engines dedicated for customer's environments deployments on cluster with static ip
deploy_engines_environment_static_ip)
  deploy_engines_environment_static_ip
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
azure_self_hosted)
  run_tests test-azure-self-hosted $commit_id 20
  ;;
scw_self_hosted)
  run_tests test-scw-self-hosted $commit_id 20
  ;;
gcp_self_hosted)
  run_tests test-gcp-self-hosted $commit_id 20
  ;;
all_self_hosted)
  run_tests test-all-self-hosted $commit_id 20
  ;;
aws_minimal_tests)
  run_tests test-aws-minimal $commit_id 20
  ;;
aws_ec2_minimal_tests)
  run_tests test-aws-ec2-minimal $commit_id 20
  ;;
azure_minimal_tests)
  run_tests test-azure-minimal $commit_id 20
  ;;
gcp_minimal_tests)
  run_tests test-gcp-minimal $commit_id 20
  ;;
scw_minimal_tests)
  run_tests test-scw-minimal $commit_id 20
  ;;
aws_managed_services)
  run_tests test-aws-managed-services $commit_id 20
  ;;
aws_ec2_managed_services)
  run_tests test-aws-ec2-managed-services $commit_id 1
  ;;
azure_managed_services)
  run_tests test-azure-managed-services $commit_id 20
  ;;
scw_managed_services)
  run_tests test-scw-managed-services $commit_id 20
  ;;
gcp_managed_services)
  run_tests test-gcp-managed-services $commit_id 20
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
azure_whole_enchilada)
  run_tests test-azure-whole-enchilada $commit_id 20
  ;;
scw_whole_enchilada)
  run_tests test-scw-whole-enchilada $commit_id 20
  ;;
gcp_whole_enchilada)
  run_tests test-gcp-whole-enchilada $commit_id 20
  ;;
aws_infra)
  run_tests test-aws-infra $commit_id 20
  ;;
aws_infra_arm)
  run_tests test-aws-infra-arm $commit_id 20
  ;;
aws_infra_karpenter)
  run_tests test-aws-infra-karpenter $commit_id 20
  ;;
aws_infra_nat_gateway)
  run_tests test-aws-infra-nat-gateway $commit_id 20
  ;;
aws_infra_upgrade)
  run_tests test-aws-infra-upgrade $commit_id 20
  ;;
aws_ec2_infra)
  run_tests test-aws-ec2-infra $commit_id 20
  ;;
azure_infra)
  run_tests test-azure-infra $commit_id 20
  ;;
azure_infra_upgrade)
  run_tests test-azure-infra-upgrade $commit_id 20
  ;;
aws_ec2_infra_upgrade)
  run_tests test-aws-ec2-infra-upgrade $commit_id 20
  ;;
scw_infra)
  run_tests test-scw-infra $commit_id 20
  ;;
scw_infra_upgrade)
  run_tests test-scw-infra-upgrade $commit_id 20
  ;;
gcp_infra)
  run_tests test-gcp-infra $commit_id 20
  ;;
gcp_infra_upgrade)
  run_tests test-gcp-infra-upgrade $commit_id 20
  ;;
quarantine)
  run_tests test-quarantine $commit_id 20
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
cargo_version)
  cargo_version
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
update_engine_protobuf)
  update_engine_protobuf
  ;;
update_engine_version)
  update_engine_version
  ;;
update_qovery_chart)
  update_qovery_chart
  ;;
*)
  print_help
  ;;
esac

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

## Main functions

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

## Git functions

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

## Build and image functions

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

function build_image() { ## Build Engine image locally. Args: <tag_version>
  tag=$(generate_image_tag)

  cp docker/load.sh docker/engine/load.sh
  cp docker/bin_versions bin_versions
  # copy providers files to download required binaries
  rm -Rf docker/engine/providers/*
  set -e
  for i in $(find lib-engine/lib -name "tf-providers*") ; do
    provider=$(echo $i | sed -r 's/.+\/(.+)(\/.+){2}.tf/\1/')
    mkdir -p docker/engine/providers/$provider
    cp $i docker/engine/providers/$provider/
    $sed -ri 's/\{\{.+\}\}/flushed/g' docker/engine/providers/$provider/*
  done

  set +e
  await_docker
  set -e

  export DOCKER_BUILDKIT=1
  export SCCACHE_ARGS=""
  if [ ! -z $CI_SCCACHE_REDIS ] ; then
    SCCACHE_ARGS="--build-arg SCCACHE_REDIS=$CI_SCCACHE_REDIS"
  else
    echo "-> SCCACHE will not use Redis because CI_SCCACHE_REDIS isn't set!!!"
  fi

  # disable sccache?
  RUSTC="/usr/bin/sccache"
  if [ ! -z $DISABLE_SCCACHE ] && [ $DISABLE_SCCACHE -eq 1 ]; then
    RUSTC="/usr/bin/rustc"
  fi
  RUSTC_WRAPPER="--build-arg RUSTC_WRAPPER=$RUSTC"
  docker build --network "host" $RUSTC_WRAPPER $SCCACHE_ARGS -t ${DEFAULT_ENGINE_IMAGE_NAME}:${tag} .

  rm -f docker/engine/load.sh
  rm -f bin_versions
  rm -Rf docker/engine/providers/*
}

function build_ci_image() { ## Build CI image locally. Args: <tag_version>
  tag=$(generate_image_tag)

  cp docker/load.sh docker/ci/load.sh
  cp docker/bin_versions docker/ci/bin_versions

  cd docker/ci
  export DOCKER_BUILDKIT=1
  docker build --network "host" --build-arg SCCACHE_REDIS=$CI_SCCACHE_REDIS --no-cache -t public.ecr.aws/r3m4q3r9/qovery-ci:${tag} .
  cd -

  rm -f docker/ci/load.sh
  rm -f docker/ci/bin_versions
}

function push_image() { ## Push Engine local image with current commit ID as tag
  tag=$(generate_image_tag)
  set -e

  docker login -u $DOCKER_LOGIN -p $DOCKER_TOKEN
  docker push ${DEFAULT_ENGINE_IMAGE_NAME}:${tag}
}

function push_ci_image() { ## Push CI local image with current commit ID as tag
  tag=$(generate_image_tag)
  set -e

  aws ecr-public get-login-password --region us-east-1 | docker login --username AWS --password-stdin public.ecr.aws
  docker push public.ecr.aws/r3m4q3r9/qovery-ci:${tag}
}

## Releases

function new_release() { ## Release a new engine version with commit ID as tag prepare_engine
  tag=$(generate_image_tag)

  check_untracked_files
  build_image
  push_image

  echo -e "\e[92mNew image name is: ${DEFAULT_ENGINE_IMAGE_NAME}:${tag}\e[0m"
}
function prod_release() { ## Release a new engine version with commit ID as tag prepare_engine
  set -e
  git_tag=$(generate_image_tag)
  archive_dir="./engine_$git_tag"
  dest_folder="dist/engine_linux_amd64"

  check_untracked_files
  build_image

  # create an archive from the required files
  # 1. get content from docker image
  container_id=$(docker create ${DEFAULT_ENGINE_IMAGE_NAME}:${git_tag})
  test -d $archive_dir && rm -Rf $archive_dir
  docker cp -a $container_id:/home/qovery $archive_dir
  docker rm "$container_id"
  # 2. clean cache and uneeded data
  rm -Rf $archive_dir/.* || echo "clean done"
  # 3. generate archive name
  mkdir -p $dest_folder
  tar -czf $dest_folder/engine.tgz $archive_dir
  git tag v1.0-$git_tag
  # 4. get goreleaser if not exists
  if [ $(which goreleaser) ] ; then
    curl -Lso /tmp/goreleaser.tgz https://github.com/goreleaser/goreleaser/releases/download/v1.9.2/goreleaser_Linux_x86_64.tar.gz
    tar -C /usr/bin/ -xzf goreleaser.tgz goreleaser
  fi
 
  goreleaser release --rm-dist
  git tag -d v1.0-$git_tag

  echo -e "\e[92mNew image name is: ${DEFAULT_ENGINE_IMAGE_NAME}:${git_tag}\e[0m"
}

function set_release_ga() { ## Release a new engine version and mark it as globally available
  tag=$(generate_image_tag)
  curl -s -X PUT -H 'Content-Type: application/json' -H "X-Qovery-Signature: $CI_ENGINE_VERSION_CONTROLLER_TOKEN" "https://${QOVERY_API}/api/v1/engine-version?type=ga&version=${tag}" || exit 1
}

function get_release_ga() { ## Get globally available release version
  echo -e "Last defined GA version: "
  curl -s -H 'Content-Type: application/json' -H "X-Qovery-Signature: $CI_ENGINE_VERSION_CONTROLLER_TOKEN" "https://${QOVERY_API}/api/v1/engine-version?type=ga"  || exit 1
}

function deploy_engines_infra() { ## Release GA to prod
  tag=$(generate_image_tag)
  AWS_ACCESS_KEY_ID="$AWS_PROD_DEPLOY_ACCESS_KEY" \
  AWS_SECRET_ACCESS_KEY="$AWS_PROD_DEPLOY_SECRET_KEY" \
  AWS_DEFAULT_REGION="$AWS_PROD_DEFAULT_REGION" \
  helm upgrade --kubeconfig="$AWS_PROD_KUBECONFIG" --install --history-max 50 --wait --timeout 3600s --namespace qovery-prod qovery-engine \
  $ENGINE_DIR/lib/common/bootstrap/charts/qovery-engine \
  --set image.tag="$tag",\
environmentVariables.QOVERY_NATS_URL="tls://nats-internal.qovery.io:4222",\
environmentVariables.QOVERY_NATS_USER="$CI_QOVERY_NATS_USER",\
environmentVariables.QOVERY_NATS_PASSWORD="$CI_QOVERY_ENGINE_NATS_INTERNAL_PASSWORD_ENGINE",\
environmentVariables.CLOUD_PROVIDER="aws",\
environmentVariables.LIB_ROOT_DIR="/home/qovery/lib",\
environmentVariables.DOCKER_HOST="tcp://0.0.0.0:2375",\
environmentVariables.WORKSPACE_ROOT_DIR="/home/qovery",\
environmentVariables.VAULT_ADDR="$CI_VAULT_ADDR",\
environmentVariables.VAULT_ROLE_ID="$CI_VAULT_ENGINE_PROD_ROLE_ID",\
environmentVariables.VAULT_SECRET_ID="$CI_VAULT_ENGINE_PROD_SECRET_ID",\
environmentVariables.WORKSPACE_ROOT_DIR="/home/qovery",\
buildContainer.enable="false",\
metrics.enabled="true",\
terminationGracePeriodSeconds="14400",\
autoscaler.enabled="true",\
autoscaler.max_replicas="30",\
autoscaler.min_replicas="1",\
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
  --set image.tag="$tag",\
environmentVariables.QOVERY_NATS_URL="tls://nats-external.qovery.com:4242",\
environmentVariables.QOVERY_NATS_USER="$CI_QOVERY_NATS_EXTERNAL_USER",\
environmentVariables.QOVERY_NATS_PASSWORD="$CI_QOVERY_NATS_EXTERNAL_PASSWORD",\
environmentVariables.LIB_ROOT_DIR="/home/qovery/lib",\
environmentVariables.DOCKER_HOST="tcp://0.0.0.0:2375",\
environmentVariables.WORKSPACE_ROOT_DIR="/home/qovery",\
environmentVariables.DEPLOYMENT_TYPE="ENVIRONMENT",\
environmentVariables.VAULT_ADDR="$CI_VAULT_ADDR",\
environmentVariables.VAULT_ROLE_ID="$CI_VAULT_ENGINE_PROD_ROLE_ID",\
environmentVariables.VAULT_SECRET_ID="$CI_VAULT_ENGINE_PROD_SECRET_ID",\
buildContainer.enable="true",\
volumes.useNetworkDisks="false",\
metrics.enable="true",\
autoscaler.enabled="false",\
autoscaler.min_replicas="15",\
engineResources.limits.cpu="1",\
engineResources.limits.memory="4Gi",\
engineResources.limits.ephemeral-storage="20Gi",\
engineResources.requests.cpu="300m",\
engineResources.requests.memory="4Gi",\
engineResources.requests.ephemeral-storage="20Gi",\
buildResources.requests.ephemeral-storage="30Gi",\
buildResources.limits.ephemeral-storage="30Gi"
}

function upgrade_on_dev() {
  tag=$(generate_image_tag)
  kubectl --kubeconfig=$DO_DEV_KUBECONFIG -n qovery patch statefulset qovery-engine -p "{'spec':{'template':{'spec':{'containers[0]':{'image':'${DEFAULT_ENGINE_IMAGE_NAME}:${tag}'}}}}}"
  kubectl --kubeconfig=$DO_DEV_KUBECONFIG rollout status --watch --timeout=600s -n qovery statefulset/qovery-engine
}

function downgrade_on_dev() {
  if [ "$1" == "" ] ; then
    return
  fi

  kubectl --kubeconfig=$DO_DEV_KUBECONFIG -n qovery patch statefulset qovery-engine -p "{'spec':{'template':{'spec':{'containers[0]':{'image':'${DEFAULT_ENGINE_IMAGE_NAME}:$1'}}}}}"
  kubectl --kubeconfig=$DO_DEV_KUBECONFIG rollout status --watch --timeout=600s -n qovery statefulset/qovery-engine
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
    docker run -d --rm -p 5000:5000 --name engine-registry registry:2

    kube_cluster_name="kube-test-cluster-$(date +%S%N)"

    k3d cluster create -a 0 \
        --image rancher/k3s:v1.21.10-k3s1 \
        --no-lb \
        --k3s-arg "--disable=traefik" \
        --wait $kube_cluster_name || k3d cluster start --wait $kube_cluster_name

    sleep 30
    kubectl -n kube-system wait pod --for=condition=Ready --selector app=local-path-provisioner || sleep 30
    kubectl -n kube-system wait pod --for=condition=Ready --selector app=local-path-provisioner

    echo "==========================TEST WITH LOCAL STACK==========================="
    trap "destroy_kube_cluster $kube_cluster_name" EXIT
    if [ -z $DOCKER_HOST ]; then unset $DOCKER_HOST; fi
    cargo test --manifest-path lib-engine/Cargo.toml --features test-all-local
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
function lint() {
  nb_treads=$2
  export RUST_LOG=info
  use_sccache

  set -e

  print_title "CARGO FMT"
  cargo fmt --all -- --check --color=always  || (echo "Use cargo fmt to format your code"; exit 1)

  export RUSTC_WRAPPER=""
  export RUSTC_WORKSPACE_WRAPPER="sccache"
  cargo clippy  --all --all-features --exclude test-utilities --locked -- -D warnings || (echo "Solve your clippy errors to succeed"; exit 1)
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
build_image)
  build_image
  ;;
build_ci_image)
  build_ci_image
  ;;
new_release)
  new_release
  ;;
prod_release)
  prod_release
  ;;
push_image)
  push_image
  ;;
push_ci_image)
  push_ci_image
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
  run_tests test-aws-ec2-self-hosted $commit_id 20
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
  run_tests test-aws-ec2-managed-services $commit_id 20
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
install_hook)
  install_hook
  ;;
test_local_stack)
  test_local_stack "$2"
;;
upgrade_on_dev)
  upgrade_on_dev
  ;;
downgrade_on_dev)
  downgrade_on_dev "$2"
  ;;
deploy_all_clusters)
  deploy_all_clusters
  ;;
*)
  print_help
  ;;
esac

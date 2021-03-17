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
ENGINE_DIR=cloned-engine

export DOCKER_BUILDKIT=0
export GITLAB_LOG_UTILITIES_DIR="/builds/qovery/qovery-engine/gitlab-log-utilities"
export GITLAB_LOG_OUTPUT_DIR="/builds/qovery/qovery-engine/gitlab-log-utilities/output"
export LIB_ROOT_DIR=$(pwd)/$ENGINE_DIR/lib
export RUNNING_ON_CI=0
export ENGINE_BRANCH=""

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
      echo "There are some untracked files changes by git. Ensure you've commited all your files first"chefclub.tv
      git status
      exit 1
    fi
  fi
}

function get_gitlab_engine_commit_id() {
  # Ensure we're in the correct folder
  if [ "$(git config --get remote.origin.url | $grep -c "gitlab.com:qovery/qovery-engine.git")" != "1" ] && [ -z $CI_REPOSITORY_URL ] ; then
    (fatal "You're not in the correct directory and should be in the gitlab repo: $(pwd)")
  fi
  git rev-parse HEAD
}

function get_github_engine_commit_id() {
  # Ensure we're in the correct folder
  if [ $(git config --get remote.origin.url | $grep "Qovery/engine.git" | $grep -c github) -ne 1 ] ; then
    (fatal "You're not in the correct directory and should be in the github repo: $(pwd)")
  fi
  git rev-parse HEAD
}

function generate_image_tag() {
  gitlab_commit_id=$(get_gitlab_engine_commit_id)

  current_dir=$(pwd)
  cd $ENGINE_DIR
  github_commit_id=$(get_github_engine_commit_id)
  cd $current_dir

  echo "${github_commit_id:0:7}-${gitlab_commit_id:0:7}"
}

function prepare_engine() { ## Ensure github engine repo is present and propose solutions if not
    if [ $RUNNING_ON_CI -eq 1 ] ; then
      # If commit id is given, then use it for the lib
      if [ ! -z $GITHUB_COMMIT_ID ] ; then
        ENGINE_BRANCH=$GITHUB_COMMIT_ID
      elif [ ! -z $CI_COMMIT_REF_NAME ] ; then
        ENGINE_BRANCH=$CI_COMMIT_REF_NAME
      else
        print_title "Can't get commit ID"
        exit 1
      fi
      # For the app, checkout on the same branch name gitlab <-> github if the same name exists
      if [ "$GITHUB_ENGINE_BRANCH_NAME" == "true" ] ; then
        echo "Requested to checkout the $ENGINE_BRANCH instead of dev branch"
        git checkout $CI_COMMIT_REF_SLUG
      fi
    else
      ENGINE_BRANCH=$(git branch --show-current)
    fi
    print_title "USING QOVERY APP COMMIT"
    git log -1

    if [ -e $ENGINE_DIR ] ; then
      print_title "USING QOVERY LIB COMMIT"
      echo "Found $ENGINE_DIR directory, going to use it"
    elif [ ! -d $ENGINE_DIR ] ; then
      if [ $RUNNING_ON_CI -eq 0 ] ; then
        echo "'cloned-engine' folder is missing. To get it, you can:"
        echo "1. Clone the engine from the engine repo: git clone https://github.com/Qovery/engine.git $ENGINE_DIR"
        echo "2. Make a symlink from your current engine version (WARN, file updates can occur)"
        echo ""
        echo "Hit any key to continue or CTRL+C to stop"
        read
      else
        git clone https://github.com/Qovery/engine.git $ENGINE_DIR
        cd $ENGINE_DIR
        git checkout $ENGINE_BRANCH
        git pull
        print_title "USING QOVERY LIB COMMIT"
        git log -1
        cd -
      fi
    fi

    if [ ! -e $ENGINE_DIR ] ; then
      echo "Engine directory $ENGINE_DIR wasn't found"
      exit 1
    fi

    cd $ENGINE_DIR
    echo "Latest commit on branch $ENGINE_BRANCH:"
    git log -1
    cd -
}

# shellcheck disable=SC2120
function build() { ## Build engine app with engine lib
  build_options=""
  if [ ! -z "$1" ] ; then
    build_options="$1"
  fi

  echo "Building with cargo options: $build_options"
  prepare_engine
  tag=$(generate_image_tag)
  use_sccache
  set -e

  echo "=> Run task-manager tests"
  cargo test $build_options --manifest-path qovery-engine-task-manager/Cargo.toml

  echo "=> Run app tests"
  cargo test $build_options --manifest-path app/Cargo.toml

  echo "=> Run build"
  cargo build $build_options --all-features --color=always
  sccache -s
}

function build_image() { ## Build Engine image locally. Args: <tag_version>
  tag=$(generate_image_tag)

  cp docker/load.sh docker/engine/load.sh
  cp docker/bin_versions bin_versions
  # copy providers files to download required binaries
  rm -Rf docker/engine/providers/*
  set -e
  for i in $(find cloned-engine/lib -name "tf-providers*") ; do
    provider=$(echo $i | sed -r 's/.+\/(.+)(\/.+){2}.tf/\1/')
    mkdir -p docker/engine/providers/$provider
    cp $i docker/engine/providers/$provider/
    $sed -ri 's/\{\{.+\}\}/flushed/g' docker/engine/providers/$provider/*
  done

  set +e
  if [ ! -z $DOCKER_HOST ] ; then
    return_code=1
    while [ $return_code -ne 0 ] ; do
      echo "waiting docker port 2375 to be available..."
      sleep 2
      nc -zv localhost 2375 2>/dev/null
      return_code=$?
    done
  fi
  set -e

  export DOCKER_BUILDKIT=1
  docker build --network "host" --build-arg SCCACHE_REDIS=$SCCACHE_REDIS -t qoveryrd/engine:${tag} .

  rm -f docker/engine/load.sh
  rm -f bin_versions
  rm -Rf docker/engine/providers/*
}

function build_ci_image() { ## Build CI image locally. Args: <tag_version>
  prepare_engine
  tag=$(generate_image_tag)

  cp docker/load.sh docker/ci/load.sh
  cp docker/bin_versions docker/ci/bin_versions

  cd docker/ci
  export DOCKER_BUILDKIT=1
  docker build --network "host" --build-arg SCCACHE_REDIS=$SCCACHE_REDIS --no-cache -t qoveryrd/ci:${tag} .
  cd ..

  rm -f docker/ci/load.sh
  rm -f docker/ci/bin_versions
}

function push_image() { ## Push Engine local image with current commit ID as tag
  prepare_engine
  tag=$(generate_image_tag)
  set -e

  docker login -u $DOCKER_LOGIN -p $DOCKER_TOKEN
  docker push qoveryrd/engine:${tag}
}

function push_ci_image() { ## Push CI local image with current commit ID as tag
  prepare_engine
  tag=$(generate_image_tag)
  set -e

  docker login -u $DOCKER_LOGIN -p $DOCKER_TOKEN
  docker push qoveryrd/ci:${tag}
}

function new_release() { ## Release a new engine version with commit ID as tag prepare_engine
  prepare_engine
  tag=$(generate_image_tag)

  check_untracked_files
  build_image
  push_image

  echo -e "\e[92mNew image name is: qoveryrd/engine:${tag}\e[0m"
}

function set_release_ga() { ## Release a new engine version and mark it as globally available
  prepare_engine
  tag=$(generate_image_tag)
  curl -s -X PUT -H "X-Qovery-Signature: $ENGINE_VERSION_CONTROLLER_TOKEN" "https://${QOVERY_API}/api/v1/engine-version?type=ga&version=${tag}" || exit 1
}

function get_release_ga() { ## Get globally available release version
  echo -e "Last defined GA version: "
  curl -s -H "X-Qovery-Signature: $ENGINE_VERSION_CONTROLLER_TOKEN" "https://${QOVERY_API}/api/v1/engine-version?type=ga"  || exit 1
}

function release_to_prod() { ## Release GA to prod
  prepare_engine
  tag=$(generate_image_tag)
  AWS_ACCESS_KEY_ID=$AWS_PROD_DEPLOY_ACCESS_KEY \
  AWS_SECRET_ACCESS_KEY=$AWS_PROD_DEPLOY_SECRET_KEY \
  AWS_DEFAULT_REGION=eu-west-3 \
  helm upgrade --kubeconfig $AWS_PROD_KUBECONFIG --install --history-max 50 --wait --namespace qovery qovery-engine \
   $ENGINE_DIR/lib/common/bootstrap/charts/qovery-engine --set \
image.tag="$tag",\
environmentVariables.QOVERY_NATS_URL="tls://nats-external.qovery.com:4242",\
environmentVariables.QOVERY_NATS_USER="$QOVERY_NATS_USER",\
environmentVariables.QOVERY_NATS_PASSWORD="$QOVERY_NATS_PASSWORD",\
environmentVariables.CLOUD_PROVIDER="aws",\
environmentVariables.LIB_ROOT_DIR="/home/qovery/lib",\
environmentVariables.DOCKER_HOST="tcp://0.0.0.0:2375",\
environmentVariables.RUST_LOG="DEBUG,rusoto_core::request=info,hyper=info",\
environmentVariables.WORKSPACE_ROOT_DIR="/home/qovery",\
environmentVariables.VAULT_ADDR="https://vaultemort.qovery.com",\
environmentVariables.VAULT_TOKEN="s.TOhnuuSbHzrPEJ46X1E4xBUM",\
environmentVariables.WORKSPACE_ROOT_DIR="/home/qovery",\
resources.limits.cpu="1",\
resources.limits.memory="2Gi",\
resources.requests.cpu="500m",\
resources.requests.memory="2Gi"
}

# Tests

function prepare_tests() { ## Update all CHANGE-ME fields from cloned-engine
  set -e

  cat .env | while read item ; do
    key=$(echo $item | $awk -F'=' '{ print $1}')
    value=$(echo $item | $sed -r "s,^\w+='(.+)'$,\1,g" | $sed 's,/,\\\/,g')
    echo "Updating $key value"
    find ${ENGINE_DIR}/test* -type f -exec $sed -ri "s/CHANGE-ME\\/$key/$value/g" {} +
  done
  print_title "Generating Vault Token"
  export VAULT_TOKEN=$(vault write -format=json auth/approle/login role_id=$VAULT_ROLE_ID secret_id=$VAULT_SECRET_ID | jq -r ".auth.client_token")
}

function single_test() { ## Run a single test. Arg, test name: aws::aws_environment::deploy_a_working_environment_with_domain
  test_name=$1
  export RUST_LOG=info
  export_env
  prepare_engine
  prepare_tests

  cargo build --color=always --all --all-targets
  sccache -s
  cd $ENGINE_DIR
  cargo test --package qovery-engine --test lib $test_name -- --ignored --exact
}

function use_sccache() {
  export RUSTC_WRAPPER=/usr/bin/sccache
  sccache --version
  sccache -s
}

function export_env() { ## Export environment variables from .env file
  use_sccache
  while IFS= read line ; do
    key=$(echo $line | $awk -F'=' '{ print $1}')
    value=$(echo $line | $sed -r "s,^\w+='(.+)'$,\1,g")
    if [ "$key" != "QOVERY_SSH_USER" ] ; then
      export $key=$value
    fi
  done <".env"
}

function run_tests(){ ## Run tests on qovery-engine. Args: cargo filter, GH branch name, threads
  filter_tests=$1
  GITHUB_ENGINE_BRANCH_NAME=$2
  nb_treads=$3
  print_title "RUNNING TESTS - $filter_tests"
  export RUST_LOG=info
  export_env
  prepare_engine
  prepare_tests

  STARTTIME=$(date +%s)

  sccache -s
  cd $ENGINE_DIR

  mkdir -p $GITLAB_LOG_OUTPUT_DIR
  touch $GITLAB_LOG_OUTPUT_DIR/tests.logs

  # Note: keep release, we don't waste time because of multiple cache and it drastically help to speed up prod build
  cargo test --color always --features $filter_tests --manifest-path Cargo.toml -- --color always --test-threads=$nb_treads -Z unstable-options --format json 2>&1 | tee $GITLAB_LOG_OUTPUT_DIR/output.log
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
  GITHUB_ENGINE_BRANCH_NAME=$1
  nb_treads=$2
  export RUST_LOG=info
  export_env
  prepare_engine

  set -e

  print_title "CARGO FMT"
  cargo fmt --all -- --check --color=always  || (echo "Use cargo fmt to format your code"; exit 1)

  print_title "CARGO BUILD NO WARNING"
  RUSTFLAGS="--deny warnings" cargo check || (echo "Solve your warnings to succeed"; exit 1)

  # FIXME fix warning in the engine and enable clippy
  # cargo clippy
}

# need to debug?
if [ ! -z $DEBUG_REQUIRED ] ; then
  echo "DEBUG MODE ENABLED FOR 1H"
  sleep 3600
fi

if [ $ARGS_NUM -eq 0 ] ; then
  print_help
fi

# Check if github called it with parameters
if [ ! -z $GITHUB_COMMIT_ID ] ; then
  commit_id=$GITHUB_COMMIT_ID
  RUNNING_ON_CI=1
# Check if running manually
elif [ ! -z $GITLAB_USER_ID ] ; then
  commit_id=$CI_COMMIT_SHA
  RUNNING_ON_CI=1
else
  commit_id="$(git rev-parse HEAD)"
  export GITLAB_LOG_UTILITIES_DIR="logs_output"
  export GITLAB_LOG_OUTPUT_DIR="logs_output"
fi
echo "Detected commit ID: $commit_id"

case $1 in
build)
  build
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
push_image)
  push_image
  ;;
push_ci_image)
  push_ci_image
  ;;
set_release_ga)
  set_release_ga
  ;;
release_to_prod)
  release_to_prod
  ;;
get_release_ga)
  get_release_ga
  ;;
aws_self_hosted)
  run_tests test-aws-self-hosted $commit_id 20
  ;;
do_self_hosted)
  run_tests test-do-self-hosted $commit_id 20
  ;;
all_self_hosted)
  run_tests test-all-self-hosted $commit_id 20
  ;;
aws_managed_services)
  run_tests test-aws-managed-services $commit_id 20
  ;;
do_managed_services)
  run_tests test-do-managed-services $commit_id 20
  ;;
all_managed_services)
  run_tests test-all-managed-services $commit_id 20
  ;;
aws_infra)
  run_tests test-aws-infra $commit_id 20
  ;;
do_infra)
  run_tests test-do-infra $commit_id 20
  ;;
test_all)
  run_tests test-all $commit_id 20
  ;;
single_test)
  check_num_args 2
  single_test $commit_id
  ;;
prepare_tests)
  prepare_tests
  ;;
prepare_engine)
  prepare_engine
  ;;
lint)
  lint
  ;;
*)
  print_help
  ;;
esac

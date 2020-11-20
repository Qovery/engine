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
export LIB_ROOT_DIR=$(pwd)/$ENGINE_DIR/lib
export RUNNING_ON_CI=0
export ENGINE_BRANCH=""

function print_help() {
  echo "Usage: $0 <option>"
  $grep '##' $0 | $grep -v grep | $sed -r "s/^function\s(\w+).+##\s*(.+)/\1| \2/g" | $awk 'BEGIN {FS = "|"}; {printf "\033[36m%-30s\033[0m %s\n", $1, $2}' | sort
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
  if [ $(git config --get remote.origin.url | $grep -c "github.com:Qovery/engine.git") -ne 1 ] ; then
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
      if [ ! -z $GITHUB_ENGINE_BRANCH_NAME ] ; then
        ENGINE_BRANCH=$GITHUB_ENGINE_BRANCH_NAME
      elif [ ! -z $CI_COMMIT_REF_NAME ] ; then
        ENGINE_BRANCH=$CI_COMMIT_REF_NAME
      else
        echo "Can't get commit ID"
        exit 1
      fi
    else
      ENGINE_BRANCH=$(git branch --show-current)
    fi

    if [ -e $ENGINE_DIR ] ; then
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

function build_image() { ## Build Engine image locally. Args: <tag_version>
  prepare_engine
  tag=$(generate_image_tag)

  cp docker/load.sh docker/engine/load.sh
  cp docker/bin_versions bin_versions
  # copy providers files to download required binaries
  rm -Rf docker/engine/providers/*
  set -e
  find $LIB_ROOT_DIR -name "tf-providers*" -exec cp {} docker/engine/providers/ \;
  $sed -ri 's/\{\{.+\}\}/flushed/g' docker/engine/providers/*
  DOCKER_BUILDKIT=1 docker build -t qoveryrd/engine:${tag} .

  rm -f docker/engine/load.sh
  rm -f bin_versions
  rm -f docker/engine/providers/*
}

function build_ci_image() { ## Build CI image locally. Args: <tag_version>
  prepare_engine
  tag=$(generate_image_tag)

  cp docker/load.sh docker/ci/load.sh
  cp docker/bin_versions docker/ci/bin_versions

  cd docker/ci
  DOCKER_BUILDKIT=1 docker build --no-cache -t qoveryrd/ci:${tag} .
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

function new_release() { ## Release a new engine version with commit ID as tagprepare_engine
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
environmentVariables.NATS_SERVER="panic.qovery.com:4242",\
environmentVariables.CLOUD_PROVIDER="aws",\
environmentVariables.LIB_ROOT_DIR="/home/qovery/lib",\
environmentVariables.DOCKER_HOST="tcp://0.0.0.0:2375",\
environmentVariables.RUST_LOG="DEBUG,rusoto_core::request=info,hyper=info",\
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
}

function single_test() { ## Run a single test. Arg, test name: aws::aws_environment::deploy_a_working_environment_with_domain
  test_name=$1
  export RUST_LOG=info
  export_env
  prepare_engine
  prepare_tests
  cd $ENGINE_DIR
  cargo test --package qovery-engine --test lib $test_name -- --ignored --exact
}

function export_env() { ## Export environment variables from .env file
  while IFS= read line ; do
    key=$(echo $line | $awk -F'=' '{ print $1}')
    value=$(echo $line | $sed -r "s,^\w+='(.+)'$,\1,g")
    if [ "$key" != "QOVERY_SSH_USER" ] ; then
      export $key=$value
    fi
  done <".env"
}

function all_tests(){ ## Run all tests on qovery-engine
  GITHUB_ENGINE_BRANCH_NAME=$1
  nb_treads=$2
  #export RUST_LOG=info
  export_env
  prepare_engine
  prepare_tests
  cd $ENGINE_DIR
  #cargo test --color always -- --ignored --color always --test-threads=$nb_treads -Z unstable-options --format json | tee results_all.json
  cargo test --color always -- --ignored --color always --test-threads=$nb_treads 
}

function fast_tests(){ ## Run fast tests only on qovery-engine
  GITHUB_ENGINE_BRANCH_NAME=$1
  nb_treads=$2
  export RUST_LOG=info
  export_env
  prepare_engine
  prepare_tests
  cd $ENGINE_DIR
  cargo test --color always -- --color always --test-threads=$nb_treads -Z unstable-options --format json | tee results.json
  TESTS_STATUS="${PIPESTATUS[0]}"
  cat results.json | cargo2junit > results-fast.xml
  return $TESTS_STATUS
}

if [ $ARGS_NUM -eq 0 ] ; then
  print_help
fi

if [ ! -z $GITHUB_ENGINE_BRANCH_NAME ] ; then
  commit_id=$GITHUB_ENGINE_BRANCH_NAME
  RUNNING_ON_CI=1
elif [ ! -z $GITLAB_USER_ID ] ; then
  commit_id=$CI_COMMIT_SHA
  RUNNING_ON_CI=1
else
  commit_id="$(git rev-parse HEAD)"
fi
echo "Detected commit ID: $commit_id"

case $1 in
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
fast_tests)
  fast_tests $commit_id 8
  ;;
fast_tests_seq)
  fast_tests $commit_id 1
  ;;
all_tests)
  all_tests $commit_id 8
  ;;
all_tests_seq)
  all_tests $commit_id 1
  ;;
single_test)
  check_num_args 2
  single_test $commit_id $2
  ;;
prepare_tests)
  prepare_tests
  ;;
prepare_engine)
  prepare_engine
  ;;
*)
  print_help
  ;;
esac

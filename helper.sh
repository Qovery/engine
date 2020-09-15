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

ARGS_NUM=$#
# Note: this is the dev version for the moment as the prod one is not released yet
QOVERY_API="api.qovery.com"

function check_num_args() {
  desired_number=$1
  if [ $ARGS_NUM -ne ${desired_number} ]; then
    echo "Illegal number of parameters, required $desired_number"
    exit 1
  fi
}

function check_untracked_files() {
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
    exit 1
  fi
}

function get_commit_id() {
  git rev-parse HEAD
}

function build_image() { ## Build Engine image locally. Args: <tag_version>
  tag=$(get_commit_id)
  cp docker/load.sh docker/engine/load.sh
  docker build -t qoveryrd/engine:${tag} .
  rm -f docker/engine/load.sh
}

function build_ci_image() { ## Build CI image locally. Args: <tag_version>
  tag=$(get_commit_id)
  cp docker/load.sh docker/ci/load.sh
  cd docker/ci
  docker build --no-cache -t qoveryrd/ci:${tag} .
  rm -f docker/ci/load.sh
}

function push_image() { ## Push Engine local image with current commit ID as tag
  tag=$(get_commit_id)
  set -e

  docker login -u $DOCKER_LOGIN -p $DOCKER_TOKEN
  docker push qoveryrd/engine:${tag}
}

function push_ci_image() { ## Push CI local image with current commit ID as tag
  tag=$(get_commit_id)
  set -e

  docker login -u $DOCKER_LOGIN -p $DOCKER_TOKEN
  docker push qoveryrd/ci:${tag}
}

function s3_upload_resources() { ## Upload Qovery Engine resources (lib) to S3
  check_untracked_files
  set -e

  export AWS_ACCESS_KEY_ID="$S3_RES_ACCESS_KEY_ID"
  export AWS_SECRET_ACCESS_KEY="$S3_RES_SECRET_KEY_ID"

  bucket=prod-qengine-resources
  file_prefix=$(get_commit_id)
  file="${file_prefix}-lib.tgz"
  file_with_bootstrap="${file_prefix}-lib-with-bootstrap.tgz"
  resource="/${bucket}/${file}"

  set +e
  aws s3api get-object-tagging --bucket prod-qengine-resources --key $file 2>/dev/null
  if [ $? -ne 0 ] ; then
    set -e
    echo "Pushing lib to s3"
    tar czf $file --exclude='*/bootstrap' lib
    tar czf $file_with_bootstrap lib
    aws s3 cp $file s3://$bucket
    aws s3 cp $file_with_bootstrap s3://$bucket
    aws s3api put-object-acl --bucket $bucket --key $file --acl public-read
    aws s3api put-object-acl --bucket $bucket --key $file_with_bootstrap --acl public-read
    rm -f $file
    rm -f $file_with_bootstrap
  else
    echo "File $file already exists in bucket $bucket"
    exit 1
  fi
}

function new_release() { ## Release a new engine version with commit ID as tag
  tag=$(get_commit_id)
  check_untracked_files
  build_image
  push_image
  s3_upload_resources
  echo -e "\e[92mNew image name is: qoveryrd/engine:${tag}\e[0m"
}

function set_release_ga() { ## Release a new engine version and mark it as globally available
  tag=$(get_commit_id)
  # Note: this is the dev version for the moment as the prod one is not released yet
  curl -s -X PUT -H "X-Qovery-Signature: $ENGINE_VERSION_CONTROLLER_TOKEN" "https://${QOVERY_API}/api/v1/engine-version?type=ga&version=${tag}"
}

function get_release_ga() { ## Get globally available release version
  echo -e "Last defined GA version: "
  curl -s -H "X-Qovery-Signature: $ENGINE_VERSION_CONTROLLER_TOKEN" "https://${QOVERY_API}/api/v1/engine-version?type=ga"
}

function fast_tests() { # Run fast tests only on qovery-engine
  export LIB_ROOT_DIR=$(pwd)/lib
  export RUST_LOG=info
  cd qovery-engine
  cargo test --color always
}

function all_tests() { # Run all tests on qovery-engine
  export LIB_ROOT_DIR=$(pwd)/lib
  export RUST_LOG=info
  cd qovery-engine
  cargo test --color always -- --ignored
}

case $1 in
build_image)
  build_image $2
  ;;
build_ci_image)
  build_ci_image $2
  ;;
s3_upload_resources)
  s3_upload_resources
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
get_release_ga)
  get_release_ga
  ;;
fast_tests)
  fast_tests
  ;;
all_tests)
  all_tests
  ;;
*)
  echo "Usage: $0 <option>"
  $grep '##' $0 | $grep -v grep | $sed -r "s/^function\s(\w+).+##\s*(.+)/\1| \2/g" | $awk 'BEGIN {FS = "|"}; {printf "\033[36m%-30s\033[0m %s\n", $1, $2}' | sort
  exit 1
  ;;
esac

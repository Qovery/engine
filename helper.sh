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
  docker build -t qoveryrd/engine:${tag} .
}

function push_image() { ## Push local image with current commit ID as tag
  tag=$(get_commit_id)
  set -e

  docker login -u $DOCKER_LOGIN -p $DOCKER_TOKEN
  docker push qoveryrd/engine:${tag}
}

function s3_upload_resources() { ## Upload Qovery Engine resources (lib) to S3
  check_untracked_files
  set -e

  export AWS_ACCESS_KEY_ID="$S3_RES_ACCESS_KEY_ID"
  export AWS_SECRET_ACCESS_KEY="$S3_RES_SECRET_KEY_ID"

  bucket=prod-qengine-resources
  file_prefix=$(get_commit_id)
  file="${file_prefix}-lib.tgz"
  resource="/${bucket}/${file}"

  aws s3api get-object-tagging --bucket prod-qengine-resources --key $file 2>/dev/null
  if [ $? -ne 0 ] ; then
    echo "Pushing lib to s3"
    tar czf $file --exclude='*/bootstrap' lib
    aws s3 cp $file s3://$bucket
    aws s3api put-object-acl --bucket $bucket --key $file --acl public-read
    rm -f $file
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

function new_ga_release() { ## Release a new engine version and mark it as globally available
  tag=$(get_commit_id)
  new_release
  # Note: this is the dev version for the moment as the prod one is not released yet
  curl -X PUT -H "X-Qovery-Signature: $ENGINE_VERSION_CONTROLLER_TOKEN" "https://api-dev.qovery.com/api/v1/engine-version?type=ga&version=${tag}"
}

case $1 in
build_image)
  build_image $2
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
*)
  echo "Usage: $0 <option>"
  $grep '##' $0 | $grep -v grep | $sed -r "s/^function\s(\w+).+##\s*(.+)/\1| \2/g" | $awk 'BEGIN {FS = "|"}; {printf "\033[36m%-30s\033[0m %s\n", $1, $2}' | sort
  exit 1
  ;;
esac
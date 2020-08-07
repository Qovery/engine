#!/usr/bin/env bash

#set -x

ARGS_NUM=$#

function check_num_args() {
  desired_number=$1
  if [ $ARGS_NUM -ne ${desired_number} ]; then
    echo "Illegal number of parameters, required $desired_number"
    exit 1
  fi
}

function check_untracked_files() {
  if [ $(git ls-files --other --exclude-standard --directory | wc -l) -ne 0 ] ; then
    echo "There are some untracked files changes by git. Ensure you've commited all your files first"
    exit 1
  fi
}

function get_commit_id() {
  git rev-parse --short HEAD
}

function build_image() { ## Build Engine image locally. Args: <tag_version>
  tag=$(get_commit_id)
  docker build -t qovery-engine:${tag} .
}

function s3_upload_resources() { ## Upload Qovery Engine resources (lib) to S3
  check_untracked_files

  export AWS_ACCESS_KEY_ID="AKIAUD622NVNEHRNE5G2"
  export AWS_SECRET_ACCESS_KEY="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"

  bucket=prod-qengine-resources
  file_prefix=$(get_commit_id)
  file="${file_prefix}-lib.tgz"
  resource="/${bucket}/${file}"

  aws s3api get-object-tagging --bucket prod-qengine-resources --key $file 2>/dev/null
  if [ $? -ne 0 ] ; then
    tar -czf $file lib
    aws s3 cp $file s3://$bucket
    rm -f $file
  else
    echo "File $file already exists in bucket $bucket"
    exit 1
  fi
}

function new_release() { ## Generate a new release with commit ID as tag
    check_untracked_files
    build_image
    s3_upload_resources
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
*)
  echo "Usage: $0 <option>"
  grep '##' $0 | grep -v grep | sed -r "s/^function\s(\w+).+##\s*(.+)/\1| \2/g" | awk 'BEGIN {FS = "|"}; {printf "\033[36m%-30s\033[0m %s\n", $1, $2}' | sort
  exit 1
  ;;
esac
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

function print_help() {
  echo "Usage: $0 <option>"
  $grep '##' $0 | $grep -v grep | $sed -r "s/^function\s(\w+).+##\s*(.+)/\1| \2/g" | $awk 'BEGIN {FS = "|"}; {printf "\033[36m%-30s\033[0m %s\n", $1, $2}' | sort
  exit 1
}

function check_num_args() {
  desired_number=$1
  if [ $ARGS_NUM -ne ${desired_number} ]; then
    echo "Illegal number of parameters, required $desired_number"
    exit 1
  fi
}

function fast_tests() { # Run fast tests only on qovery-engine
  export LIB_ROOT_DIR=$(pwd)/lib
  #export RUST_LOG=info
  nb_treads=$1
  check_env
  cargo test --color always -- --color always --test-threads=$nb_treads -Z unstable-options --format json | tee results.json
  cat results.json | cargo2junit > results.xml
}

function all_tests() { # Run all tests on qovery-engine
  export LIB_ROOT_DIR=$(pwd)/lib
  #export RUST_LOG=info
  nb_treads=$1
  check_env
  cargo test --color always -- --ignored --test-threads=$nb_treads
}

function variable_not_found() {
  echo "Required variable not found: $1"
  exit 1
}

function check_env() {
  if [ -f .env ] ; then
    for line in $(cat .env) ; do
      export $line
    done
  fi
  test -z $AWS_ACCESS_KEY_ID && variable_not_found "AWS_ACCESS_KEY_ID"
  test -z $AWS_SECRET_ACCESS_KEY && variable_not_found "AWS_SECRET_ACCESS_KEY"
  test -z $AWS_DEFAULT_REGION && variable_not_found "AWS_DEFAULT_REGION"
  test -z $TERRAFORM_AWS_ACCESS_KEY_ID && variable_not_found "TERRAFORM_AWS_ACCESS_KEY_ID"
  test -z $TERRAFORM_AWS_SECRET_ACCESS_KEY && variable_not_found "TERRAFORM_AWS_SECRET_ACCESS_KEY"
  test -z $CLOUDFLARE_ID && variable_not_found "CLOUDFLARE_ID"
  test -z $CLOUDFLARE_TOKEN && variable_not_found "CLOUDFLARE_TOKEN"
  test -z $CLOUDFLARE_DOMAIN && variable_not_found "CLOUDFLARE_DOMAIN"
  test -z $DIGITAL_OCEAN_TOKEN && variable_not_found "DIGITAL_OCEAN_TOKEN"
}

if [ $ARGS_NUM -eq 0 ] ; then
  print_help
fi
set -u

case $1 in
fast_tests)
  fast_tests 8
  ;;
fast_tests-seq)
  fast_tests 1
  ;;
all_tests)
  all_tests 8
  ;;
all_tests-seq)
  all_tests 1
  ;;
*)
  print_help
  ;;
esac

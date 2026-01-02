#!/usr/bin/env bash

set -e
#set -x

total_args=$#
awk=awk
sed=sed
if [ "$(uname)" == "Darwin" ] ; then
  awk='gawk'
  sed='gsed'
fi

function help() {
  echo "Usage: $0 <command> <args>"
  grep '##' $0 | grep -v grep | $sed -r "s/^function\s(\w+).+##\s*(.+)$/\1| \2/g" | $awk 'BEGIN {FS = "|"}; {printf "\033[36m%-30s\033[0m %s\n", $1, $2}'
}

function check_args() {
  num_args=$(($1+1))
  if [[ $total_args -ne $num_args ]]; then
    echo "Illegal number of parameters, expected $num_args"
    exit 2
  fi
}

function get_connection_details() { ## print environment variables to connect to cluster
  echo 'export GOOGLE_CREDENTIALS="$(cat gcp-credentials.json)"'
  echo 'export KUBECONFIG={{ object_storage_kubeconfig_bucket }}/{{ kubernetes_cluster_id }}.yaml'
}

case $1 in
  get_connection_details)
    get_connection_details
  ;;
  *)
    help
    exit 1
  ;;
esac
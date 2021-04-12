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
  echo 'export AWS_ACCESS_KEY_ID="{{ aws_access_key }}"'
  echo 'export AWS_SECRET_ACCESS_KEY="{{ aws_secret_key }}"'
  echo 'export AWS_DEFAULT_REGION="{{ aws_region }}"'
  echo 'export KUBECONFIG={{ s3_kubeconfig_bucket }}/{{ kubernetes_cluster_id }}.yaml'
}

function get_engine_version_to_use() { ## get the engine version for a given cluster. Args: token, api_fqdn, cluster_id
  ENGINE_VERSION_CONTROLLER_TOKEN=$1
  API_FQDN=$2
  CLUSTER_ID=$3
  API_URL="https://$API_FQDN/api/v1/engine-version"

  curl -s -H "X-Qovery-Signature: $ENGINE_VERSION_CONTROLLER_TOKEN" "$API_URL?type=cluster&clusterId=$CLUSTER_ID" && exit 0
}

function get_agent_version_to_use() { ## get the agent version for a given cluster. Args: token, api_fqdn, cluster_id
  AGENT_VERSION_CONTROLLER_TOKEN=$1
  API_FQDN=$2
  CLUSTER_ID=$3
  API_URL="https://$API_FQDN/api/v1/agent-version"

  curl -s -H "X-Qovery-Signature: $AGENT_VERSION_CONTROLLER_TOKEN" "$API_URL?type=cluster&clusterId=$CLUSTER_ID" && exit 0
}

case $1 in
  get_engine_version_to_use)
    check_args 3
    get_engine_version_to_use "$2" "$3" "$4"
  ;;
  get_agent_version_to_use)
    check_args 3
    get_agent_version_to_use "$2" "$3" "$4"
  ;;
  get_connection_details)
    get_connection_details
  ;;
  *)
    help
    exit 1
  ;;
esac

# If ok return nothing
echo "{}"

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

# Todo: do it engine side when terraform/helm split will be done
function is_cni_old_installed_version() { ## Check if an old CNI version is already installed
  export AWS_ACCESS_KEY_ID="{{ aws_access_key }}"
  export AWS_SECRET_ACCESS_KEY="{{ aws_secret_key }}"
  export AWS_DEFAULT_REGION="{{ aws_region }}"
  export KUBECONFIG={{ s3_kubeconfig_bucket }}/{{ kubernetes_cluster_id }}.yaml

  # shellcheck disable=SC2046
  if [ $(kubectl -n kube-system get ds aws-node -o json | jq -c '.spec.selector.matchLabels' | grep -c '"k8s-app":"aws-node"') -eq 1 ] ; then
    echo '{"is_cni_old_installed_version": "true"}'
  else
    echo '{"is_cni_old_installed_version": "false"}'
  fi
  exit 0
}

function enable_cni_managed_by_helm() { ## Check if an old CNI version is already installed
  export AWS_ACCESS_KEY_ID="{{ aws_access_key }}"
  export AWS_SECRET_ACCESS_KEY="{{ aws_secret_key }}"
  export AWS_DEFAULT_REGION="{{ aws_region }}"
  export KUBECONFIG={{ s3_kubeconfig_bucket }}/{{ kubernetes_cluster_id }}.yaml

  set +e
  # shellcheck disable=SC2046
  if [ "$(kubectl -n kube-system get daemonset -l k8s-app=aws-node,app.kubernetes.io/managed-by=Helm 2>&1 | grep -ic 'No resources found')" == "0" ] ; then
    exit 0
  fi

  for kind in daemonSet clusterRole clusterRoleBinding serviceAccount; do
    echo "setting annotations and labels on $kind/aws-node"
    kubectl -n kube-system annotate --overwrite $kind aws-node meta.helm.sh/release-name=aws-vpc-cni
    kubectl -n kube-system annotate --overwrite $kind aws-node meta.helm.sh/release-namespace=kube-system
    kubectl -n kube-system label --overwrite $kind aws-node app.kubernetes.io/managed-by=Helm
  done
  exit 0
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
  is_cni_old_installed_version)
    is_cni_old_installed_version
  ;;
  enable_cni_managed_by_helm)
    enable_cni_managed_by_helm
  ;;
  *)
    help
    exit 1
  ;;
esac

# If ok return nothing
echo "{}"

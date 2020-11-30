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

export AWS_ACCESS_KEY_ID="{{ aws_access_key }}"
export AWS_SECRET_ACCESS_KEY="{{ aws_secret_key }}"
export AWS_DEFAULT_REGION="{{ aws_region }}"

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

function create_elasticsearch_role_for_aws_service() { ## Create ES linked role for a service. Args: role_name, service_name
  # Because this is uniq and should never be deleted once spawned (because it can break other ES clusters), I'm using
  # dirty trick to ensure it won't never be deleted. Try to use suffix if the service support it instead of this.
  role_name=$1
  service_name=$2
  # shellcheck disable=SC2046
  if [ $(aws iam get-role --role-name "$role_name" 2>&1 | grep -c 'cannot be found') -eq 1 ] ; then
    aws iam create-service-linked-role --aws-service-name "$service_name"
    if [ $? -ne 0 ] ; then
      echo "Error while creating $service_name linked role $role_name"
      exit 1
    fi
  fi
}

function create_ecr_repository() { ## Create ECR repository. Args: repo_name
  ecr_name=$1
  # shellcheck disable=SC2046
  if [ $(aws ecr describe-repositories --repository-names qovery 2>&1 | grep -c 'RepositoryNotFoundException') -eq 1 ] ; then
    aws ecr create-repository --repository-name $ecr_name
      if [ $? -ne 0 ] ; then
        echo "Error while creating ECR repository $ecr_name"
      exit 1
    fi
  fi
}

function is_cni_handled_by_aws() { ## Check if CNI is handled by AWS or Helm. Args: repo_name kubeconfig file path
  set +e
  export KUBECONFIG=$1
  kubectl -n kube-system get daemonset -l k8s-app=aws-node,app.kubernetes.io/managed-by=Helm 2>&1 | grep -ic 'No resources found'
}

function delete_cni_managed_by_aws() { ## Delete the CNI not handled by helm. Args: repo_name kubeconfig file path
  export KUBECONFIG=$1
  if [ is_cni_handled_by_aws == "0" ] ; then
    echo -e "$cni_aws"
    exit 0
  fi

  set +e
  kubectl -n kube-system delete daemonset aws-node > /dev/null 2>&1
  kubectl -n kube-system delete clusterrole aws-node > /dev/null 2>&1
  kubectl -n kube-system delete clusterrolebinding aws-node > /dev/null 2>&1
  kubectl -n kube-system delete crd eniconfigs.crd.k8s.amazonaws.com > /dev/null 2>&1
  kubectl -n kube-system delete serviceaccount aws-node > /dev/null 2>&1

  # shellcheck disable=SC2046
  while [ $(kubectl -n kube-system get daemonset aws-node 2>&1 | grep -ic 'NotFound') -ne 1 ] ; do
    sleep 1
  done

  # shellcheck disable=SC2046
  while [ $(kubectl -n kube-system get clusterrole aws-node 2>&1 | grep -ic 'NotFound') -ne 1 ] ; do
    sleep 1
  done

  # shellcheck disable=SC2046
  while [ $(kubectl -n kube-system get clusterrolebinding aws-node 2>&1 | grep -ic 'NotFound') -ne 1 ] ; do
    sleep 1
  done

  # shellcheck disable=SC2046
  while [ $(kubectl -n kube-system get crd eniconfigs.crd.k8s.amazonaws.com 2>&1 | grep -ic 'NotFound') -ne 1 ] ; do
    sleep 1
  done

  # shellcheck disable=SC2046
  while [ $(kubectl -n kube-system get serviceaccount aws-node 2>&1 | grep -ic 'NotFound') -ne 1 ] ; do
    sleep 1
  done

  echo -e '{"aws":"0"}'
  exit 0
}

function get_engine_version_to_use() { ## get the engine version for a given cluster. Args: token, api_fqdn, cluster_id
  ENGINE_VERSION_CONTROLLER_TOKEN=$1
  API_FQDN=$2
  CLUSTER_ID=$3
  API_URL="https://$API_FQDN/api/v1/engine-version"

  curl -s -H "X-Qovery-Signature: $ENGINE_VERSION_CONTROLLER_TOKEN" "$API_URL?type=cluster&clusterId=$CLUSTER_ID" && exit 0
}

case $1 in
  create_elasticsearch_role_for_aws_service)
    check_args 2
    create_elasticsearch_role_for_aws_service "$2" "$3"
  ;;
  create_ecr_repository)
    check_args 1
    create_ecr_repository "$2"
  ;;
  is_cni_handled_by_aws)
    check_args 1
    is_cni_handled_by_aws "$2"
    exit 0
  ;;
  delete_cni_managed_by_aws)
    check_args 1
    delete_cni_managed_by_aws "$2"
  ;;
  get_engine_version_to_use)
    check_args 3
    get_engine_version_to_use "$2" "$3" "$4"
  ;;
  *)
    help
    exit 1
  ;;
esac

# If ok return nothing
echo "{}"
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

case $1 in
  create_elasticsearch_role_for_aws_service)
    check_args 2
    create_elasticsearch_role_for_aws_service "$2" "$3"
  ;;
  create_ecr_repository)
    check_args 1
    create_ecr_repository "$2"
  ;;
  *)
    help
    exit 1
  ;;
esac

# If ok return nothing
echo "{}"
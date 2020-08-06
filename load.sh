#!/usr/bin/env bash

set -e

ARCH="amd64"
SYSTEM="linux"
BIN_DEST_FOLDER="/binaries"
ARGS_NUM=$#

TERRAFORM_VERSION="0.12.29"
HELM_VERSION="3.2.4"
KUBECTL_VERSION="1.18.6"
AWS_IAM_AUTHENTICATOR_VERSION="0.5.1"

function check_num_args() {
  desired_number=$1
  if [ $ARGS_NUM -ne ${desired_number} ]; then
    echo "Illegal number of parameters, required $desired_number"
    exit 1
  fi
}

function download() { ## Download prerequisites binaries for the engine
  mkdir -p /tmp/binaries && cd /tmp/binaries
  mkdir $BIN_DEST_FOLDER

  # terraform
  curl -so terraform.zip https://releases.hashicorp.com/terraform/${TERRAFORM_VERSION}/terraform_${TERRAFORM_VERSION}_${SYSTEM}_${ARCH}.zip
  unzip terraform.zip
  mv terraform $BIN_DEST_FOLDER/terraform${TERRAFORM_VERSION}

  # helm
  curl -so helm.tgz https://get.helm.sh/helm-v${HELM_VERSION}-${SYSTEM}-${ARCH}.tar.gz
  tar -zxf helm.tgz
  mv linux-amd64/helm $BIN_DEST_FOLDER/helm${HELM_VERSION}

  # kubectl
  curl -so kubectl https://storage.googleapis.com/kubernetes-release/release/v1.18.6/bin/linux/amd64/kubectl
  mv kubectl $BIN_DEST_FOLDER/kubectl${KUBECTL_VERSION}

  # Aws iam authenticator
  curl -so aws-iam-authenticator https://github.com/kubernetes-sigs/aws-iam-authenticator/releases/download/v${AWS_IAM_AUTHENTICATOR_VERSION}/aws-iam-authenticator_${AWS_IAM_AUTHENTICATOR_VERSION}_${SYSTEM}_${ARCH}
  mv aws-iam-authenticator $BIN_DEST_FOLDER/aws-iam-authenticator${AWS_IAM_AUTHENTICATOR_VERSION}

  chmod 755 $BIN_DEST_FOLDER/*
}

function install() { ## Make symlinks to install binaries in default PATH
  BIN_DIR=$1
  ln -s $BIN_DIR/helm${HELM_VERSION} /usr/bin/helm
  ln -s $BIN_DIR/terraform${TERRAFORM_VERSION} /usr/bin/terraform
  ln -s $BIN_DIR/kubectl${KUBECTL_VERSION} /usr/bin/kubectl
  ln -s $BIN_DIR/aws-iam-authenticator${AWS_IAM_AUTHENTICATOR_VERSION} /usr/bin/aws-iam-authenticator
}

case $1 in
download)
  download
  ;;
install)
  check_num_args 2
  install $2
  ;;
*)
  check_num_args 1
  echo "Usage: $0 <option>"
  grep '##' $0 | grep -v grep | sed -r "s/^function\s(\w+).+##\s*(.+)/\1: \2/g" | awk 'BEGIN {FS = ":"}; {printf "\033[36m%-30s\033[0m %s\n", $1, $2}'
  exit 1
  ;;
esac
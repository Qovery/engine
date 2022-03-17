#!/usr/bin/env bash

set -e

ARCH="amd64"
SYSTEM="linux"
ARGS_NUM=$#
TMP_FOLDER="/tmp/binaries"

function check_num_args() {
  desired_number=$1
  if [ $ARGS_NUM -ne ${desired_number} ]; then
    echo "Illegal number of parameters, required $desired_number"
    exit 1
  fi
}

source bin_versions

function download() { ## Download prerequisites binaries for the engine
  echo "Downloading binaries"

  mkdir -p $TMP_FOLDER && cd $TMP_FOLDER
  mkdir $BIN_DEST_FOLDER

  # buildpacks (binary is named `pack`)
  echo "Downloading buildpacks"
  curl -sLo pack.tgz https://github.com/buildpacks/pack/releases/download/v${PACK_VERSION}/pack-v${PACK_VERSION}-linux.tgz
  tar -zxf pack.tgz
  mv pack $BIN_DEST_FOLDER/pack${PACK_VERSION}

  # terraform
  echo "Downloading terraform"
  curl -so terraform.zip https://releases.hashicorp.com/terraform/${TERRAFORM_VERSION}/terraform_${TERRAFORM_VERSION}_${SYSTEM}_${ARCH}.zip
  unzip terraform.zip
  mv terraform $BIN_DEST_FOLDER/terraform${TERRAFORM_VERSION}

  # helm
  echo "Downloading helm"
  curl -so helm.tgz https://get.helm.sh/helm-v${HELM_VERSION}-${SYSTEM}-${ARCH}.tar.gz
  tar -zxf helm.tgz
  mv linux-amd64/helm $BIN_DEST_FOLDER/helm${HELM_VERSION}

  # kubectl
  echo "Downloading kubectl"
  curl -so kubectl https://storage.googleapis.com/kubernetes-release/release/v${KUBECTL_VERSION}/bin/linux/amd64/kubectl
  mv kubectl $BIN_DEST_FOLDER/kubectl${KUBECTL_VERSION}

  # Aws iam authenticator
  echo "Downloading AWS IAM Authenticator"
  curl -sLo aws-iam-authenticator https://github.com/kubernetes-sigs/aws-iam-authenticator/releases/download/v${AWS_IAM_AUTHENTICATOR_VERSION}/aws-iam-authenticator_${AWS_IAM_AUTHENTICATOR_VERSION}_${SYSTEM}_${ARCH}
  mv aws-iam-authenticator $BIN_DEST_FOLDER/aws-iam-authenticator${AWS_IAM_AUTHENTICATOR_VERSION}

  # Dumb init
  echo "Downloading Dumb init"
  curl -sLo dumb-init https://github.com/Yelp/dumb-init/releases/download/v${DUMB_INIT_VERSION}/dumb-init_${DUMB_INIT_VERSION}_x86_64
  mv dumb-init $BIN_DEST_FOLDER/

  # Vault
  echo "Downloading Vault"
  curl -so vault.zip https://releases.hashicorp.com/vault/${VAULT_VERSION}/vault_${VAULT_VERSION}_${SYSTEM}_${ARCH}.zip
  unzip vault.zip
  mv vault $BIN_DEST_FOLDER/vault${VAULT_VERSION}

  # DigitalOcean Doctl
  echo "Downloading doctl"
  curl -Lso doctl.tgz https://github.com/digitalocean/doctl/releases/download/v${DOCTL_VERSION}/doctl-${DOCTL_VERSION}-linux-amd64.tar.gz
  tar -zxf doctl.tgz
  mv doctl $BIN_DEST_FOLDER/doctl${DOCTL_VERSION}

  # Clean
  chmod 755 $BIN_DEST_FOLDER/*
  rm -Rf $TMP_FOLDER
  cd ~
}

function download_terraform_plugins() {
  echo "Downloading Terraform plugins"
  origin_dir=$(pwd)
  cd docker/engine/providers
  for i in * ; do
    cd $i
    sed -ri 's/\{%.+//g' *.tf
    terraform init
    cd -
  done
  cd $origin_dir
}

function install() { ## Make symlinks to install binaries in default PATH
  BIN_DIR=$1

  ln -s $BIN_DIR/pack${PACK_VERSION} /usr/bin/pack
  ln -s $BIN_DIR/helm${HELM_VERSION} /usr/bin/helm
  ln -s $BIN_DIR/terraform${TERRAFORM_VERSION} /usr/bin/terraform
  ln -s $BIN_DIR/kubectl${KUBECTL_VERSION} /usr/bin/kubectl
  ln -s $BIN_DIR/aws-iam-authenticator${AWS_IAM_AUTHENTICATOR_VERSION} /usr/bin/aws-iam-authenticator
  ln -s $BIN_DIR/doctl${DOCTL_VERSION} /usr/bin/doctl
  ln -s $BIN_DIR/vault${VAULT_VERSION} /usr/bin/vault

  # Generate all symlinks at once
  ln -s $BIN_DIR/* /usr/bin/
}

function post_install() { ## Perform post installation (plugins, etc...)
  # install helm diff
  /usr/bin/helm plugin install --version v3.1.3 https://github.com/databus23/helm-diff
}

case $1 in
download)
  download
  ;;
install)
  check_num_args 2
  install $2
  ;;
post_install)
  post_install
  ;;
download_terraform_plugins)
  download_terraform_plugins
  ;;
*)
  echo "Usage: $0 <option>"
  grep '##' $0 | grep -v grep | sed -r "s/^function\s(\w+).+##\s*(.+)/\1| \2/g" | awk 'BEGIN {FS = "|"}; {printf "\033[36m%-30s\033[0m %s\n", $1, $2}'
  exit 1
  ;;
esac

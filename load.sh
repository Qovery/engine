#!/usr/bin/env bash

mkdir /tmp/binaries && cd /tmp/binaries
mkdir /binaries

# docker
wget https://download.docker.com/linux/static/stable/x86_64/docker-19.03.9.tgz
tar zxf docker-19.03.9.tgz # output into docker - TODO change
mv docker/docker docker/docker19.03
chmod +x docker/docker19.03
mv docker/docker19.03 /binaries/
mv docker/dockerd docker/dockerd19.03
chmod +x docker/dockerd19.03
mv docker/dockerd19.03 /binaries/

# terraform
wget https://releases.hashicorp.com/terraform/0.12.29/terraform_0.12.29_linux_amd64.zip
unzip terraform_0.12.29_linux_amd64.zip
mv terraform terraform0.12
chmod +x terraform0.12
mv terraform0.12 /binaries/

# helm
wget https://get.helm.sh/helm-v3.2.4-linux-amd64.tar.gz
tar zxf helm-v3.2.4-linux-amd64.tar.gz
mv linux-amd64/helm linux-amd64/helm3.2
chmod +x linux-amd64/helm3.2
mv linux-amd64/helm3.2 /binaries/

# kubectl
wget https://storage.googleapis.com/kubernetes-release/release/v1.18.6/bin/linux/amd64/kubectl
mv kubectl kubectl1.18
chmod +x kubectl1.18
mv kubectl1.18 /binaries/

# To find the version do an `apt list -a xxxx` helm inside the CI image

# Upgrading kubectl/helm requires to update kubeconfig to not use anymore client.authentication.k8s.io/v1beta1
ARG KUBECTL_VERSION="1.27.10-1.1"
ARG HELM_VERSION="3.15.2-1"
ARG TERRAFORM_VERSION="1.3.3"
ARG VAULT_VERSION="1.13.0-1"
ARG HELM_DIFF_VERSION="v3.8.1"
# If you update docker version, please also update the docker in docker version
# within the engine chart
ARG DOCKER_VERSION="5:27.0.3-1~debian.12~bookworm"
ARG BUILDX_VERSION="0.15.1-1~debian.12~bookworm"
ARG PACK_VERSION="0.33.1"
ARG CONTAINERD_VERSION="1.7.18-1"
ARG SKOPEO_VERSION=1.9.3+ds1-1+b9

ARG BIN_DEST_FOLDER="/binaries"
ARG RUST_IMAGE="public.ecr.aws/r3m4q3r9/qovery-ci:rust-1.79.0-2024-07-17T07-40-35"


###########################################
#
#  ENGINE CI IMAGE 
#
###########################################
FROM $RUST_IMAGE AS engine_ci

ARG BIN_DEST_FOLDER
ENV TF_PLUGIN_CACHE_DIR=/root/.terraform.d/plugin-cache

ARG HELM_VERSION
ARG KUBECTL_VERSION
ARG TERRAFORM_VERSION
ARG VAULT_VERSION
ARG HELM_DIFF_VERSION
ARG BUILDX_VERSION
ARG PACK_VERSION
ARG AWS_IAM_AUTHENTICATOR_VERSION
ARG DOCKER_VERSION
ARG CONTAINERD_VERSION
ARG SKOPEO_VERSION

RUN apt-get update && \
  apt-get -y --allow-downgrades install \
  make libfindbin-libs-perl curl unzip pkg-config libssl-dev git jq gcc cmake protobuf-compiler libprotobuf-dev git-lfs python3 apt-transport-https ca-certificates gnupg binutils \
  skopeo=$SKOPEO_VERSION \
  docker-ce=$DOCKER_VERSION \
  docker-ce-cli=$DOCKER_VERSION \
  docker-buildx-plugin=$BUILDX_VERSION \
  containerd.io=$CONTAINERD_VERSION \
  helm=$HELM_VERSION \
  kubectl=$KUBECTL_VERSION \
  vault=$VAULT_VERSION && \
  curl -sSL "https://github.com/buildpacks/pack/releases/download/v$PACK_VERSION/pack-v$PACK_VERSION-linux.tgz" | tar -C /usr/local/bin/ --no-same-owner -xzv pack && \
  helm plugin install --version ${HELM_DIFF_VERSION} https://github.com/databus23/helm-diff && \
  mkdir /build ${BIN_DEST_FOLDER} && \
  mkdir -p $TF_PLUGIN_CACHE_DIR

# Hashicorp apt repository does not package terraform for arm64 ...
RUN curl -sLo terraform.zip https://releases.hashicorp.com/terraform/${TERRAFORM_VERSION}/terraform_${TERRAFORM_VERSION}_linux_$(dpkg --print-architecture).zip && \
  unzip terraform.zip && \
  mv terraform /usr/bin/ && \
  rm -rf terraform.zip 

WORKDIR /build


CMD ["/bin/sh"]


###########################################
#
#  ENGINE BUILDER IMAGE 
#
###########################################
FROM $RUST_IMAGE AS build

ARG TERRAFORM_VERSION
ARG SCCACHE_REDIS
ENV SCCACHE_REDIS=$SCCACHE_REDIS
ENV RUSTFLAGS="-C link-arg=-Wl,--compress-debug-sections=zlib -C force-frame-pointers=yes"
ENV CARGO_FLAGS="--release --bin engine_grpc"

WORKDIR /build

# Init terraform providers and DL deps for build
COPY docker docker
RUN <<EOF
  mkdir -p app/src
  mkdir -p lib-engine/src

  apt-get update
  apt-get -y install make cmake protobuf-compiler libprotobuf-dev binutils unzip apt-transport-https ca-certificates

  curl -sLo terraform.zip https://releases.hashicorp.com/terraform/${TERRAFORM_VERSION}/terraform_${TERRAFORM_VERSION}_linux_$(dpkg --print-architecture).zip
  unzip terraform.zip
  mv terraform /usr/bin/
  rm -rf terraform.zip

  for i in $(find lib-engine/lib -name "tf-providers*")
  do
      provider=$(echo $i | sed -r 's/.+\/(.+)(\/.+){2}.tf/\1/')
      mkdir -p docker/engine/providers/$provider
      cp $i docker/engine/providers/$provider/
      sed -ri 's/\{\{.+\}\}/flushed/g' docker/engine/providers/$provider/*
  done
  ./docker/download_terraform_plugins.sh

EOF

COPY Cargo.toml .
COPY Cargo.lock .
COPY lib-engine/Cargo.toml lib-engine/
COPY lib-engine/Cargo.lock lib-engine/
COPY app/Cargo.toml app/
COPY app/build.rs app/
COPY app/proto app/proto

RUN <<EOF
  set -e

  # Use stub main.rs and lib.rs to build and cache dependencies
  echo "pub fn main() {}" > app/src/main_grpc.rs
  echo "// dummy" > lib-engine/src/lib.rs
  cargo build ${CARGO_FLAGS}
  rm app/src/main_grpc.rs
  rm lib-engine/src/lib.rs
  rm target/release/deps/engine*
EOF

COPY . .

# build engine
# If sscache is set we set rustc wrapper
RUN <<EOF
  set -e
  
  touch app/src/main_grpc.rs
  touch lib-engine/src/lib.rs

  if [ -z "${SCCACHE_REDIS}" ];
  then
      unset SCCACHE_REDIS
      cargo build ${CARGO_FLAGS}
  else
      echo "USING SSCACHE"
      export RUSTC_WRAPPER=/usr/bin/sccache
      sccache --version
      sccache --show-stats
  fi

  cp /build/target/release/engine_grpc /build/target/release/engine_grpc_stripped
  strip -s /build/target/release/engine_grpc_stripped
EOF


###########################################
#
#  ENGINE FINAL IMAGE 
#
###########################################
FROM public.ecr.aws/r3m4q3r9/qovery-ci:debian-bookworm-slim AS run

ARG BIN_DEST_FOLDER

ENV HOME_DIR="/home/qovery"
ENV BIN_DIR=$HOME_DIR/binaries
ENV TF_PLUGIN_CACHE_DIR=$HOME_DIR/.terraform.d/plugin-cache
ENV BIN_DEST_FOLDER=$BIN_DEST_FOLDER
ENV ARCHIVE_BUCKET_NAME=qovery-engine-deployment-archive

ARG HELM_VERSION
ARG KUBECTL_VERSION
ARG TERRAFORM_VERSION
ARG VAULT_VERSION
ARG HELM_DIFF_VERSION
ARG AWS_IAM_AUTHENTICATOR_VERSION
ARG DOCKER_VERSION
ARG BUILDX_VERSION
ARG PACK_VERSION
ARG CONTAINERD_VERSION
ARG SKOPEO_VERSION

RUN apt-get update && apt-get install -y \
  apt-transport-https ca-certificates curl gnupg lsb-release && \
  curl -fsSL https://download.docker.com/linux/debian/gpg | gpg --dearmor -o /usr/share/keyrings/docker.gpg  && \
  curl -fsSL https://pkgs.k8s.io/core:/stable:/v1.28/deb/Release.key | gpg --dearmor -o /usr/share/keyrings/kubernetes.gpg && \
  curl https://baltocdn.com/helm/signing.asc | gpg --dearmor -o /usr/share/keyrings/helm.gpg && \
  curl https://apt.releases.hashicorp.com/gpg | gpg --dearmor -o /usr/share/keyrings/hashicorp-archive-keyring.gpg && \
  curl https://packages.cloud.google.com/apt/doc/apt-key.gpg | gpg --dearmor -o /usr/share/keyrings/cloud.google.gpg && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/docker.gpg] https://download.docker.com/linux/debian $(lsb_release -cs) stable" | tee /etc/apt/sources.list.d/docker.list && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/kubernetes.gpg] https://pkgs.k8s.io/core:/stable:/v1.27/deb/ /" | tee -a /etc/apt/sources.list.d/kubernetes.list && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/helm.gpg] https://baltocdn.com/helm/stable/debian/ all main" | tee /etc/apt/sources.list.d/helm-stable-debian.list && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/hashicorp-archive-keyring.gpg] https://apt.releases.hashicorp.com $(lsb_release -cs) main" | tee /etc/apt/sources.list.d/hashicorp.list && \
  echo "deb [signed-by=/usr/share/keyrings/cloud.google.gpg] https://packages.cloud.google.com/apt cloud-sdk main" | tee -a /etc/apt/sources.list.d/google-cloud-sdk.list && \
  apt-get update && \
  apt-get dist-upgrade -y && \
  apt-get install -y \
  skopeo=$SKOPEO_VERSION \
  docker-ce-cli=$DOCKER_VERSION \
  docker-buildx-plugin=$BUILDX_VERSION \
  helm=$HELM_VERSION \
  kubectl=$KUBECTL_VERSION \
  google-cloud-sdk google-cloud-sdk-gke-gcloud-auth-plugin \
  procps netcat-openbsd iproute2 dumb-init git-lfs unzip python3 && \
  curl -sSL "https://github.com/buildpacks/pack/releases/download/v$PACK_VERSION/pack-v$PACK_VERSION-linux.tgz" | tar -C /usr/local/bin/ --no-same-owner -xzv pack && \
  apt-get clean && rm -rf /var/lib/apt/lists

RUN curl -s "https://awscli.amazonaws.com/awscli-exe-linux-$(dpkg --print-architecture | sed 's/amd64/x86_64/' | sed 's/arm64/aarch64/').zip" -o "awscliv2.zip" && \
  unzip awscliv2.zip && \
  ./aws/install && \
  rm -rf awscliv2.zip aws

RUN curl -sLo terraform.zip https://releases.hashicorp.com/terraform/${TERRAFORM_VERSION}/terraform_${TERRAFORM_VERSION}_linux_$(dpkg --print-architecture).zip && \
  unzip terraform.zip && \
  mv terraform /usr/bin/ && \
  rm -rf terraform.zip 

RUN groupadd -g 1000 qovery && \
  useradd --home-dir $HOME_DIR --gid 1000 --uid 1000 -m -s /bin/bash qovery && \
  mkdir -p $TF_PLUGIN_CACHE_DIR && \
  chown -Rf 1000:1000 $HOME_DIR/.terraform.d


WORKDIR $HOME_DIR
ADD lib-engine/lib $HOME_DIR/lib
COPY --from=build --chown=qovery:qovery --chmod=500 /build/target/release/engine_grpc .
COPY --from=build --chown=qovery:qovery --chmod=500 /build/docker/engine/run.sh $HOME_DIR
COPY --from=build --chown=qovery:qovery /build/docker/bin_versions $HOME_DIR
COPY --from=build --chown=qovery:qovery /root/.terraform.d $HOME_DIR/.terraform.d

USER qovery
RUN helm plugin install --version ${HELM_DIFF_VERSION} https://github.com/databus23/helm-diff && \
  echo "disable_checkpoint = true" > ~/.terraform.rc

# for local use only
VOLUME /qovery_libs
ENV LOCAL_DEPLOY=false

ENV PATH="$HOME_DIR/binaries:${PATH}"
ENV BIN_VERSION_FILE="$HOME_DIR/bin_versions"

CMD ["/usr/bin/dumb-init", "--verbose", "--single-child", "--", "./run.sh"]



###########################################
#
#  ENGINE SLIM FINAL IMAGE 
#  thats the same image than the release one
#  but with terraform and other binary for infra install
#  stripped down
#
###########################################
FROM public.ecr.aws/r3m4q3r9/qovery-ci:debian-bookworm-slim AS run-slim

ARG BIN_DEST_FOLDER

ENV HOME_DIR="/home/qovery"
ENV BIN_DIR=$HOME_DIR/binaries
ENV BIN_DEST_FOLDER=$BIN_DEST_FOLDER

ARG HELM_VERSION
ARG KUBECTL_VERSION
ARG HELM_DIFF_VERSION
ARG DOCKER_VERSION
ARG BUILDX_VERSION
ARG PACK_VERSION
ARG CONTAINERD_VERSION
ARG SKOPEO_VERSION

RUN apt-get update && apt-get install -y \
  apt-transport-https ca-certificates curl gnupg lsb-release && \
  curl -fsSL https://download.docker.com/linux/debian/gpg | gpg --dearmor -o /usr/share/keyrings/docker.gpg  && \
  curl -fsSL https://pkgs.k8s.io/core:/stable:/v1.28/deb/Release.key | gpg --dearmor -o /usr/share/keyrings/kubernetes.gpg && \
  curl https://baltocdn.com/helm/signing.asc | gpg --dearmor -o /usr/share/keyrings/helm.gpg && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/docker.gpg] https://download.docker.com/linux/debian $(lsb_release -cs) stable" | tee /etc/apt/sources.list.d/docker.list && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/kubernetes.gpg] https://pkgs.k8s.io/core:/stable:/v1.27/deb/ /" | tee -a /etc/apt/sources.list.d/kubernetes.list && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/helm.gpg] https://baltocdn.com/helm/stable/debian/ all main" | tee /etc/apt/sources.list.d/helm-stable-debian.list && \
  apt-get update && \
  apt-get dist-upgrade -y && \
  apt-get install --no-install-recommends --no-install-suggests -y \
  skopeo=$SKOPEO_VERSION \
  docker-ce-cli=$DOCKER_VERSION \
  docker-buildx-plugin=$BUILDX_VERSION \
  helm=$HELM_VERSION \
  kubectl=$KUBECTL_VERSION \
  dumb-init git-lfs && \
  curl -sSL "https://github.com/buildpacks/pack/releases/download/v$PACK_VERSION/pack-v$PACK_VERSION-linux.tgz" | tar -C /usr/local/bin/ --no-same-owner -xzv pack && \
  apt-get clean && rm -rf /var/lib/apt/lists

RUN groupadd -g 1000 qovery && \
  useradd --home-dir $HOME_DIR --gid 1000 --uid 1000 -m -s /bin/bash qovery


WORKDIR $HOME_DIR
ADD lib-engine/lib $HOME_DIR/lib
COPY --from=build --chown=qovery:qovery --chmod=500 /build/target/release/engine_grpc_stripped engine_grpc 
COPY --from=build --chown=qovery:qovery --chmod=500 /build/docker/engine/run.sh $HOME_DIR
COPY --from=build /build/docker/bin_versions $HOME_DIR

USER qovery
RUN helm plugin install --version ${HELM_DIFF_VERSION} https://github.com/databus23/helm-diff

# for local use only
ENV LOCAL_DEPLOY=false

ENV PATH="$HOME_DIR/binaries:${PATH}"
ENV BIN_VERSION_FILE="$HOME_DIR/bin_versions"

CMD ["/usr/bin/dumb-init", "--verbose", "--single-child", "--", "./run.sh"]

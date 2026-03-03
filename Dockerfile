# To find the version do an `apt list -a xxxx` helm inside the CI image

# Upgrading kubectl/helm requires to update kubeconfig to not use anymore client.authentication.k8s.io/v1beta1
ARG KUBECTL_VERSION="1.32.7"
ARG HELM_VERSION="3.17.4-1"
ARG TERRAFORM_VERSION="1.9.7"
ARG VAULT_VERSION="1.13.0-1"
ARG HELM_DIFF_VERSION="v3.11.0"
# If you update docker version, please also update the docker in docker version
# within the engine chart
ARG DOCKER_VERSION="5:28.4.0-1~debian.13~trixie"
ARG BUILDX_VERSION="0.27.0-1~debian.13~trixie"
ARG PACK_VERSION="0.35.1"
ARG CONTAINERD_VERSION="1.7.29-1~debian.13~trixie"
ARG SKOPEO_VERSION=1.18.0+ds1-1+b5
ARG KUBENT_VERSION=0.7.3
ARG PLUTO_VERSION=5.22.8

ARG BIN_DEST_FOLDER="/binaries"
ARG RUST_IMAGE="public.ecr.aws/r3m4q3r9/qovery-ci:rust-1.92.0-2026-01-05T18-08-48"


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
ARG KUBENT_VERSION
ARG PLUTO_VERSION

RUN apt-get update && \
  apt-get -y --allow-downgrades install \
  make libfindbin-libs-perl curl unzip pkg-config libssl-dev git jq gcc cmake protobuf-compiler libprotobuf-dev git-lfs python3 apt-transport-https ca-certificates gnupg binutils \
  skopeo=$SKOPEO_VERSION \
  docker-ce=$DOCKER_VERSION \
  docker-ce-cli=$DOCKER_VERSION \
  docker-buildx-plugin=$BUILDX_VERSION \
  containerd.io=$CONTAINERD_VERSION \
  helm=$HELM_VERSION \
  vault=$VAULT_VERSION && \
  curl -sSL "https://github.com/doitintl/kube-no-trouble/releases/download/${KUBENT_VERSION}/kubent-${KUBENT_VERSION}-linux-$(dpkg --print-architecture).tar.gz" | tar -C /usr/local/bin/ --no-same-owner -xzv kubent && \
  curl -sSL "https://github.com/FairwindsOps/pluto/releases/download/v${PLUTO_VERSION}/pluto_${PLUTO_VERSION}_linux_$(dpkg --print-architecture).tar.gz" | tar -C /usr/local/bin/ --no-same-owner -xzv pluto && \
  curl -sSL "https://github.com/buildpacks/pack/releases/download/v$PACK_VERSION/pack-v$PACK_VERSION-linux$(dpkg --print-architecture | sed -e 's/amd64//' -e 's/arm64/-arm64/').tgz" | tar -C /usr/local/bin/ --no-same-owner -xzv pack && \
  helm plugin install --version ${HELM_DIFF_VERSION} https://github.com/databus23/helm-diff && \
  mkdir /build ${BIN_DEST_FOLDER} && \
  mkdir -p $TF_PLUGIN_CACHE_DIR

RUN curl -LO https://dl.k8s.io/release/v${KUBECTL_VERSION}/bin/linux/$(dpkg --print-architecture)/kubectl && install -o root -g root -m 0755 kubectl /usr/local/bin/kubectl

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
ARG CI_SCCACHE_REDIS_ENDPOINT
ARG CI_SCCACHE_REDIS_PASSWORD
ENV CI_SCCACHE_REDIS_URL=$CI_SCCACHE_REDIS_ENDPOINT
ENV CI_SCCACHE_REDIS_PASSWORD=$CI_SCCACHE_REDIS_PASSWORD
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

# If sscache is set we set rustc wrapper
RUN <<EOF
  if [ -z "${CI_SCCACHE_REDIS_ENDPOINT}" ];
  then
      echo "!!! WARNING: sccache is not used !!!"
  else
      echo "USING SSCACHE"
      export RUSTC_WRAPPER=/usr/bin/sccache
      export SCCACHE_REDIS_ENDPOINT=$CI_SCCACHE_REDIS_ENDPOINT
      export SCCACHE_REDIS_USERNAME=default
      export SCCACHE_REDIS_PASSWORD=$CI_SCCACHE_REDIS_PASSWORD
      sccache --version
      sccache --show-stats
  fi

  set -e

  # Use stub main.rs and lib.rs to build and cache dependencies
  echo "pub fn main() {}" > app/src/main_grpc.rs
  echo "// dummy" > lib-engine/src/lib.rs
  cargo build ${CARGO_FLAGS}
  if [ ! -z "${CI_SCCACHE_REDIS_ENDPOINT}" ]; then
    sccache --show-stats
  fi
  rm app/src/main_grpc.rs
  rm lib-engine/src/lib.rs
  rm target/release/deps/engine*
EOF

COPY . .

# build engine

ARG CI_COMMIT_SHORT_SHA
ENV CI_COMMIT_SHORT_SHA=$CI_COMMIT_SHORT_SHA
RUN <<EOF
  if [ -z "${CI_SCCACHE_REDIS_ENDPOINT}" ];
  then
      echo "!!! WARNING: sccache is not used !!!"
  else
      echo "USING SSCACHE"
      export RUSTC_WRAPPER=/usr/bin/sccache
      export SCCACHE_REDIS_ENDPOINT=$CI_SCCACHE_REDIS_ENDPOINT
      export SCCACHE_REDIS_USERNAME=default
      export SCCACHE_REDIS_PASSWORD=$CI_SCCACHE_REDIS_PASSWORD
      sccache --version
      sccache --show-stats
  fi

  set -e
  
  touch app/src/main_grpc.rs
  touch lib-engine/src/lib.rs
  cargo build ${CARGO_FLAGS}
  if [ ! -z "${CI_SCCACHE_REDIS_ENDPOINT}" ]; then
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
FROM public.ecr.aws/r3m4q3r9/qovery-ci:debian-trixie-slim AS run

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
ARG KUBENT_VERSION
ARG PLUTO_VERSION

RUN apt-get update && apt-get install -y \
  apt-transport-https ca-certificates curl gnupg lsb-release && \
  curl -fsSL https://download.docker.com/linux/debian/gpg | gpg --dearmor -o /usr/share/keyrings/docker.gpg  && \
  curl -fsSL https://pkgs.k8s.io/core:/stable:/v1.33/deb/Release.key | gpg --dearmor -o /usr/share/keyrings/kubernetes.gpg && \
  curl -fsSL https://packages.buildkite.com/helm-linux/helm-debian/gpgkey | gpg --dearmor -o /usr/share/keyrings/helm.gpg && \
  curl https://apt.releases.hashicorp.com/gpg | gpg --dearmor -o /usr/share/keyrings/hashicorp-archive-keyring.gpg && \
  curl https://packages.cloud.google.com/apt/doc/apt-key.gpg | gpg --dearmor -o /usr/share/keyrings/cloud.google.gpg && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/docker.gpg] https://download.docker.com/linux/debian $(lsb_release -cs) stable" | tee /etc/apt/sources.list.d/docker.list && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/kubernetes.gpg] https://pkgs.k8s.io/core:/stable:/v1.33/deb/ /" | tee -a /etc/apt/sources.list.d/kubernetes.list && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/helm.gpg] https://packages.buildkite.com/helm-linux/helm-debian/any/ any main" | tee /etc/apt/sources.list.d/helm-stable-debian.list && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/hashicorp-archive-keyring.gpg] https://apt.releases.hashicorp.com $(lsb_release -cs) main" | tee /etc/apt/sources.list.d/hashicorp.list && \
  echo "deb [signed-by=/usr/share/keyrings/cloud.google.gpg] https://packages.cloud.google.com/apt cloud-sdk main" | tee -a /etc/apt/sources.list.d/google-cloud-sdk.list && \
  apt-get update && \
  apt-get dist-upgrade -y && \
  apt-get install -y \
  skopeo=$SKOPEO_VERSION \
  docker-ce-cli=$DOCKER_VERSION \
  docker-buildx-plugin=$BUILDX_VERSION \
  helm=$HELM_VERSION \
  google-cloud-sdk google-cloud-sdk-gke-gcloud-auth-plugin \
  procps netcat-openbsd iproute2 dumb-init git-lfs unzip python3 && \
  curl -sL https://aka.ms/InstallAzureCLIDeb | bash && \
  curl -sSL "https://github.com/doitintl/kube-no-trouble/releases/download/${KUBENT_VERSION}/kubent-${KUBENT_VERSION}-linux-$(dpkg --print-architecture).tar.gz" | tar -C /usr/local/bin/ --no-same-owner -xzv kubent && \
  curl -sSL "https://github.com/FairwindsOps/pluto/releases/download/v${PLUTO_VERSION}/pluto_${PLUTO_VERSION}_linux_$(dpkg --print-architecture).tar.gz" | tar -C /usr/local/bin/ --no-same-owner -xzv pluto && \
  curl -sSL "https://github.com/buildpacks/pack/releases/download/v$PACK_VERSION/pack-v$PACK_VERSION-linux$(dpkg --print-architecture | sed -e 's/amd64//' -e 's/arm64/-arm64/').tgz" | tar -C /usr/local/bin/ --no-same-owner -xzv pack && \
  apt-get clean && rm -rf /var/lib/apt/lists

RUN curl -s "https://awscli.amazonaws.com/awscli-exe-linux-$(dpkg --print-architecture | sed 's/amd64/x86_64/' | sed 's/arm64/aarch64/').zip" -o "awscliv2.zip" && \
  unzip awscliv2.zip && \
  ./aws/install && \
  rm -rf awscliv2.zip aws

RUN curl -LO https://dl.k8s.io/release/v${KUBECTL_VERSION}/bin/linux/$(dpkg --print-architecture)/kubectl && install -o root -g root -m 0755 kubectl /usr/local/bin/kubectl

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
FROM public.ecr.aws/r3m4q3r9/qovery-ci:debian-trixie-slim AS run-slim

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
ARG KUBENT_VERSION
ARG PLUTO_VERSION

RUN apt-get update && apt-get install -y \
  apt-transport-https ca-certificates curl gnupg lsb-release && \
  curl -fsSL https://download.docker.com/linux/debian/gpg | gpg --dearmor -o /usr/share/keyrings/docker.gpg  && \
  curl -fsSL https://pkgs.k8s.io/core:/stable:/v1.33/deb/Release.key | gpg --dearmor -o /usr/share/keyrings/kubernetes.gpg && \
  curl -fsSL https://packages.buildkite.com/helm-linux/helm-debian/gpgkey | gpg --dearmor -o /usr/share/keyrings/helm.gpg && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/docker.gpg] https://download.docker.com/linux/debian $(lsb_release -cs) stable" | tee /etc/apt/sources.list.d/docker.list && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/kubernetes.gpg] https://pkgs.k8s.io/core:/stable:/v1.33/deb/ /" | tee -a /etc/apt/sources.list.d/kubernetes.list && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/helm.gpg] https://packages.buildkite.com/helm-linux/helm-debian/any/ any main" | tee /etc/apt/sources.list.d/helm-stable-debian.list && \
  apt-get update && \
  apt-get dist-upgrade -y && \
  apt-get install --no-install-recommends --no-install-suggests -y \
  skopeo=$SKOPEO_VERSION \
  docker-ce-cli=$DOCKER_VERSION \
  docker-buildx-plugin=$BUILDX_VERSION \
  helm=$HELM_VERSION \
  dumb-init git-lfs && \
  curl -sSL "https://github.com/doitintl/kube-no-trouble/releases/download/${KUBENT_VERSION}/kubent-${KUBENT_VERSION}-linux-$(dpkg --print-architecture).tar.gz" | tar -C /usr/local/bin/ --no-same-owner -xzv kubent && \
  curl -sSL "https://github.com/FairwindsOps/pluto/releases/download/v${PLUTO_VERSION}/pluto_${PLUTO_VERSION}_linux_$(dpkg --print-architecture).tar.gz" | tar -C /usr/local/bin/ --no-same-owner -xzv pluto && \
  curl -sSL "https://github.com/buildpacks/pack/releases/download/v$PACK_VERSION/pack-v$PACK_VERSION-linux$(dpkg --print-architecture | sed -e 's/amd64//' -e 's/arm64/-arm64/').tgz" | tar -C /usr/local/bin/ --no-same-owner -xzv pack && \
  apt-get clean && rm -rf /var/lib/apt/lists

RUN curl -LO https://dl.k8s.io/release/v${KUBECTL_VERSION}/bin/linux/$(dpkg --print-architecture)/kubectl && install -o root -g root -m 0755 kubectl /usr/local/bin/kubectl

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

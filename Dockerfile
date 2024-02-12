# To find the version do an `apt list -a xxxx` helm inside the CI image

# Upgrading kubectl/helm requires to update kubeconfig to not use anymore client.authentication.k8s.io/v1beta1
ARG KUBECTL_VERSION="1.27.10-1.1"
ARG HELM_VERSION="3.12.3-1"
ARG TERRAFORM_VERSION="1.3.3"
ARG VAULT_VERSION="1.13.0-1"
ARG HELM_DIFF_VERSION="v3.8.1"
ARG AWS_IAM_AUTHENTICATOR_VERSION="0.5.12"
# If you update docker version, please also update the docker in docker version
# within the engine chart
ARG DOCKER_VERSION="5:25.0.3-1~debian.12~bookworm"
ARG BUILDX_VERSION="0.12.1-1~debian.12~bookworm"
ARG CONTAINERD_VERSION="1.6.28-1"

ARG BIN_DEST_FOLDER="/binaries"


###########################################
#
#  ENGINE CI IMAGE 
#
###########################################
FROM public.ecr.aws/r3m4q3r9/qovery-ci:rust-1.75.0-2024-02-05T15-08-12 as engine_ci

ARG BIN_DEST_FOLDER
ENV TF_PLUGIN_CACHE_DIR=/root/.terraform.d/plugin-cache

ARG HELM_VERSION
ARG KUBECTL_VERSION
ARG TERRAFORM_VERSION
ARG VAULT_VERSION
ARG HELM_DIFF_VERSION
ARG BUILDX_VERSION
ARG AWS_IAM_AUTHENTICATOR_VERSION
ARG DOCKER_VERSION
ARG CONTAINERD_VERSION

RUN apt-get update && \
  apt-get -y --allow-downgrades install \
  make libfindbin-libs-perl curl unzip pkg-config libssl-dev git jq gcc cmake protobuf-compiler libprotobuf-dev git-lfs python3 apt-transport-https ca-certificates gnupg \
  docker-ce=$DOCKER_VERSION \
  docker-ce-cli=$DOCKER_VERSION \
  docker-buildx-plugin=$BUILDX_VERSION \
  containerd.io=$CONTAINERD_VERSION \
  helm=$HELM_VERSION \
  kubectl=$KUBECTL_VERSION \
  vault=$VAULT_VERSION && \
  curl -sSL "https://github.com/buildpacks/pack/releases/download/v0.28.0/pack-v0.28.0-linux.tgz" | tar -C /usr/local/bin/ --no-same-owner -xzv pack && \
  helm plugin install --version ${HELM_DIFF_VERSION} https://github.com/databus23/helm-diff && \
  mkdir /build ${BIN_DEST_FOLDER} && \
  mkdir -p $TF_PLUGIN_CACHE_DIR

# TODO: Remove after migration to aws cli
# Aws iam authenticator
RUN curl -sLo aws-iam-authenticator https://github.com/kubernetes-sigs/aws-iam-authenticator/releases/download/v${AWS_IAM_AUTHENTICATOR_VERSION}/aws-iam-authenticator_${AWS_IAM_AUTHENTICATOR_VERSION}_linux_$(dpkg --print-architecture) && \
  chmod +x aws-iam-authenticator && \
  mv aws-iam-authenticator $BIN_DEST_FOLDER/aws-iam-authenticator

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
FROM engine_ci as build


ADD . .

ARG SCCACHE_REDIS
ENV SCCACHE_REDIS=$SCCACHE_REDIS

# Init terraform providers
RUN for i in $(find lib-engine/lib -name "tf-providers*") ; do \
  provider=$(echo $i | sed -r 's/.+\/(.+)(\/.+){2}.tf/\1/') ; \
  mkdir -p docker/engine/providers/$provider ; \
  cp $i docker/engine/providers/$provider/ ;  \
  sed -ri 's/\{\{.+\}\}/flushed/g' docker/engine/providers/$provider/* ; \
  done && \
  ./docker/download_terraform_plugins.sh

# build engine
# If sscache is set we set rustc wrapper
RUN export RUSTFLAGS="-C link-arg=-Wl,--compress-debug-sections=zlib -C force-frame-pointers=yes"; \
  if [ -z "${SCCACHE_REDIS}" ]; \
  then \
  unset SCCACHE_REDIS; \
  cargo build --release; \
  else \
  echo "USING SSCACHE" ; \
  export RUSTC_WRAPPER=/usr/bin/sccache; \
  sccache --version && cargo build --release && sccache --show-stats; \
  fi 




###########################################
#
#  ENGINE FINAL IMAGE 
#
###########################################
FROM public.ecr.aws/r3m4q3r9/qovery-ci:debian-bookworm-slim as run

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
ARG CONTAINERD_VERSION

RUN apt-get update && apt-get install -y \
  apt-transport-https ca-certificates curl gnupg lsb-release && \
  curl -fsSL https://download.docker.com/linux/debian/gpg | gpg --dearmor -o /usr/share/keyrings/docker.gpg  && \
  curl -fsSL https://pkgs.k8s.io/core:/stable:/v1.28/deb/Release.key | gpg --dearmor -o /usr/share/keyrings/kubernetes.gpg && \
  curl https://baltocdn.com/helm/signing.asc | gpg --dearmor -o /usr/share/keyrings/helm.gpg && \
  curl https://apt.releases.hashicorp.com/gpg | gpg --dearmor -o /usr/share/keyrings/hashicorp-archive-keyring.gpg && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/docker.gpg] https://download.docker.com/linux/debian $(lsb_release -cs) stable" | tee /etc/apt/sources.list.d/docker.list && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/kubernetes.gpg] https://pkgs.k8s.io/core:/stable:/v1.27/deb/ /" | tee -a /etc/apt/sources.list.d/kubernetes.list && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/helm.gpg] https://baltocdn.com/helm/stable/debian/ all main" | tee /etc/apt/sources.list.d/helm-stable-debian.list && \
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/hashicorp-archive-keyring.gpg] https://apt.releases.hashicorp.com $(lsb_release -cs) main" | tee /etc/apt/sources.list.d/hashicorp.list && \
  apt-get update && \
  apt-get dist-upgrade -y && \
  apt-get install -y \
  docker-ce-cli=$DOCKER_VERSION \
  docker-buildx-plugin=$BUILDX_VERSION \
  helm=$HELM_VERSION \
  kubectl=$KUBECTL_VERSION \
  procps netcat-openbsd iproute2 dumb-init git-lfs unzip python3 && \
  curl -sSL "https://github.com/buildpacks/pack/releases/download/v0.28.0/pack-v0.28.0-linux.tgz" | tar -C /usr/local/bin/ --no-same-owner -xzv pack && \
  apt-get clean && rm -rf /var/lib/apt/lists

RUN curl -s "https://awscli.amazonaws.com/awscli-exe-linux-$(dpkg --print-architecture | sed 's/amd64/x86_64/' | sed 's/arm64/aarch64/').zip" -o "awscliv2.zip" && \
  unzip awscliv2.zip && \
  ./aws/install && \
  rm -rf awscliv2.zip aws

RUN echo "deb [signed-by=/usr/share/keyrings/cloud.google.gpg] http://packages.cloud.google.com/apt cloud-sdk main" | tee -a /etc/apt/sources.list.d/google-cloud-sdk.list && curl https://packages.cloud.google.com/apt/doc/apt-key.gpg | gpg --dearmor -o /usr/share/keyrings/cloud.google.gpg && apt-get update -y && apt-get install google-cloud-sdk google-cloud-sdk-gke-gcloud-auth-plugin -y

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
COPY --from=build /build/target/release/engine_grpc .
COPY --from=build /build/docker/engine/run.sh $HOME_DIR
COPY --from=build /build/docker/bin_versions $HOME_DIR
COPY --from=build /root/.terraform.d $HOME_DIR/.terraform.d
COPY --from=build $BIN_DEST_FOLDER/aws-iam-authenticator /usr/bin/aws-iam-authenticator

RUN chown -Rf qovery:qovery . && \
  chown qovery:qovery /usr/bin/aws-iam-authenticator && \
  chmod 500 engine_grpc 

USER qovery
RUN helm plugin install --version ${HELM_DIFF_VERSION} https://github.com/databus23/helm-diff && \
  echo "disable_checkpoint = true" > ~/.terraform.rc

# for local use only
VOLUME /qovery_libs
ENV LOCAL_DEPLOY false

ENV PATH="$HOME_DIR/binaries:${PATH}"
ENV BIN_VERSION_FILE="$HOME_DIR/bin_versions"

CMD ["/usr/bin/dumb-init", "--verbose", "--single-child", "--", "./run.sh"]

ARG BIN_DEST_FOLDER="/binaries"

# docker build stage
FROM rust:1.50.0-slim-buster as build

ARG BIN_DEST_FOLDER
ARG SCCACHE_REDIS
ENV BIN_DEST_FOLDER=$BIN_DEST_FOLDER
ENV BIN_DIR=/root/binaries
ENV TF_PLUGIN_CACHE_DIR=/root/.terraform.d/plugin-cache
ENV RUSTC_WRAPPER=/usr/bin/sccache
ENV SCCACHE_REDIS=$SCCACHE_REDIS

WORKDIR /root
RUN apt-get update && apt-get -y install make libfindbin-libs-perl curl unzip pkg-config libssl-dev
ADD . .

# run release build
RUN mkdir -p $TF_PLUGIN_CACHE_DIR
RUN ./docker/load.sh download $BIN_DEST_FOLDER
RUN ./docker/load.sh install $BIN_DEST_FOLDER
RUN ./docker/load.sh download_terraform_plugins

# get sccache
RUN sccache_release=$(curl --silent "https://github.com/Qovery/sccache-bin/releases/latest" | sed -r 's/^.+tag\/(.+)">.+/\1/') && \
    curl -sLo /usr/bin/sccache https://github.com/Qovery/sccache-bin/releases/download/${sccache_release}/sccache && \
    chmod 755 /usr/bin/sccache

# build engine
RUN sccache --version && sccache -s && cargo build --release && sccache -s

# Final image
FROM debian:buster-slim as run

ARG BIN_DEST_FOLDER

ENV HOME_DIR="/home/qovery"
ENV BIN_DIR=$HOME_DIR/binaries
ENV TF_PLUGIN_CACHE_DIR=$HOME_DIR/.terraform.d/plugin-cache
ENV BIN_DEST_FOLDER=$BIN_DEST_FOLDER
ENV ARCHIVE_BUCKET_NAME=qovery-engine-deployment-archive

RUN apt-get update && \
    apt-get -y install curl docker.io vim awscli procps netcat-openbsd iproute2 lldb && \
    apt-get clean &&\
    groupadd -g 1000 qovery && \
    useradd --home-dir $HOME_DIR --gid 1000 --uid 1000 -m -s /bin/bash qovery && \
    mkdir $HOME_DIR/.terraform.d/ && \
    chown -Rf 1000:1000 $HOME_DIR/.terraform.d

WORKDIR $HOME_DIR
ADD cloned-engine/lib $HOME_DIR/lib
COPY --from=build /root/target/release/app .
COPY --from=build /root/docker/engine/load.sh $HOME_DIR
COPY --from=build /root/docker/engine/run.sh $HOME_DIR
COPY --from=build /root/bin_versions $HOME_DIR
COPY --from=build /root/.terraform.d $HOME_DIR/.terraform.d
COPY --from=build $BIN_DEST_FOLDER $BIN_DIR

RUN ./load.sh install $BIN_DIR && \
    chown -Rf qovery. . && \
    chmod 500 app && \
    rm -f ./load.sh

USER qovery

# for local use only
VOLUME /qovery_libs
ENV LOCAL_DEPLOY false

ENV PATH="$HOME_DIR/binaries:${PATH}"
ENV BIN_VERSION_FILE="$HOME_DIR/bin_versions"

CMD ["./run.sh"]


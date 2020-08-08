ARG BIN_DEST_FOLDER="/binaries"

# docker build stage
FROM rust:1.45-slim-buster as build

ARG BIN_DEST_FOLDER
ENV BIN_DEST_FOLDER=$BIN_DEST_FOLDER

RUN apt-get update && apt-get -y install make libfindbin-libs-perl curl unzip
WORKDIR /usr/src/app
ADD . .

# run tests
#RUN cargo test --workspace
# run release build
RUN cargo build --release

RUN ./docker/load.sh download

# Final image
FROM debian:buster-slim as run

ARG BIN_DEST_FOLDER

ENV HOME_DIR="/home/qovery"
ENV BIN_DIR=$HOME_DIR/binaries
ENV BIN_DEST_FOLDER=$BIN_DEST_FOLDER

RUN apt-get update && \
    apt-get -y install awscli && \
    apt-get clean &&\
    groupadd -g 1000 qovery && \
    useradd --home-dir $HOME_DIR --gid 1000 --uid 1000 -m -s /bin/bash qovery

WORKDIR $HOME_DIR
COPY --from=build /usr/src/app/target/release/app .
COPY --from=build /usr/src/app/docker/load.sh .
COPY --from=build /usr/src/app/docker/run.sh .
COPY --from=build $BIN_DEST_FOLDER $BIN_DIR

RUN ./load.sh install $BIN_DIR && chown -Rf qovery. . && chmod 500 app

USER qovery
ENV PATH="/home/qovery/binaries:${PATH}"

CMD ["./app"]

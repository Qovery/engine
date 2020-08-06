# docker build stage
FROM rust:1.45-slim-buster as build

RUN apt-get update && apt-get -y install make libfindbin-libs-perl curl unzip
WORKDIR /usr/src/app
ADD . .

RUN ./load.sh download

# run tests
#RUN cargo test --workspace
# run release build
RUN cargo build --release

# Final image
FROM debian:buster-slim as run

ENV HOME_DIR="/home/qovery"
ENV BIN_DIR=$HOME_DIR/binaries

RUN groupadd -g 1000 qovery && \
    useradd --home-dir $HOME_DIR --gid 1000 --uid 1000 -m -s /bin/bash qovery

WORKDIR $HOME_DIR
COPY --from=build /usr/src/app/target/release/app .
COPY --from=build /usr/src/app/load.sh .
COPY --from=build $BIN_DL_DEST_FOLDER $BIN_DIR

RUN ./load.sh install $BIN_DIR

# TODO load lib directory from S3

RUN chown -Rf qovery. . && chmod 500 app

USER qovery
ENV PATH="/home/qovery/binaries:${PATH}"

CMD ["./app"]

# docker build stage
FROM rust:1.45-slim-buster as build

RUN apt-get update && apt-get -y install make libfindbin-libs-perl wget unzip
WORKDIR /usr/src/app
ADD . .

RUN chmod +x load.sh && ./load.sh

# run tests
#RUN cargo test --workspace
# run release build
RUN cargo build --release

FROM debian:buster-slim as run

RUN groupadd -g 1000 qovery && \
    useradd --home-dir /home/qovery --gid 1000 --uid 1000 -m -s /bin/bash qovery

WORKDIR /home/qovery
COPY --from=build /usr/src/app/target/release/app .
COPY --from=build /usr/src/app/lib ./lib
COPY --from=build /binaries ./binaries

RUN ln -s /home/qovery/binaries/docker19.03 /usr/bin/docker
RUN ln -s /home/qovery/binaries/dockerd19.03 /usr/bin/dockerd
RUN ln -s /home/qovery/binaries/helm3.2 /usr/bin/helm
RUN ln -s /home/qovery/binaries/terraform0.12 /usr/bin/terraform
RUN ln -s /home/qovery/binaries/kubectl1.18 /usr/bin/kubectl

# TODO load lib directory from S3

RUN chown -Rf qovery. . && chmod 500 app

USER qovery
ENV PATH="/home/qovery/binaries:${PATH}"

CMD ["./app"]

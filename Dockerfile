# build stage
FROM ekidd/rust-musl-builder:stable as cargo-build

WORKDIR /usr/src/app

ADD app app
ADD lib lib
ADD qovery-engine qovery-engine
ADD qovery-engine-shared qovery-engine-shared
ADD qovery-engine-task-manager qovery-engine-task-manager
ADD Cargo.toml Cargo.toml
ADD Cargo.lock Cargo.lock

RUN sudo chown -R rust:rust .

RUN cargo build --release

# final stage
FROM alpine:latest

RUN addgroup -g 1000 app
RUN adduser -D -s /bin/sh -u 1000 -G app app

WORKDIR /home/app/bin/

COPY --from=cargo-build /usr/src/app/target/x86_64-unknown-linux-musl/release/app .

RUN chown app:app app
USER app

CMD ["./app"]

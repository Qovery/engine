FROM rust:1.48.0-buster

ARG BIN_DEST_FOLDER="/binaries"
ENV BIN_DIR=$BIN_DEST_FOLDER

RUN apt-get update && apt-get install -y docker.io awscli curl libssh-dev pkg-config unzip make netcat-openbsd jq

RUN cargo install cargo2junit
ENV PATH="/root/.cargo/bin/:${PATH}"

ADD load.sh /
ADD bin_versions /

RUN /load.sh download && ./load.sh install $BIN_DIR

### Docker

# https://github.com/docker/docker/tree/master/hack/dind
ENV DIND_COMMIT ed89041433a031cafc0a0f19cfe573c31688d377

RUN set -eux; \
	curl -so /usr/local/bin/dind "https://raw.githubusercontent.com/docker/docker/${DIND_COMMIT}/hack/dind"; \
	chmod +x /usr/local/bin/dind
ADD dockerd-entrypoint.sh /usr/bin/dockerd-entrypoint.sh

VOLUME /var/lib/docker
EXPOSE 2375 2376

ENTRYPOINT ["/usr/bin/dockerd-entrypoint.sh"]
CMD ["sh"]

FROM docker:19.03.12-dind

ARG BIN_DEST_FOLDER="/binaries"
ENV HOME_DIR="/root"
ENV BIN_DIR=$HOME_DIR/binaries

RUN apk add -U --no-cache git bash aws-cli curl cargo openssl-dev
ADD load.sh /
RUN /load.sh download && ./load.sh install $BIN_DIR
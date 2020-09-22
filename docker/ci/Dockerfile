FROM docker:19.03.12-dind

ARG BIN_DEST_FOLDER="/binaries"
ENV BIN_DIR=$BIN_DEST_FOLDER

RUN apk add -U --no-cache git bash aws-cli curl cargo openssl-dev
ADD load.sh /
ADD bin_versions /

RUN /load.sh download && ./load.sh install $BIN_DIR
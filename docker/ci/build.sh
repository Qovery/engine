#!/usr/bin/env bash

docker build -t qoveryrd/ci:$(git rev-parse HEAD) .
docker push qoveryrd/ci:$(git rev-parse HEAD)
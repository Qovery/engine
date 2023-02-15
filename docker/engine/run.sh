#!/usr/bin/env bash

git lfs install
git config --global credential.helper '!f() { sleep 1; echo "username=${GIT_USER}"; echo "password=${GIT_PASSWORD}"; }; f'

exec dumb-init --single-child -- ./engine_grpc


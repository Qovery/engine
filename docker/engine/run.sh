#!/usr/bin/env bash


echo "Cleaning workspace ${WORKSPACE_ROOT_DIR:=/home/qovery}/.qovery-workspace"
mkdir ${WORKSPACE_ROOT_DIR}/.qovery-workspace 
rm -rf ${WORKSPACE_ROOT_DIR}/.qovery-workspace/*

git lfs install
git config --global credential.helper '!f() { sleep 1; echo "username=${GIT_USER}"; echo "password=${GIT_PASSWORD}"; }; f'

exec dumb-init --single-child -- ./engine_grpc


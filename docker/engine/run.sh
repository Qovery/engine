#!/usr/bin/env bash

if [ -z "$GRPC_SERVER" ]
then
    exec dumb-init --single-child -- ./app
else 
    exec dumb-init --single-child -- ./engine_grpc
fi


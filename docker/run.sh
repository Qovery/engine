#!/usr/bin/env bash

set -e

# Get libs
curl -o libs.tgz $LIB_ARCHIVE
tar -xzf libs.tgz

# Run
dumb-init ./app
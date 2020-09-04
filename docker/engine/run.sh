#!/usr/bin/env bash

#set -x

function get_lib() {
    curl -so lib.tgz ${ENGINE_RES_URL}
    echo $?
}

# shellcheck disable=SC2236
if [ -z "$ENGINE_RES_URL" ] ; then
  echo "Missing ENGINE_RES_URL variable!"
  exit 1
fi

if [ "$ENGINE_RES_URL" == "" ] ; then
  echo "ENGINE_RES_URL variable is empty!"
  exit 1
fi

# Load lib
counter=0
max_retry=5
while ! get_lib ; do
  if [ $counter -gt $max_retry ] ; then
    echo "Wasn't able to load Engine lib"
    exit 1
  fi
  counter=$((counter+1))
  sleep 10
done
tar -xzf lib.tgz
rm -f lib.tgz

# Run
dumb-init ./app
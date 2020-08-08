#!/usr/bin/env bash

function get_lib() {
    AWS_ACCESS_KEY_ID="${ENGINE_RES_AK}" AWS_SECRET_ACCESS_KEY="${ENGINE_RES_SK}" aws s3 cp ${ENGINE_RES_URL} lib.tgz && echo 0
    echo 1
}

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
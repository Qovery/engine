#!/usr/bin/env bash

#set -x

function get_lib() {
    if [ "$LOCAL_DEPLOY" = "true" ]
    then
      echo "Local: please ensure libs are accessible on ${ENGINE_RES_URL}"
      cp ${ENGINE_RES_URL}/lib.tgz .
    else
      echo "Production: downloading from ENGINE_RES_URL"
      curl -so lib.tgz ${ENGINE_RES_URL}
    fi
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
exec dumb-init ./app
#!/bin/sh

if [ ! -z $DOCKER_HOST ] ; then
  return_code=1
  while [ $return_code -ne 0 ] ; do
    echo "waiting docker port 2375 to be available..."
    sleep 2
    nc -zv localhost 2375 2>/dev/null
    return_code=$?
  done
fi

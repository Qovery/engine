#!/usr/bin/env bash

echo "Downloading Terraform plugins"
origin_dir=$(pwd)
cd docker/engine/providers
for i in * ; 
do
  cd $i
  sed -ri 's/\{%.+//g' *.tf
  terraform init
  cd -
done

cd $origin_dir

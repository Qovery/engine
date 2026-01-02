#!/usr/bin/env bash
# Copyright 2018 Google LLC
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#      http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.


set -e

if [ "$#" -ne 3 ]; then
    >&2 echo "3 arguments expected. Exiting."
    exit 1
fi

RESOURCE_NAMESPACE=$1
RESOURCE_TYPE=$2
RESOURCE_NAME=$3

RESOURCE_LIST=$(kubectl -n "${RESOURCE_NAMESPACE}" get "${RESOURCE_TYPE}" || exit 1)

# Delete requested resource
if [[ $RESOURCE_LIST = *"${RESOURCE_NAME}"* ]]; then
    RESOURCE_MAINTAINED_LABEL=$(kubectl -n "${RESOURCE_NAMESPACE}" get "${RESOURCE_TYPE}" "${RESOURCE_NAME}" -o=jsonpath='{.metadata.labels.maintained_by}')
    if [[ $RESOURCE_MAINTAINED_LABEL = "terraform" ]]; then
        echo "Terraform maintained ${RESOURCE_NAME} ${RESOURCE_TYPE} appears to have already been created in ${RESOURCE_NAMESPACE} namespace"
    else
        echo "Deleting default ${RESOURCE_NAME} ${RESOURCE_TYPE} found in ${RESOURCE_NAMESPACE} namespace"
        kubectl -n "${RESOURCE_NAMESPACE}" delete "${RESOURCE_TYPE}" "${RESOURCE_NAME}"
    fi
else
    echo "No default ${RESOURCE_NAME} ${RESOURCE_TYPE} found in ${RESOURCE_NAMESPACE} namespace"
fi

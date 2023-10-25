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

if [ "$#" -lt 3 ]; then
    >&2 echo "Not all expected arguments set."
    exit 1
fi

HOST=$1
TOKEN=$2
CA_CERTIFICATE=$3

shift 3

RANDOM_ID="${RANDOM}_${RANDOM}"
export TMPDIR="/tmp/kubectl_wrapper_${RANDOM_ID}"

function cleanup {
    rm -rf "${TMPDIR}"
}
trap cleanup EXIT

mkdir "${TMPDIR}"

export KUBECONFIG="${TMPDIR}/config"

# shellcheck disable=SC1117
base64 --help | grep "\--decode" && B64_ARG="--decode" || B64_ARG="-d"
echo "${CA_CERTIFICATE}" | base64 ${B64_ARG} > "${TMPDIR}/ca_certificate"

kubectl config set-cluster kubectl-wrapper --server="${HOST}" --certificate-authority="${TMPDIR}/ca_certificate" --embed-certs=true 1>/dev/null
rm -f "${TMPDIR}/ca_certificate"
kubectl config set-context kubectl-wrapper --cluster=kubectl-wrapper --user=kubectl-wrapper --namespace=default 1>/dev/null
kubectl config set-credentials kubectl-wrapper --token="${TOKEN}" 1>/dev/null
kubectl config use-context kubectl-wrapper 1>/dev/null
kubectl version 1>/dev/null

"$@"

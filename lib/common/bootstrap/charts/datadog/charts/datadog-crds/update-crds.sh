#!/bin/bash
set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)

TAG=master
if [[ $# -eq 1 ]] ; then
    TAG=$1
fi

download_crd() {
    inFile=datadoghq.com_$2.yaml
    version=$4
    outFile=datadoghq.com_$2_$version.yaml
    path=$ROOT/charts/datadog-crds/templates/$outFile
    echo "Download CRD \"$inFile\" version \"$version\" from tag \"$1\""
    curl --silent --show-error --fail --location --output "$path" "https://raw.githubusercontent.com/DataDog/datadog-operator/$1/config/crd/bases/$version/$inFile"

    ifCondition="{{- if and .Values.crds.$3 (not (.Capabilities.APIVersions.Has \"apiextensions.k8s.io/v1/CustomResourceDefinition\")) }}"
    if [ "$version" = "v1" ]; then
        ifCondition="{{- if and .Values.crds.$3 (.Capabilities.APIVersions.Has \"apiextensions.k8s.io/v1/CustomResourceDefinition\") }}"
        cp "$path" "$ROOT/crds/datadoghq.com_$2.yaml"
    fi

    VALUE="'{{ include \"datadog-crds.chart\" . }}'" \
    yq eval '.metadata.labels."helm.sh/chart" = env(VALUE)'                              -i "$path"
    yq eval '.metadata.labels."app.kubernetes.io/managed-by" = "{{ .Release.Service }}"' -i "$path"
    VALUE="'{{ include \"datadog-crds.name\" . }}'" \
    yq eval '.metadata.labels."app.kubernetes.io/name" = env(VALUE)'                     -i "$path"
    yq eval '.metadata.labels."app.kubernetes.io/instance" = "{{ .Release.Name }}"'      -i "$path"

    { echo "$ifCondition"; cat "$path"; } > tmp.file
    mv tmp.file "$path"
    echo '{{- end }}' >> "$path"
}

mkdir -p "$ROOT/crds"
download_crd "$TAG" datadogmetrics datadogMetrics v1beta1
download_crd "$TAG" datadogmetrics datadogMetrics v1
download_crd "$TAG" datadogagents datadogAgents v1beta1
download_crd "$TAG" datadogagents datadogAgents v1
download_crd "$TAG" datadogmonitors datadogMonitors v1beta1
download_crd "$TAG" datadogmonitors datadogMonitors v1

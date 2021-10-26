# Datadog CRDs

![Version: 0.3.2](https://img.shields.io/badge/Version-0.3.2-informational?style=flat-square) ![AppVersion: 1](https://img.shields.io/badge/AppVersion-1-informational?style=flat-square)

This chart was designed to allow other "datadog" charts to share `CustomResourceDefinitions` such as the `DatadogMetric`.

## How to use Datadog Helm repository

You need to add this repository to your Helm repositories:

```
helm repo add datadog https://helm.datadoghq.com
helm repo update
```

## Prerequisites

This chart can be used with Kubernetes `1.11+` or OpenShift `3.11+` since  `CustomResourceDefinitions` are supported starting with these versions.
But the recommended Kubernetes versions are `1.16+`.

## Values

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| crds.datadogAgents | bool | `false` | Set to true to deploy the DatadogAgents CRD |
| crds.datadogMetrics | bool | `false` | Set to true to deploy the DatadogMetrics CRD |
| crds.datadogMonitors | bool | `false` | Set to true to deploy the DatadogMonitors CRD |
| fullnameOverride | string | `""` | Override the fully qualified app name |
| nameOverride | string | `""` | Override name of app |

## Developers

### How to update CRDs

```shell
./update-crds.sh <datadog-operator-tag>
```

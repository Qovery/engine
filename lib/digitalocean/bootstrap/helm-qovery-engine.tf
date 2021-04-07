data "external" "get_engine_version_to_use" {
  program = ["./helper.sh", "get_engine_version_to_use", var.qovery_engine_info.token, var.qovery_engine_info.api_fqdn, var.kubernetes_cluster_id]
}

resource "helm_release" "qovery_engine_resources" {
  name = "qovery-engine"
  chart = "common/charts/qovery-engine"
  namespace = "qovery"
  atomic = true
  create_namespace = true
  max_history = 50
  force_update = true
  timeout = 600
  recreate_pods = true

  // need kubernetes 1.18, should be well tested before activating it
  set {
    name = "autoscaler.enabled"
    value = "false"
  }

  set {
    name = "image.tag"
    value = data.external.get_engine_version_to_use.result.version
  }

  set {
    name = "volumes.storageClassName"
    value = "do-volume-standard-0"
  }

  set {
    name = "environmentVariables.QOVERY_NATS_URL"
    value = var.qovery_nats_url
  }

  set {
    name = "environmentVariables.QOVERY_NATS_USER"
    value = var.qovery_nats_user
  }

  set {
    name = "environmentVariables.QOVERY_NATS_PASSWORD"
    value = var.qovery_nats_password
  }

  set {
    name = "environmentVariables.ORGANIZATION"
    value = var.organization_id
  }

  set {
    name = "environmentVariables.CLOUD_PROVIDER"
    value = var.cloud_provider
  }

  set {
    name = "environmentVariables.REGION"
    value = var.region
  }

  set {
    name = "environmentVariables.LIB_ROOT_DIR"
    value = "/home/qovery/lib"
  }

  set {
    name = "environmentVariables.DOCKER_HOST"
    value = "tcp://0.0.0.0:2375"
  }

  # Engine Limits
  set {
    name = "engineResources.limits.cpu"
    value = "1"
  }

  set {
    name = "engineResources.requests.cpu"
    value = "500m"
  }

  set {
    name = "engineResources.limits.memory"
    value = "512Mi"
  }

  set {
    name = "engineResources.requests.memory"
    value = "512Mi"
  }

  # Build limits
  set {
    name = "buildResources.limits.cpu"
    value = "1"
  }

  set {
    name = "buildResources.requests.cpu"
    value = "500m"
  }

  set {
    name = "buildResources.limits.memory"
    value = "4Gi"
  }

  set {
    name = "buildResources.requests.memory"
    value = "4Gi"
  }

  set {
    name = "forced_upgrade"
    value = timestamp()
  }

  depends_on = [
    digitalocean_kubernetes_cluster.kubernetes_cluster,
    helm_release.prometheus-adapter,
  ]
}

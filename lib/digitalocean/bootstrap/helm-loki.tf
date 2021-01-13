resource "digitalocean_spaces_bucket" "loki_space" {
  name   = "qovery-logs-${var.kubernetes_cluster_id}"
  region = var.region
  force_destroy = true
}

resource "helm_release" "loki" {
  name = "loki"
  chart = "common/charts/loki"
  namespace = "logging"
  create_namespace = true
  atomic = true
  max_history = 50

  values = [file("chart_values/loki.yaml")]

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  set {
    name = "config.storage_config.aws.endpoint"
    value = "${var.region}.digitaloceanspaces.com"
  }

  set {
    name = "config.storage_config.aws.s3"
    value = "s3://${var.region}.digitaloceanspaces.com/qovery-logs-${var.kubernetes_cluster_id}"
  }

  set {
    name = "config.storage_config.aws.region"
    value = var.region
  }

  set {
    name = "config.storage_config.aws.access_key_id"
    value = var.space_access_id
  }

  set {
    name = "config.storage_config.aws.secret_access_key"
    value = var.space_secret_key
  }

  # Limits
  set {
    name = "resources.limits.cpu"
    value = "100m"
  }

  set {
    name = "resources.requests.cpu"
    value = "100m"
  }

  set {
    name = "resources.limits.memory"
    value = "2Gi"
  }

  set {
    name = "resources.requests.memory"
    value = "1Gi"
  }

  depends_on = [
    digitalocean_spaces_bucket.loki_space,
    digitalocean_kubernetes_cluster.kubernetes_cluster,
    helm_release.q_storageclass,
  ]
}

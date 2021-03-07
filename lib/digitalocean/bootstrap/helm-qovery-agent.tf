data "external" "get_agent_version_to_use" {
  program = ["./helper.sh", "get_agent_version_to_use", var.qovery_agent_info.token, var.qovery_agent_info.api_fqdn, var.kubernetes_cluster_id]
}

resource "random_id" "qovery_agent_id" {
  keepers = {
    # Generate a new id each time we add a new Agent id
    agent_id = var.qovery_agent_replicas
  }

  byte_length = 16
}

resource "helm_release" "qovery_agent_resources" {
  name = "qovery-agent"
  chart = "common/charts/qovery-agent"
  namespace = "qovery"
  atomic = true
  create_namespace = true
  max_history = 50
  force_update = true
  recreate_pods = true

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  set {
    name = "image.tag"
    value = data.external.get_agent_version_to_use.result.version
  }

  set {
    name = "replicaCount"
    value = random_id.qovery_agent_id.keepers.agent_id
  }

  set {
    name = "environmentVariables.AGENT_ID"
    value = random_id.qovery_agent_id.hex
  }

  set {
    name = "environmentVariables.NATS_HOST_URL"
    value = var.qovery_nats_url
  }

  set {
    name = "environmentVariables.NATS_USERNAME"
    value = var.qovery_nats_user
  }

  set {
    name = "environmentVariables.NATS_PASSWORD"
    value = var.qovery_nats_password
  }

  set {
    name = "environmentVariables.LOKI_URL"
    value = "http://loki.logging.svc.cluster.local:3100"
  }

  set {
    name = "environmentVariables.CLOUD_REGION"
    value = var.region
  }

  set {
    name = "environmentVariables.CLOUD_PROVIDER"
    value = var.cloud_provider
  }

  set {
    name = "environmentVariables.KUBERNETES_ID"
    value = var.kubernetes_cluster_id
  }

  set {
    name = "environmentVariables.RUST_LOG"
    value = "DEBUG"
  }

  # Limits
  set {
    name = "resources.limits.cpu"
    value = "1"
  }

  set {
    name = "resources.requests.cpu"
    value = "200m"
  }

  set {
    name = "resources.limits.memory"
    value = "500Mi"
  }

  set {
    name = "resources.requests.memory"
    value = "500Mi"
  }

  depends_on = [
    digitalocean_kubernetes_cluster.kubernetes_cluster
  ]
}
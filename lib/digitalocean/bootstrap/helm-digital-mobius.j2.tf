resource "helm_release" "digital_mobius" {
  name = "digital-mobius"
  chart = "charts/digital-mobius"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  set {
    name = "environmentVariables.LOG_LEVEL"
    value = "debug"
  }
  set {
    name = "environmentVariables.DELAY_NODE_CREATION"
    value = "5m"
  }

  set {
    name = "environmentVariables.DIGITAL_OCEAN_TOKEN"
    value = "{{ digitalocean_token }}"
  }

  set {
    name = "environmentVariables.DIGITAL_OCEAN_CLUSTER_ID"
    value = digitalocean_kubernetes_cluster.kubernetes_cluster.id
  }

  set {
    name = "forced_upgrade"
    value = var.forced_upgrade
  }

  depends_on = [
    digitalocean_kubernetes_cluster.kubernetes_cluster
  ]
}
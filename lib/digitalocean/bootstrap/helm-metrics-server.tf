resource "helm_release" "metrics_server" {
  name = "metrics-server"
  chart = "common/charts/metrics-server"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  set {
    name = "extraArgs.kubelet-preferred-address-types"
    value = "InternalIP"
  }

  set {
    name = "apiService.create"
    value = "true"
  }

  set {
    name = "resources.limits.cpu"
    value = "250m"
  }

  set {
    name = "resources.requests.cpu"
    value = "250m"
  }

  set {
    name = "resources.limits.memory"
    value = "256Mi"
  }

  set {
    name = "resources.requests.memory"
    value = "256Mi"
  }

  depends_on = [
    digitalocean_kubernetes_cluster.kubernetes_cluster,
    helm_release.q_storageclass,
  ]
}
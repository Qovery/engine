resource "helm_release" "q_storageclass" {
  name = "q-storageclass"
  chart = "charts/q-storageclass"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  depends_on = [
    digitalocean_kubernetes_cluster.kubernetes_cluster,
  ]
}
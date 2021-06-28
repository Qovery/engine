resource "null_resource" "kubeconfig" {
  depends_on = [scaleway_k8s_pool.kubernetes_cluster_worker] # TODO(benjaminch): use `scw_ks_worker_node in scw_ks_worker_nodes` to get all nodes names
  triggers = {
    host                   = scaleway_k8s_cluster.kubernetes_cluster.kubeconfig[0].host
    token                  = scaleway_k8s_cluster.kubernetes_cluster.kubeconfig[0].token
    cluster_ca_certificate = scaleway_k8s_cluster.kubernetes_cluster.kubeconfig[0].cluster_ca_certificate
  }
}

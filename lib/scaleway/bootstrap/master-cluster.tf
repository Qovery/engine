resource "scaleway_k8s_cluster" "kubernetes_cluster" {
  name    = "kubernetes_cluster_1" # TODO: make it qovery named: qovery-${var.kubernetes_cluster_id}
  version = "1.21.1"
  cni     = "cilium"

  autoscaler_config {
    disable_scale_down              = false
    scale_down_delay_after_add      = "5m"
    estimator                       = "binpacking"
    expander                        = "random"
    ignore_daemonsets_utilization   = true
    balance_similar_node_groups     = true
    expendable_pods_priority_cutoff = -5
  }

  tags    = ["qovery", "integration-test"]
}

resource "scaleway_k8s_pool" "kubernetes_cluster_pool" {
  cluster_id    = scaleway_k8s_cluster.kubernetes_cluster.id
  name          = "kubernetes_cluster_pool_1" # TODO: make it qovery named: qovery-${var.kubernetes_cluster_id}
  node_type     = "DEV1-L"
  autoscaling   = true
  autohealing   = true
  size          = 3
  min_size      = 3
  max_size      = 10

  tags          = ["qovery", "integration-test"]
}

resource "null_resource" "kubeconfig" {
  depends_on = [scaleway_k8s_pool.kubernetes_cluster_pool] # at least one pool here
  triggers = {
    host                   = scaleway_k8s_cluster.kubernetes_cluster.kubeconfig[0].host
    token                  = scaleway_k8s_cluster.kubernetes_cluster.kubeconfig[0].token
    cluster_ca_certificate = scaleway_k8s_cluster.kubernetes_cluster.kubeconfig[0].cluster_ca_certificate
  }
}

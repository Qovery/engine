resource "scaleway_k8s_cluster" "kubernetes_cluster" {
  name    = "qovery-${var.kubernetes_cluster_id}"
  version = "1.21.1"
  cni     = "cilium"

  tags    = ["qovery", "integration-test"] # TODO(benjaminch): put more usefull data in tags

  autoscaler_config {
    disable_scale_down              = false
    scale_down_delay_after_add      = "5m"
    estimator                       = "binpacking"
    expander                        = "random"
    ignore_daemonsets_utilization   = true
    balance_similar_node_groups     = true
    expendable_pods_priority_cutoff = -5
  }
}

resource "scaleway_k8s_pool" "kubernetes_cluster_pool" {
  cluster_id    = scaleway_k8s_cluster.kubernetes_cluster.id
  name          = "qovery-${var.kubernetes_cluster_id}"
  node_type     = "DEV1-L"

  # use Scaleway built-in cluster autoscaler
  autoscaling   = true
  autohealing   = true
  size          = "{{ scw_ks_worker_nodes[0].instance_type }}"
  min_nodes  = "{{ scw_ks_worker_nodes[0].min_size }}"
  max_nodes  = "{{ scw_ks_worker_nodes[0].max_size }}"

  tags          = ["qovery", "integration-test"] # TODO(benjaminch): put more usefull data in tags
}

resource "null_resource" "kubeconfig" {
  depends_on = [scaleway_k8s_pool.kubernetes_cluster_pool] # at least one pool here
  triggers = {
    host                   = scaleway_k8s_cluster.kubernetes_cluster.kubeconfig[0].host
    token                  = scaleway_k8s_cluster.kubernetes_cluster.kubeconfig[0].token
    cluster_ca_certificate = scaleway_k8s_cluster.kubernetes_cluster.kubeconfig[0].cluster_ca_certificate
  }
}

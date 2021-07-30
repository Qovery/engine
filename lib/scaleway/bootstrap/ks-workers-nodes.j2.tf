{% for scw_ks_worker_node in scw_ks_worker_nodes %}
resource "scaleway_k8s_pool" "kubernetes_cluster_workers_{{ loop.index }}" {
  cluster_id    = scaleway_k8s_cluster.kubernetes_cluster.id
  name          = var.kubernetes_cluster_id
  node_type     = "DEV1-L" # TODO(benjaminch): to be changed

  # use Scaleway built-in cluster autoscaler
  autoscaling   = true # TODO(benjaminch): use scaleway_ks_pool_autoscale variable
  autohealing   = true
  size          = 3 # TODO(benjaminch) : use : "{{ scw_ks_worker_node.min_size }}"
  min_size      = 3 # TODO(benjaminch) : use : "{{ scw_ks_worker_node.min_size }}"
  max_size      = 10 # TODO(benjaminch) : use : "{{ scw_ks_worker_node.max_size }}"

  tags          =  local.tags_ks_list
}
{% endfor %}
{% for scw_ks_worker_node in scw_ks_worker_nodes %}
resource "scaleway_k8s_pool" "kubernetes_cluster_workers_{{ loop.index }}" {
  cluster_id    = scaleway_k8s_cluster.kubernetes_cluster.id
  name          = var.kubernetes_cluster_id
  node_type     = "{{ scw_ks_worker_node.instance_type }}"

  # use Scaleway built-in cluster autoscaler
  autoscaling   = {{ scw_ks_pool_autoscale }}
  autohealing   = true
  size          = "{{ scw_ks_worker_node.min_size }}"
  min_size      = "{{ scw_ks_worker_node.min_size }}"
  max_size      = "{{ scw_ks_worker_node.max_size }}"

  tags          =  local.tags_ks_list
}
{% endfor %}
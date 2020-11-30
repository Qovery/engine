{% for oks_worker_node in oks_worker_nodes %}
resource "digitalocean_kubernetes_node_pool" "app_node_pool_{{ loop.index }}" {
cluster_id = digitalocean_kubernetes_cluster.kubernetes_cluster.id

name = "qovery-{{oks_cluster_id}}-{{ loop.index }}"
size = "{{ oks_worker_node.instance_type }}"

auto_scale = false
min_nodes  = "{{ oks_worker_node.min_size }}"
max_nodes  = "{{ oks_worker_node.max_size }}"

depends_on = [
digitalocean_kubernetes_cluster.kubernetes_cluster,
]
}
{% endfor %}

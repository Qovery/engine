resource "digitalocean_kubernetes_cluster" "kubernetes_cluster" {
  name = var.doks_master_name
  region = var.region
  version = var.doks_version
  vpc_uuid = digitalocean_vpc.qovery_vpc.id

  node_pool {
    tags = [digitalocean_tag.cluster_tag.id]
    name = var.doks_master_name
    size = "{{ (index .doks_worker_node 0).instance_type }}"
    auto_scale = true
    min_nodes  = "{{ (index .doks_worker_node 0).min_size }}"
    max_nodes  = "{{ (index .doks_worker_node 0).max_size }}"
  }
}

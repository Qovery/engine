resource "digitalocean_kubernetes_cluster" "kubernetes_cluster" {
  name = var.kubernetes_cluster_name
  region = var.region
  version = var.doks_version
  vpc_uuid = digitalocean_vpc.qovery_vpc.id

  # upgrade
  auto_upgrade = true
  surge_upgrade = true

  node_pool {
    tags = [digitalocean_tag.cluster_tag.id]
    name = var.kubernetes_cluster_id
    size = "{{ doks_worker_nodes[0].instance_type }}"
    # use Digital Ocean built-in cluster autoscaler
    auto_scale = true
    min_nodes  = "{{ doks_worker_nodes[0].min_size }}"
    max_nodes  = "{{ doks_worker_nodes[0].max_size }}"
  }
}
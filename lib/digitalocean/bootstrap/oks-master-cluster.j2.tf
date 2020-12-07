resource "digitalocean_kubernetes_cluster" "kubernetes_cluster" {
  name = var.kubernetes_master_cluster_name
  region = var.region
  version = var.oks_version
  vpc_uuid = digitalocean_vpc.qovery_vpc.id

  node_pool {
    name = var.oks_master_name
    size = var.oks_master_size
    auto_scale = var.oks_master_autoscale
    node_count = var.oks_master_node_count
  }
}
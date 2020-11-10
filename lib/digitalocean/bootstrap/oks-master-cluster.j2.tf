resource "digitalocean_kubernetes_cluster" "kubernetes_cluster" {
  name = var.kubernetes_master_cluster_name
  region = var.digitalocean_region
  version = var.oks_version

  node_pool {
    name = var.oks_master_name
    size = var.oks_master_size
    auto_scale = var.oks_master_autoscale
    node_count = var.oks_master_node_count
  }

  provisioner "local-exec" {
    command = "doctl kubernetes cluster kubeconfig show {{ kubernetes_master_cluster_name }} -t {{ digitalocean_token }} >> kubeconfig.yaml"
  }
}
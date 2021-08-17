locals {
  kubeconfig = <<KUBECONFIG
${digitalocean_kubernetes_cluster.kubernetes_cluster.kube_config.0.raw_config}
KUBECONFIG
}

resource "local_file" "kubeconfig" {
  filename = "${var.space_bucket_kubeconfig}/${var.kubeconfig_filename}"
  content = local.kubeconfig
  file_permission = "0644"
}
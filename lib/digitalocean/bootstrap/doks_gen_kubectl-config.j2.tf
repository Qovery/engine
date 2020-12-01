
locals {
  kubeconfig = <<KUBECONFIG
${digitalocean_kubernetes_cluster.kubernetes_cluster.kube_config.0.raw_config}
KUBECONFIG
}

resource "local_file" "kubeconfig" {
  filename = "${var.space_bucket_kubeconfig}/${var.oks_cluster_id}.yaml"
  content = local.kubeconfig
}


resource "digitalocean_spaces_bucket_object" "upload_kubeconfig" {
  region       = digitalocean_spaces_bucket.space_bucket_kubeconfig.region
  bucket       = digitalocean_spaces_bucket.space_bucket_kubeconfig.name
  key          = "${var.oks_cluster_id}.yaml"
  source       = local_file.kubeconfig.filename
  depends_on = [local_file.kubeconfig, digitalocean_spaces_bucket.space_bucket_kubeconfig]
}
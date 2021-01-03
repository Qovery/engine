resource "digitalocean_spaces_bucket" "space_bucket_kubeconfig" {
  name   = var.space_bucket_kubeconfig
  region = var.region
  force_destroy = true
}
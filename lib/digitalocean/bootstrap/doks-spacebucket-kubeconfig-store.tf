
resource "digitalocean_spaces_bucket" "space_bucket_kubeconfig" {
  name   = var.space_bucket_kubeconfig
  region = var.digitalocean_region
}

resource "digitalocean_container_registry" "qovery_registry" {
  name = var.container_registry_name
}
# TODO(benjaminch): To be handled directly in the engine to properly handle reinstall
resource "scaleway_object_bucket" "object-bucket" {
  # NOTE: name supports only alpha-numerics!
  name = "{{ object_storage_kubeconfig_bucket }}"
  acl  = "private"
  versioning {
    enabled = true
  }

  tags = local.tags_ks
}
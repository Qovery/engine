# TODO(benjaminch): To be handled directly in the engine to properly handle reinstall
resource "scaleway_object_bucket" "object-bucket" {
  # NOTE: name supports only alpha-numerics!
  name = "test-cluster" # TODO(benjaminch): use var.s3_bucket_kubeconfig
  acl  = "private"
  tags = local.tags_ks
}
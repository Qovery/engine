# TODO(benjaminch): To be handled directly in the engine to properly handle reinstall
resource "scaleway_object_bucket" "upload_kubeconfig" {
  # NOTE: name supports only alpha-numerics!
  name = "test-cluster-bucket" # TODO(benjaminch): use var.s3_bucket_kubeconfig
  acl  = "private"
  tags = local.tags_ks
}
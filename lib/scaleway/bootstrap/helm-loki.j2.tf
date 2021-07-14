// S3 bucket to store indexes and logs
// TODO(benjaminch): in order to avoid 24h before re-creating the same s3 bucket, we need to manage this from the engine
// and never delete the s3 bucket. Instead, clean the content
resource "scaleway_object_bucket" "loki_bucket" {
  name = "qovery-logs-${var.kubernetes_cluster_id}"
  acl    = "private"
  region = var.region

  versioning {
    enabled = false
  }

  tags = merge(
    local.tags_ks,
    {
      "Name" = "Applications logs"
    }
  )
}
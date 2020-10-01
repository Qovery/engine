// S3 bucket to store kubeconfigs
resource "aws_s3_bucket" "kubeconfigs_bucket" {
  bucket = var.s3_bucket_kubeconfig
  acl    = "private"
  versioning {
    enabled = true
  }

  tags = merge(
    local.tags_eks,
    {
      "Name" = "Kubernetes kubeconfig"
    }
  )
}
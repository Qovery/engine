// S3 bucket to store kubeconfigs
resource "aws_s3_bucket" "kubeconfigs_bucket" {
  bucket = "{{ region_cluster_id }}"
  acl    = "private"
  region = var.region
  versioning {
    enabled = true
  }
  tags = {
    Name        = "Kubernetes kubeconfig ${var.s3_bucket_kubeconfig}"
    Region      = var.region
  }
}

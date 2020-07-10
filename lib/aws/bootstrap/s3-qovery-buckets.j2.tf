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

// S3 bucket to store application statefiles
resource "aws_s3_bucket" "qovery-applications" {
  bucket = "qovery-applications-{{ region_cluster_id }}"
  acl    = "private"
  region = var.region
  versioning {
    enabled = true
  }
  tags = {
    Name        = "Qovery terraform customers"
    Region      = var.region
  }
}
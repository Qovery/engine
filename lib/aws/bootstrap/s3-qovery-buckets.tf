// S3 bucket to store kubeconfigs
resource "aws_s3_bucket" "kubeconfigs_bucket" {
  bucket = var.s3_bucket_kubeconfig
  acl    = "private"
  region = var.region
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

// S3 bucket to store application statefiles
resource "aws_s3_bucket" "qovery-applications" {
  bucket = aws_dynamodb_table.qovery_applications.name
  acl    = "private"
  region = var.region
  versioning {
    enabled = true
  }

  tags = merge(
    local.tags_eks,
    {
      "Name" = "Qovery terraform customers"
    }
  )
}
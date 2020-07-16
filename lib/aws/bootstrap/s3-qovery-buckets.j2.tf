// S3 bucket to store kubeconfigs
resource "aws_s3_bucket" "kubeconfigs_bucket" {
  bucket = var.eks_cluster_id
  acl    = "private"
  region = var.region
  versioning {
    enabled = true
  }

  tags = merge(
    aws_eks_cluster.eks_cluster.tags,
    {
      "Name" = "Kubernetes kubeconfig"
    }
  )
}

// S3 bucket to store application statefiles
resource "aws_s3_bucket" "qovery-applications" {
  bucket = aws_dynamodb_table.qovery-applications.name
  acl    = "private"
  region = var.region
  versioning {
    enabled = true
  }

  tags = merge(
    aws_eks_cluster.eks_cluster.tags,
    {
      "Name" = "Qovery terraform customers"
    }
  )
}
resource "aws_dynamodb_table" "qovery_applications" {
  name = "q-applications-${var.eks_cluster_id}"
  hash_key = "LockID"
  billing_mode = "PAY_PER_REQUEST"
  attribute {
    name = "LockID"
    type = "S"
  }

  tags = local.tags_eks
}

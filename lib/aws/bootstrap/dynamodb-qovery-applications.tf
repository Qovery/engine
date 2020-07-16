resource "aws_dynamodb_table" "qovery-applications" {
  name = "q-applications-${var.eks_cluster_id}"
  hash_key = "LockID"
  billing_mode = "PAY_PER_REQUEST"
  attribute {
    name = "LockID"
    type = "S"
  }

  tags = aws_eks_cluster.eks_cluster.tags
}

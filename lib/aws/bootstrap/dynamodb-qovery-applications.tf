resource "aws_dynamodb_table" "qovery-applications" {
  name = "qovery-applications-${var.region_cluster_name}"
  hash_key = "LockID"
  billing_mode = "PAY_PER_REQUEST"
  attribute {
    name = "LockID"
    type = "S"
  }
}

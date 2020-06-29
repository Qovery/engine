resource "aws_ecr_repository" "qovery-repo" {
  name = var.ecr_name
}
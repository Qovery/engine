terraform {
  backend "s3" {
    access_key = "{{ aws_access_key }}"
    secret_key = "{{ aws_secret_key }}"
    bucket = "{{ aws_terraform_backend_bucket }}"
    key = "${var.eks_cluster_id}.tfstate"
    dynamodb_table = "{{ aws_terraform_backend_dynamodb_table }}"
    region = var.region
  }
}
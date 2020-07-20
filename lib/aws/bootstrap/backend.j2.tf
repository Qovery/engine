terraform {
  backend "s3" {
    access_key = "{{ aws_access_key }}"
    secret_key = "{{ aws_secret_key }}"
    bucket = "{{ aws_terraform_backend_bucket }}"
    key = "{{ aws_terraform_backend_bucket }}.tfstate"
    dynamodb_table = "{{ aws_terraform_backend_dynamodb_table }}"
    region = "{{ aws_region }}"
  }
}
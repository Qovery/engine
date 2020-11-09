terraform {
  backend "s3" {
    access_key = "{{ aws_access_key_tfstates_account }}"
    secret_key = "{{ aws_secret_key_tfstates_account }}"
    bucket = "{{ aws_terraform_backend_bucket }}"
    key = "{{ oks_cluster_id }}/{{ aws_terraform_backend_bucket }}.tfstate"
    dynamodb_table = "{{ aws_terraform_backend_dynamodb_table }}"
    region = "{{ aws_region_tfstates_account }}"
  }
}
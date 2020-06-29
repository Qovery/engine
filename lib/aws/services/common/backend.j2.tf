terraform {
  backend "s3" {
    bucket = "{{ qovery_env.region }}-qovery-terraform-customers"
    key = "{{ service_info['fqdn_id'] }}.tfstate"
    dynamodb_table = "{{ qovery_env.region }}-{{ qovery_env.cluster_name }}-terraform-customers"
    region = "{{ qovery_env.region }}"
  }
}

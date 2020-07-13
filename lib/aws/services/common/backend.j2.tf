terraform {
  backend "s3" {
    bucket = "{{ region }}-qovery-terraform-customers"
    key = "{{ service_info['fqdn_id'] }}.tfstate"
    dynamodb_table = "{{ region }}-{{ cluster_name }}-terraform-customers"
    region = "{{ region }}"
  }
}

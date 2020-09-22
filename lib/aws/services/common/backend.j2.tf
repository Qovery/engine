terraform {
  backend "s3" {
    bucket = "q-applications-{{eks_cluster_id}}"
    key = "{{ fqdn_id }}.tfstate"
    dynamodb_table = "q-applications-{{eks_cluster_id}}"
    region = "{{ region }}"
  }
}

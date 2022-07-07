data "aws_caller_identity" "current" {}

locals {
  database_tf_config = <<TF_CONFIG
{
  "database_target_id": "${aws_db_instance.postgresql_instance.id}",
  "database_target_hostname": "${aws_db_instance.postgresql_instance.address}",
  "database_target_fqdn_id": "{{ fqdn_id }}",
  "database_target_fqdn": "{{ fqdn }}"
}
TF_CONFIG
}

resource "local_file" "database_tf_config" {
  filename = "database-tf-config.json"
  content = local.database_tf_config
  file_permission = "0600"
}

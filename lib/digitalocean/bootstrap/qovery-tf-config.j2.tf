locals {
  qovery_tf_config = <<TF_CONFIG
{
  "loki_storage_config_do_space_access_id": "${var.space_access_id}",
  "loki_storage_config_do_space_secret_key": "${var.space_secret_key}",
  "loki_storage_config_do_space_region": "${var.region}",
  "loki_storage_config_do_space_host": "https://${var.region}.digitaloceanspaces.com",
  "loki_storage_config_do_space_bucket_name": "{{ object_storage_logs_bucket }}"
}
TF_CONFIG
}

resource "local_file" "qovery_tf_config" {
  filename = "qovery-tf-config.json"
  content = local.qovery_tf_config
  file_permission = "0600"
}

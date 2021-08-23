locals {
  qovery_tf_config = <<TF_CONFIG
{
  "loki_storage_config_do_space": "s3://${urlencode(var.space_access_id)}:${urlencode(var.space_secret_key)}@${var.region}/{{ object_storage_logs_bucket }}/${var.kubernetes_cluster_id}"
}
TF_CONFIG
}

resource "local_file" "qovery_tf_config" {
  filename = "qovery-tf-config.json"
  content = local.qovery_tf_config
  file_permission = "0600"
}

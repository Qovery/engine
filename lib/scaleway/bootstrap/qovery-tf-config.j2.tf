locals {
  qovery_tf_config = <<TF_CONFIG
{
  "loki_storage_config_scaleway_s3": "s3://${urlencode(var.scaleway_access_key)}:${urlencode(var.scaleway_secret_key)}@{{ object_storage_logs_bucket }}.s3.${var.region}.scw.cloud"
}
TF_CONFIG
}

resource "local_file" "qovery_tf_config" {
  filename = "qovery-tf-config.json"
  content = local.qovery_tf_config
  file_permission = "0600"
}

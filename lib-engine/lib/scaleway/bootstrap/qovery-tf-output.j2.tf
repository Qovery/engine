output "loki_storage_config_scaleway_s3" { value = "s3://${urlencode(var.scaleway_access_key)}:${urlencode(var.scaleway_secret_key)}@s3.${var.region}.scw.cloud/{{ object_storage_logs_bucket }}" }

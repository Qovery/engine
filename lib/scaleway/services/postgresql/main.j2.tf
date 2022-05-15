locals {
  tags_postgresql = {
    cluster_name = var.cluster_name
    cluster_id = var.kubernetes_cluster_id
    region = var.region
    q_client_id = var.q_customer_id
    q_environment_id = var.q_environment_id
    q_project_id = var.q_project_id
    database_identifier = var.postgresql_identifier
    {% if resource_expiration_in_seconds is defined %}ttl = var.resource_expiration_in_seconds{% endif %}
    {% if snapshot is defined and snapshot["snapshot_id"] %}meta_last_restored_from = var.snapshot_identifier{% endif %}
  }
  tags_postgresql_list = [for i, v in local.tags_postgresql : "${i}=${v}"] # NOTE: Scaleway doesn't support KV style tags
}

{%- if publicly_accessible != false %}
resource "scaleway_rdb_acl" "main" {
  instance_id = scaleway_rdb_instance.postgresql_instance.id
  # TODO(benjaminch): Allow only Scaleway's private traffic
  acl_rules {
    ip = "0.0.0.0/0"
    description = "accessible from any host"
  }
  depends_on = [
    scaleway_rdb_instance.postgresql_instance
  ]
}
{% else %}
resource "scaleway_rdb_acl" "main" {
  instance_id = scaleway_rdb_instance.postgresql_instance.id
  acl_rules {
    ip = "0.0.0.0/0"
    description = "accessible from any host"
  }
  depends_on = [
    scaleway_rdb_instance.postgresql_instance
  ]
}
{% endif %}

resource "scaleway_rdb_instance" "postgresql_instance" {
  name              = var.database_name
  engine            = "PostgreSQL-${var.postgresql_version_major}"

  node_type         = var.instance_class
  volume_type       = var.storage_type
  volume_size_in_gb = var.disk_size

  is_ha_cluster     = var.activate_high_availability
  disable_backup    = !var.activate_backups

  user_name         = var.username
  password          = var.password

  region            = var.region

  tags              = local.tags_postgresql_list

# TODO:(benjaminch): features to be added at some point but be discussed with Scaleway
  # - port
  # - instance create timeout
  # - instance update timeout
  # - instance delete timeout
  # - snapshot id for restore: maybe should use volume ? https://registry.terraform.io/providers/scaleway/scaleway/latest/docs/resources/instance_volume => Ask them how to do it
  # - db_subnet_group_name: not sure we can customize it? => Ok
  # - vpc_security_group_ids: not sure we can customize it? => Ok
  # - multi_az: not sure we can customize it? => Ok
  # - maintenance apply_immediately: not sure we can customize it? => Ok
  # - maintenance maintenance_window: not sure we can customize it? => Ok
  # - monitoring_interval: not sure we can customize it? => Ok
  # - monitoring_role_arn: not sure we can customize it? => Ok
  # - backup backup_retention_period: not sure we can customize it? => Ok
  # - backup backup_window: not sure we can customize it? => Ok
  # - backup skip_final_snapshot: not sure we can customize it? => Ok
  # - backup delete_automated_backups: not sure we can customize it? => Ok

  # available settings to be retrieved via API
  # https://developers.scaleway.com/en/products/rdb/api/#get-1eafb7
  # https://api.scaleway.com/rdb/v1/regions/fr-par/database-engines
  # settings          = {} TODO(benjaminch): to activate slow queries logs, but not possible for now via `log_min_duration_statement`
}

resource "scaleway_rdb_database" "postgresql_main" {
  instance_id    = scaleway_rdb_instance.postgresql_instance.id
  name           = var.database_name
}

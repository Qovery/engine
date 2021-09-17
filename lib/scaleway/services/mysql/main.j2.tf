locals {
  tags_mysql = {
    cluster_name = var.cluster_name
    cluster_id = var.kubernetes_cluster_id
    region = var.region
    q_client_id = var.q_customer_id
    q_environment_id = var.q_environment_id
    q_project_id = var.q_project_id
    database_identifier = var.mysql_identifier
    {% if resource_expiration_in_seconds is defined %}ttl = var.resource_expiration_in_seconds{% endif %}
    {% if snapshot is defined and snapshot["snapshot_id"] %}meta_last_restored_from = var.snapshot_identifier{% endif %}
  }
  tags_mysql_list = [for i, v in local.tags_mysql : "${i}=${v}"] # NOTE: Scaleway doesn't support KV style tags
}

# By default, all IPs are authorized => 0.0.0.0/0
{%- if publicly_accessible %}
resource "scaleway_rdb_acl" "main" {
  instance_id = scaleway_rdb_instance.mysql_instance.id
  acl_rules {
    ip = "0.0.0.0/0"
    description = "accessible from any host"
  }
  depends_on [
    scaleway_rdb_instance.mysql_instance
  ]
}
{%- else %}

{%- endif %}

resource "scaleway_rdb_instance" "mysql_instance" {
  name              = var.database_name
  engine            = "MySQL-${var.mysql_version_major}"

  node_type         = var.instance_class
  # volume_type       = var.storage_type
  # volume_size_in_gb = var.disk_size

  is_ha_cluster     = var.is_ha_cluster
  disable_backup    = false # TODO(benjaminch): plug backup options (CF AWS)

  user_name         = var.username
  password          = var.password

  region            = var.region

  tags              = local.tags_mysql_list

  # TODO:(benjaminch): To be discussed with Pierre
  # missing configuration from DB
  # - port: not sure we can customize it? => Will do without
  # - create timeout: not sure we can customize it?
  # - update timeout: not sure we can customize it?
  # - delete timeout: not sure we can customize it?
  # - snapshot id for restore: maybe should use volume ? https://registry.terraform.io/providers/scaleway/scaleway/latest/docs/resources/instance_volume => Ask them how to do it
  # - certificate: not sure we can customize it? => Ok
  # - db_subnet_group_name: not sure we can customize it? => Ok
  # - vpc_security_group_ids: not sure we can customize it? => Ok
  # - publicly_accessible: Should be done via ACL? => Check K8s CNI / Core will send all subnets so we will have access / test if accessible from outside without acl or should we set one large one / Whitelist Scaleway private ID range
  # - multi_az: not sure we can customize it? => Ok
  # - maintenance apply_immediately: not sure we can customize it? => Ok
  # - maintenance maintenance_window: not sure we can customize it? => Ok
  # - auto_minor_version_upgrade: not needed, they handle minors => Ok
  # - monitoring_interval: not sure we can customize it? => Ok
  # - monitoring_role_arn: not sure we can customize it? => Ok
  # - backup backup_retention_period: not sure we can customize it? => Ok
  # - backup backup_window: not sure we can customize it? => Ok
  # - backup skip_final_snapshot: not sure we can customize it? => Ok
  # - backup delete_automated_backups: not sure we can customize it? => Ok

  # available settings to be retrieved via API
  # https://developers.scaleway.com/en/products/rdb/api/#get-1eafb7
  # https://api.scaleway.com/rdb/v1/regions/fr-par/database-engines
  # settings          = { }
  # - activate_slow_queries
}

resource "scaleway_rdb_database" "mysql_main" {
  instance_id    = scaleway_rdb_instance.mysql_instance.id
  name           = var.database_name
}
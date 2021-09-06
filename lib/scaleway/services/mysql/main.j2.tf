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

resource "scaleway_rdb_user" "db_admin" {
  instance_id = scaleway_rdb_instance.mysql_instance.id
  name        = var.username
  password    = var.password
  is_admin    = true
}

resource "scaleway_rdb_instance" "mysql_instance" {
  name              = var.database_name
  node_type         = var.instance_class
  engine            = "MySQL-${var.mysql_version}"
  # volume_type       = var.storage_type
  # volume_size_in_gb = var.disk_size
  is_ha_cluster     = true
  disable_backup    = true
  user_name         = var.username
  password          = var.password
  # settings: TODO(benjaminch): check what needs to be set here
  region            = var.region
  tags              = local.tags_mysql_list

}

resource "scaleway_rdb_database" "mysql_main" {
  instance_id    = scaleway_rdb_instance.mysql_instance.id
  name           = var.database_name
}
data "aws_vpc" "selected" {
  filter {
    name = "tag:ClusterId"
    values = [var.kubernetes_cluster_id]
  }
}

data "aws_subnet_ids" "k8s_subnet_ids" {
  vpc_id = data.aws_vpc.selected.id
  filter {
    name = "tag:ClusterId"
    values = [var.kubernetes_cluster_id]
  }
  filter {
    name = "tag:Service"
    values = ["RDS"]
  }
}

data "aws_security_group" "selected" {
  {%- if not user_provided_network -%}
  filter {
    name = "tag:Name"
    values = ["qovery-eks-workers"]
  }
  {%- endif %}

  filter {
    name   = "tag:kubernetes.io/cluster/qovery-${var.kubernetes_cluster_id}"
    values = ["owned"]
  }
}

data "aws_iam_role" "rds_enhanced_monitoring" {
  name = "qovery-rds-enhanced-monitoring-${var.kubernetes_cluster_id}"
}

# /!\ DO NOT REMOVE: adding a timestamp to final snapshot in order to avoid duplicate which triggers a tf error. /!\
locals {
  final_snap_timestamp = replace(timestamp(), "/[- TZ:]/", "")
  final_snapshot_name = "${var.final_snapshot_name}-${local.final_snap_timestamp}"
}

# Non snapshoted version
resource "aws_db_instance" "postgresql_instance" {
  identifier = var.postgresql_identifier

  tags = local.postgres_database_tags

  # Postgres instance basics
  instance_class = var.instance_class
  port = var.port
  timeouts {
    create = "60m"
    update = "120m"
    delete = "60m"
  }
  password = var.password
  storage_encrypted = var.encrypt_disk
  {%- if snapshot and snapshot["snapshot_id"] %}
  # Snapshot
  snapshot_identifier = var.snapshot_identifier
  {%- else %}
  allocated_storage = var.disk_size
  name = var.database_name
  storage_type = var.storage_type
  username = var.username
  engine_version = var.postgresql_version
  engine = "postgres"
  ca_cert_identifier = "rds-ca-2019"
  {%- endif %}

  # Network
  db_subnet_group_name = data.aws_subnet_ids.k8s_subnet_ids.id
  vpc_security_group_ids = data.aws_security_group.selected.*.id
  publicly_accessible = var.publicly_accessible
  multi_az = var.multi_az

  # Maintenance and upgrades
  apply_immediately = var.apply_changes_now
  auto_minor_version_upgrade = var.auto_minor_version_upgrade
  maintenance_window = var.preferred_maintenance_window

  # Monitoring
  performance_insights_enabled = var.performance_insights_enabled
  performance_insights_retention_period = var.performance_insights_enabled_retention
  monitoring_interval = 10
  monitoring_role_arn = data.aws_iam_role.rds_enhanced_monitoring.arn

  # Backups
  backup_retention_period = var.backup_retention_period
  backup_window = var.preferred_backup_window
  skip_final_snapshot = var.skip_final_snapshot
  {%- if not skip_final_snapshot %}
  final_snapshot_identifier = local.final_snapshot_name
  lifecycle {
    ignore_changes = [
      final_snapshot_identifier,
    ]
  }
  {%- endif %}
  copy_tags_to_snapshot = true
  delete_automated_backups = var.delete_automated_backups

}

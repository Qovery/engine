data "aws_vpc" "selected" {
  filter {
    name = "tag:ClusterId"
    values = [var.kubernetes_cluster_id]
  }
}

data "aws_security_group" "selected" {
  {% if not user_provided_network %}
  filter {
     name = "tag:Name"
     values = ["qovery-eks-workers"]
   }
  {% endif %}

  filter {
    name   = "tag:kubernetes.io/cluster/qovery-${var.kubernetes_cluster_id}"
    values = ["owned"]
  }
}

# /!\ DO NOT REMOVE: adding a timestamp to final snapshot in order to avoid duplicate which triggers a tf error. /!\
locals {
  final_snap_timestamp = replace(timestamp(), "/[- TZ:]/", "")
  final_snapshot_name = "${var.final_snapshot_name}-${local.final_snap_timestamp}"
}

resource "aws_docdb_cluster_instance" "documentdb_cluster_instances" {
  count              = var.documentdb_instances_number

  cluster_identifier = aws_docdb_cluster.documentdb_cluster.id
  identifier         = "${var.documentdb_identifier}-${count.index}"

  instance_class     = var.instance_class

  # Maintenance and upgrade
  auto_minor_version_upgrade = var.auto_minor_version_upgrade
  preferred_maintenance_window = var.preferred_maintenance_window

  tags = local.mongodb_database_tags
}

resource "aws_docdb_cluster" "documentdb_cluster" {
  cluster_identifier = var.documentdb_identifier

  tags = local.mongodb_database_tags

  # DocumentDB instance basics
  port = var.port
  timeouts {
    create = "60m"
    update = "120m"
    delete = "60m"
  }

  master_password = var.password
  {%- if snapshot is defined and snapshot["snapshot_id"] %}
  # Snapshot
  snapshot_identifier = var.snapshot_identifier
  {%- else %}
  master_username = var.username
  engine = "docdb"
  {%- endif %}
  storage_encrypted = var.encrypt_disk

  # Network
  availability_zones = var.kubernetes_cluster_az_list
  {%- if database_docdb_subnet_use_old_group_name %}
  db_subnet_group_name = data.aws_vpc.selected.id
  {% else %}
  db_subnet_group_name = "documentdb-${data.aws_vpc.selected.id}"
  {% endif %}
  vpc_security_group_ids = data.aws_security_group.selected.*.id

  # Maintenance and upgrades
  apply_immediately = var.apply_changes_now

  # Backups
  backup_retention_period = var.backup_retention_period
  preferred_backup_window = var.preferred_backup_window
  skip_final_snapshot = var.skip_final_snapshot
  {%- if not skip_final_snapshot %}
  final_snapshot_identifier = local.final_snapshot_name
  lifecycle {
    ignore_changes = [
      final_snapshot_identifier,
    ]
  }
  {%- endif %}
}

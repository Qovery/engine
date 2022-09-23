data "aws_vpc" "selected" {
  filter {
    name = "tag:ClusterId"
    values = [var.kubernetes_cluster_id]
  }
}

data "aws_subnet_ids" "selected" {
  vpc_id = data.aws_vpc.selected.id
  filter {
    name = "tag:ClusterId"
    values = [var.kubernetes_cluster_id]
  }
  filter {
    name = "tag:Service"
    values = ["Elasticache"]
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

# /!\ DO NOT REMOVE: adding a timestamp to final snapshot in order to avoid terraform creating duplicated snapshot which triggers a tf error. /!\
locals {
  final_snap_timestamp = replace(timestamp(), "/[- TZ:]/", "")
  final_snapshot_name = "${var.final_snapshot_name}-${local.final_snap_timestamp}"
}

{%- if database_elasticache_parameter_group_name == 'default.redis5.0' or database_login == 'qoveryadmin'%}
resource "aws_elasticache_cluster" "elasticache_cluster" {
  cluster_id = var.elasticache_identifier

  tags = local.redis_database_tags

  # Elasticache instance basics
  port = var.port
  engine_version = var.elasticache_version
  # Thanks GOD AWS for not using SemVer and adding your own versioning system,
  # need to add this dirty trick while Hashicorp fix this issue
  # https://github.com/hashicorp/terraform-provider-aws/issues/15625
  lifecycle {
    ignore_changes = [engine_version {%- if not skip_final_snapshot %}, final_snapshot_identifier{%- endif %}]
  }

  engine = "redis"
  node_type = var.instance_class
  num_cache_nodes = var.elasticache_instances_number
  parameter_group_name = var.parameter_group_name

{%- if snapshot is defined and snapshot["snapshot_id"] %}
  # Snapshot
  snapshot_name = var.snapshot_identifier
{%- endif %}

  # Network
  # WARNING: this value cna't get fetch from data sources and is linked to the bootstrap phase
  subnet_group_name = "elasticache-${data.aws_vpc.selected.id}"

  # Security
  security_group_ids = data.aws_security_group.selected.*.id

  # Maintenance and upgrades
  apply_immediately = var.apply_changes_now
  maintenance_window = var.preferred_maintenance_window

  # Backups
  snapshot_window = var.preferred_backup_window
  snapshot_retention_limit = var.backup_retention_period
  {%- if not skip_final_snapshot %}
  final_snapshot_identifier = local.final_snapshot_name
  {%- endif %}
}
{%- else %}
resource "aws_elasticache_replication_group" "elasticache_cluster" {
  replication_group_id          = var.elasticache_identifier
  replication_group_description = "Qovery's elasticache"

  # Elasticache instance basics
  node_type = var.instance_class
  port = var.port
  parameter_group_name = var.parameter_group_name


{%- if database_elasticache_instances_number > 1 %}
  multi_az_enabled = true
  num_node_groups = var.elasticache_instances_number
  replicas_per_node_group = 1
{%- else %}
  number_cache_clusters       =  var.elasticache_instances_number
{%- endif %}

  tags = local.redis_database_tags

  # Elasticache auth
  transit_encryption_enabled = true
  auth_token = var.password

  # Network
  # WARNING: this value cna't get fetch from data sources and is linked to the bootstrap phase
  subnet_group_name = "elasticache-${data.aws_vpc.selected.id}"

  # Security
  security_group_ids = data.aws_security_group.selected.*.id

  # Maintenance and upgrades
  apply_immediately = var.apply_changes_now
  maintenance_window = var.preferred_maintenance_window

  # Backups
  snapshot_window = var.preferred_backup_window
  snapshot_retention_limit = var.backup_retention_period
{%- if not skip_final_snapshot %}
  final_snapshot_identifier = var.final_snapshot_name
{%- endif %}

{%- if snapshot is defined and snapshot["snapshot_id"] %}
# Snapshot
  snapshot_name = var.snapshot_identifier
{%- endif %}
}
{%- endif %}



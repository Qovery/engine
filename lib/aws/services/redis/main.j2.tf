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
  filter {
    name = "tag:Name"
    values = ["qovery-eks-workers"]
  }
  filter {
    name   = "tag:kubernetes.io/cluster/${var.kubernetes_cluster_id}"
    values = ["owned"]
  }
}

resource "helm_release" "elasticache_instance_external_name" {
  name = "${aws_elasticache_cluster.elasticache_cluster.id}-externalname"
  chart = "external-name-svc"
  namespace = "{{namespace}}"
  atomic = true
  max_history = 50

  set {
    name = "target_hostname"
    value = aws_elasticache_cluster.elasticache_cluster.cache_nodes.0.address
  }

  set {
    name = "source_fqdn"
    value = "{{database_fqdn}}"
  }

  set {
    name = "app_id"
    value = "{{database_id}}"
  }

  set {
    name= "publicly_accessible"
    value= {{ publicly_accessible }}
  }

  depends_on = [
    aws_elasticache_cluster.elasticache_cluster
  ]
}

resource "aws_elasticache_cluster" "elasticache_cluster" {
  cluster_id = var.elasticache_identifier

  tags = {
    cluster_name = var.cluster_name
    region = var.region
    q_client_id = var.q_customer_id
    q_environment_id = var.q_environment_id
    q_project_id = var.q_project_id
    database_identifier = var.elasticache_identifier
    {% if resource_expiration_in_seconds is defined %}ttl = var.resource_expiration_in_seconds{% endif %}
  }

  # Elasticache instance basics
  port = var.port
  engine_version = var.elasticache_version
  # Thanks GOD AWS for not using SemVer and adding your own versioning system,
  # need to add this dirty trick while Hashicorp fix this issue
  # https://github.com/hashicorp/terraform-provider-aws/issues/15625
  lifecycle {
    ignore_changes = [engine_version]
  }

  {%- if replication_group_id is defined %}
  # todo: add cluster mode and replicas support
  {%- else %}
  engine = "redis"
  node_type = var.instance_class
  num_cache_nodes = var.elasticache_instances_number
  parameter_group_name = var.parameter_group_name
  {%- endif %}

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
  final_snapshot_identifier = var.final_snapshot_name
  {%- endif %}

}

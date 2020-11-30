data "aws_vpc" "selected" {
  filter {
    name = "tag:ClusterId"
    values = [var.eks_cluster_id]
  }
}

data "aws_subnet_ids" "k8s_subnet_ids" {
  vpc_id = data.aws_vpc.selected.id
  filter {
    name = "tag:ClusterId"
    values = [var.eks_cluster_id]
  }
  filter {
    name = "tag:Service"
    values = ["DocumentDB"]
  }
}

data "aws_security_group" "selected" {
  filter {
    name = "tag:Name"
    values = ["qovery-eks-workers"]
  }
  filter {
    name   = "tag:kubernetes.io/cluster/${var.eks_cluster_id}"
    values = ["owned"]
  }
}

resource "helm_release" "documentdb_instance_external_name" {
  name = "${aws_docdb_cluster.documentdb_cluster.id}-externalname"
  chart = "external-name-svc"
  namespace = "{{namespace}}"
  atomic = true
  max_history = 50

  set {
    name = "target_hostname"
    value = aws_docdb_cluster.documentdb_cluster.endpoint
  }

  set {
    name = "source_fqdn"
    value = "{{database_fqdn}}"
  }

  set {
    name = "app_id"
    value = "{{database_id}}"
  }

  depends_on = [
    aws_docdb_cluster.documentdb_cluster
  ]
}

resource "aws_docdb_cluster_instance" "documentdb_cluster_instances" {
  count              = var.documentdb_instances_number

  cluster_identifier = aws_docdb_cluster.documentdb_cluster.id
  identifier         = "${var.documentdb_identifier}-${count.index}"

  instance_class     = var.instance_class

  # Maintenance and upgrade
  auto_minor_version_upgrade = var.auto_minor_version_upgrade
  preferred_maintenance_window = var.preferred_maintenance_window

  tags = {
    cluster_name = var.cluster_name
    region = var.region
    q_client_id = var.q_customer_id
    q_environment_id = var.q_environment_id
    q_project_id = var.q_project_id
    database_identifier = var.documentdb_identifier
  }
}

resource "aws_docdb_cluster" "documentdb_cluster" {
  cluster_identifier = var.documentdb_identifier

  tags = {
    cluster_name = var.cluster_name
    region = var.region
    q_client_id = var.q_customer_id
    q_environment_id = var.q_environment_id
    q_project_id = var.q_project_id
    database_identifier = var.documentdb_identifier
    {% if snapshot is defined and snapshot["snapshot_id"] %}meta_last_restored_from = var.snapshot_identifier{% endif %}
  }

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

  # Network
  db_subnet_group_name = data.aws_subnet_ids.k8s_subnet_ids.id
  vpc_security_group_ids = data.aws_security_group.selected.*.id

  # Maintenance and upgrades
  apply_immediately = var.apply_changes_now

  # Backups
  backup_retention_period = var.backup_retention_period
  preferred_backup_window = var.preferred_backup_window
  skip_final_snapshot = true
}

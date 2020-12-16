# Qovery

variable "cluster_name" {
  description = "Kubernetes cluster name"
  default = "{{ cluster_name }}"
  type = string
}

variable "region" {
  description = "AWS region to store terraform state and lock"
  default = "{{ region }}"
  type = string
}

variable "eks_cluster_id" {
  description = "Kubernetes cluster name with region"
  default     = "{{ eks_cluster_id }}"
  type        = string
}

variable "region_cluster_name" {
  description = "AWS region to store terraform state and lock"
  default = "{{ region }}-{{ cluster_name }}"
  type = string
}

variable "q_project_id" {
  description = "Qovery project ID"
  default = "{{ project_id }}"
  type = string
}

variable "q_customer_id" {
  description = "Qovery customer ID"
  default = "{{ owner_id }}"
  type = string
}

variable "q_environment_id" {
  description = "Qovery client environment"
  default = "{{ environment_id }}"
  type = string
}

# elasticache instance basics

variable "elasticache_identifier" {
  description = "Elasticache cluster name (Cluster identifier)"
  default = "{{ fqdn_id }}"
  type = string
}

variable "elasticache_version" {
  description = "Elasticache version"
  default = "{{ version }}"
  type = string
}

variable "parameter_group_name" {
  description = "Elasticache parameter group name"
  default = "{{ database_elasticache_parameter_group_name }}"
  type = string
}

variable "elasticache_instances_number" {
  description = "Elasticache instance numbers"
  default = 1
  type = number
}

variable "port" {
  description = "Elasticache instance port"
  default = {{ database_port }}
  type = number
}

variable "instance_class" {
  description = "Type of instance: https://docs.aws.amazon.com/AmazonElastiCache/latest/red-ug/CacheNodes.SupportedTypes.html"
  default = "{{database_instance_type}}"
  type = string
}

# Upgrades

variable "auto_minor_version_upgrade" {
  description = "Indicates that minor engine upgrades will be applied automatically to the DB instance during the maintenance window"
  default = true
  type = bool
}

variable "apply_changes_now" {
  description = "Apply changes now or during the during the maintenance window"
  default = true
  type = bool
}

variable "preferred_maintenance_window" {
  description = "Maintenance window"
  default = "Tue:02:00-Tue:04:00"
  type = string
}

# Backups

variable "backup_retention_period" {
  description = "Backup rentention period"
  default = 7
  type = number
}

variable "preferred_backup_window" {
  description = "Maintenance window"
  default = "00:00-01:00"
  type = string
}

{%- if snapshot is defined %}
# Snapshots
variable "snapshot_identifier" {
  description = "Snapshot ID to restore"
  default = "{{ snapshot['snapshot_id']}}"
  type = string
}
{% endif %}

{%- if resource_expiration_in_seconds is defined %}
# Pleco ttl
variable "resource_expiration_in_seconds" {
  description = "Resource expiration in seconds"
  default = {{ resource_expiration_in_seconds }}
  type = number
}
{% endif %}

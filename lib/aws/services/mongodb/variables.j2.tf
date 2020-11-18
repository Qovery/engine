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

# documentdb instance basics

variable "documentdb_identifier" {
  description = "Documentdb cluster name (Cluster identifier)"
  default = "{{ fqdn_id }}"
  type = string
}

variable "documentdb_instances_number" {
  description = "DocumentDB instance numbers"
  default = 1
  type = number
}

variable "port" {
  description = "Documentdb instance port"
  default = {{ database_port }}
  type = number
}

variable "instance_class" {
  description = "Type of instance: https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/Concepts.DBInstanceClass.html"
  default = "{{database_instance_type}}"
  type = string
}

variable "username" {
  description = "Admin username for the master DB user"
  default = "{{ database_login }}"
  type = string
}

variable "password" {
  description = "Admin password for the master DB user"
  default = "{{ database_password }}"
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
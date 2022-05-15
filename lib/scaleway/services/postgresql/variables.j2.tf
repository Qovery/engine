# Qovery

variable "cluster_name" {
  description = "Kubernetes cluster name"
  default     = "{{ cluster_name }}"
  type        = string
}

variable "region" {
  description = "SCW region to store terraform state and lock"
  default     = "{{ region }}"
  type        = string
}

variable "zone" {
  description = "SCW zone to store terraform state and lock"
  default     = "{{ zone }}"
  type        = string
}

variable "kubernetes_cluster_id" {
  description = "Kubernetes cluster name with region"
  default     = "{{ kubernetes_cluster_id }}"
  type        = string
}

variable "region_cluster_name" {
  description = "SCW region to store terraform state and lock"
  default     = "{{ region }}-{{ cluster_name }}"
  type        = string
}

variable "q_project_id" {
  description = "Qovery project ID"
  default     = "{{ project_id }}"
  type        = string
}

variable "q_customer_id" {
  description = "Qovery customer ID"
  default     = "{{ owner_id }}"
  type        = string
}

variable "q_environment_id" {
  description = "Qovery client environment"
  default     = "{{ environment_id }}"
  type        = string
}

# PostgreSQL instance basics

variable "postgresql_identifier" {
  description = "PostgreSQL instance name (DB identifier)"
  default = "{{ fqdn_id }}"
  type = string
}

variable "port" {
  description = "PostgreSQL instance port"
  default = {{ database_port }}
  type = number
}

variable "disk_size" {
  description = "disk instance size"
  default = {{ database_disk_size_in_gib }}
  type = number
}

variable "postgresql_version" {
  description = "PostgreSQL version"
  default = "{{ version }}"
  type = string
}

variable "postgresql_version_major" {
  description = "PostgreSQL version major"
  default = "{{ version_major }}"
  type = string
}

variable "storage_type" {
  description = "One of lssd or bssd."
  default = "{{ database_disk_type }}"
  type = string
}

variable "instance_class" {
  description = "Type of instance: https://www.scaleway.com/fr/tarifs/"
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

variable "database_name" {
  description = "The name of the database to create when the DB instance is created. If this parameter is not specified, no database is created in the DB instance"
  default = "{{ database_name }}"
  type = string
}

# Network

variable "publicly_accessible" {
  description = "Instance publicly accessible"
  default = {{ publicly_accessible }}
  type = bool
}

variable "multi_az" {
  description = "Multi availability zones"
  default = true
  type = bool
}

# Upgrades

variable "upgrade_minor" {
  description = "Automatic minor version upgrade during window maintenance"
  default = true
  type = bool
}

variable "apply_changes_now" {
  description = "Apply changes now or during the during the maintenance window"
  default = false
  type = bool
}

variable "maintenance_window" {
  description = "Maintenance window"
  default = "Tue:02:00-Tue:04:00"
  type = string
}

# Backups

variable "activate_backups" {
  description = "Backups activated"
  default = {{ activate_backups }}
  type = bool
}

variable "backup_retention_period" {
  description = "Backup rentention period"
  default = 7
  type = number
}

variable "backup_window" {
  description = "Maintenance window"
  default = "00:00-01:00"
  type = string
}

variable "delete_automated_backups" {
  description = "Delete automated backups"
  default = {{ delete_automated_backups }}
  type = bool
}

variable "skip_final_snapshot" {
  description = "Skip final snapshot"
  default = {{ skip_final_snapshot }}
  type = bool
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

# Clustering
variable "activate_high_availability" {
  description = "Define if DB should be in cluster mode"
  default = {{ activate_high_availability }}
  type = bool
}
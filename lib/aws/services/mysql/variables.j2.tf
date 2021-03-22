# Qovery

variable "cluster_name" {
  description = "Kubernetes cluster name"
  default     = "{{ cluster_name }}"
  type        = string
}

variable "region" {
  description = "AWS region to store terraform state and lock"
  default     = "{{ region }}"
  type        = string
}

variable "kubernetes_cluster_id" {
  description = "Kubernetes cluster name with region"
  default     = "{{ kubernetes_cluster_id }}"
  type        = string
}

variable "region_cluster_name" {
  description = "AWS region to store terraform state and lock"
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

# MySQL instance basics

variable "mysql_identifier" {
  description = "MySQL instance name (DB identifier)"
  default = "{{ fqdn_id }}"
  type = string
}

variable "port" {
  description = "MySQL instance port"
  default = {{ database_port }}
  type = number
}

variable "disk_size" {
  description = "disk instance size"
  default = {{ database_disk_size_in_gib }}
  type = number
}

variable "mysql_version" {
  description = "MySQL version"
  default = "{{ version }}"
  type = string
}

variable "storage_type" {
  description = "One of 'standard' (magnetic), 'gp2' (general purpose SSD), or 'io1' (provisioned IOPS SSD)."
  default = "{{ database_disk_type }}"
  type = string
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

variable "database_name" {
  description = "The name of the database to create when the DB instance is created. If this parameter is not specified, no database is created in the DB instance"
  default = "{{ database_name }}"
  type = string
}

# Network

variable "publicly_accessible" {
  description = "Instance publicly accessible"
  default = true
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
  default = true
  type = bool
}

variable "maintenance_window" {
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

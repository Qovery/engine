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

variable "eks_cluster_id" {
  description = "Kubernetes cluster name with region"
  default     = "{{ eks_cluster_id }}"
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

# PostgreSQL instance basics

variable "postgresql_identifier" {
  description = "PostgreSQL instance name (DB identifier)"
  default = "{{ fqdn_id }}"
  type = string
}

variable "port" {
  description = "PostgreSQL instance port"
  default = "{{ database_port }}"
  type = number
}

variable "disk_size" {
  description = "disk instance size"
  default = "{{ database_disk_size_in_gib }}"
  type = number
}

variable "postgresql_version" {
  description = "Postgresql version"
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
  default = "{{ database_instance_type }}"
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

# Monitoring

variable "performance_insights_enabled" {
  description = "Specifies whether Performance Insights are enabled"
  default = true
  type = bool
}

variable "performance_insights_enabled_retention" {
  description = "The amount of time in days to retain Performance Insights data"
  default = 7
  type = number
}

# Backups

variable "backup_retention_period" {
  description = "Backup retention period"
  default = 14
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

# Snapshots
# TODO later
#variable "snapshot_identifier" {
#  description = "Snapshot ID to restore"
#  default = "{ service_info['snapshot']['snapshot_id'] }"
#  type = string
#}

{%- if resource_expiration_in_seconds is defined %}
# Pleco ttl
variable "resource_expiration_in_seconds" {
  description = "Resource expiration in seconds"
  default = {{ resource_expiration_in_seconds }}
  type = number
}
{% endif %}
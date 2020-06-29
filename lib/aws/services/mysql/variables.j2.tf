# Qovery

variable "cluster_name" {
  description = "Kubernetes cluster name"
  default     = "{{ qovery_env.cluster_name }}"
  type        = string
}

variable "region" {
  description = "AWS region to store terraform state and lock"
  default     = "{{ qovery_env.region }}"
  type        = string
}

variable "region_cluster_name" {
  description = "AWS region to store terraform state and lock"
  default     = "{{ qovery_env.region }}-{{ qovery_env.cluster_name }}"
  type        = string
}

variable "q_project_id" {
  description = "Qovery project ID"
  default     = "{{ qovery_env.project_id }}"
  type        = string
}

variable "q_customer_id" {
  description = "Qovery customer ID"
  default     = "{{ qovery_env.owner_id }}"
  type        = string
}

variable "q_environment_id" {
  description = "Qovery client environment"
  default     = "{{ qovery_env.environment_id }}"
  type        = string
}

# MySQL instance basics

variable "mysql_identifier" {
  description = "MySQL instance name (DB identifier)"
  default = "{{ service_info['fqdn_id'] }}"
  type = string
}

variable "port" {
  description = "MySQL instance port"
  default = {{ service_info["port"] }}
  type = number
}

variable "disk_size" {
  description = "disk instance size"
  default = {{ service_info["disk_size_in_mb"] }}
  type = number
}

variable "mysql_version" {
  description = "MySQL version"
  default = "{{ service_info['version'] }}"
  type = string
}

variable "storage_type" {
  description = "One of 'standard' (magnetic), 'gp2' (general purpose SSD), or 'io1' (provisioned IOPS SSD)."
  default = "gp2"
  type = string
}

variable "instance_class" {
  description = "Type of instance: https://docs.aws.amazon.com/AmazonRDS/latest/UserGuide/Concepts.DBInstanceClass.html"
  default = "db.t2.micro"
  type = string
}

variable "username" {
  description = "Admin username for the master DB user"
  default = "{{ service_info['username'] }}"
  type = string
}

variable "password" {
  description = "Admin password for the master DB user"
  default = "{{ service_info['password'] }}"
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

# Snapshots

variable "snapshot_identifier" {
  description = "Snapshot ID to restore"
  default = "{{ service_info['snapshot']['snapshot_id'] }}"
  type = string
}

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

variable "database_tags" {
  description = "Qovery database tags"
  default     = {
    "cluster_name"                                                                   = "{{ cluster_name }}"
    "cluster_id"                                                                     = "{{ kubernetes_cluster_id }}"
    "region"                                                                         = "{{ region }}"
    "q_client_id"                                                                    = "{{ owner_id }}"
    "q_environment_id"                                                               = "{{ environment_id }}"
    "q_project_id"                                                                   = "{{ project_id }}"
    {% if resource_expiration_in_seconds > -1 %}
    "ttl"                                                                            = "{{ resource_expiration_in_seconds }}"
    {% endif %}
    {% if snapshot is defined and snapshot["snapshot_id"] %} meta_last_restored_from = { { snapshot['snapshot_id'] } }
    {% endif %}
  }
  type        = map
}

{%- if resource_expiration_in_seconds > -1 %}
# Pleco ttl
variable "resource_expiration_in_seconds" {
  description = "Resource expiration in seconds"
  default = {{resource_expiration_in_seconds}}
  type = number
}
{% endif %}

{%- if snapshot is defined %}
# Snapshots
variable "snapshot_identifier" {
  description = "Snapshot ID to restore"
  default = "{{ snapshot['snapshot_id']}}"
  type = string
}
{% endif %}

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

variable "kubernetes_cluster_az_list" {
  description = "Kubernetes availability zones"
  default = {{ kubernetes_cluster_az_list }}
  type = list(string)
}

# Upgrades

variable "auto_minor_version_upgrade" {
  description = "Indicates that minor engine upgrades will be applied automatically to the DB instance during the maintenance window"
  default = true
  type = bool
}

variable "apply_changes_now" {
  description = "Apply changes now or during the during the maintenance window"
  default = false
  type = bool
}

variable "preferred_maintenance_window" {
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

variable "preferred_backup_window" {
  description = "Maintenance window"
  default = "00:00-01:00"
  type = string
}

variable "delete_automated_backups" {
  description = "Delete automated backups"
  default = {{delete_automated_backups}}
  type = bool
}

variable "skip_final_snapshot" {
  description = "Skip final snapshot"
  default = {{ skip_final_snapshot }}
  type = bool
}

variable "final_snapshot_name" {
  description = "Name of the final snapshot before the database goes deleted"
  default = "{{ final_snapshot_name }}"
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
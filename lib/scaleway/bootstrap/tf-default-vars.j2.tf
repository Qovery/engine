# Qovery

variable "cloud_provider" {
  description = "Cloud provider name"
  default = "scw"
  type = string
}

variable "region" {
  description = "Scaleway region to store terraform state and lock"
  default     = "{{ scw_region }}"
  type        = string
}

variable "zone" {
  description = "Scaleway zone to store terraform state and lock"
  default     = "{{ scw_zone }}"
  type        = string
}

variable "qovery_nats_url" {
  description = "URL of qovery nats server"
  default = "{{ qovery_nats_url }}"
  type = string
}

variable "qovery_nats_user" {
  description = "user of qovery nats server"
  default = "{{ qovery_nats_user }}"
  type = string
}

variable "qovery_nats_password" {
  description = "password of qovery nats server"
  default = "{{ qovery_nats_password }}"
  type = string
}

variable "test_cluster" {
  description = "Is this a test cluster?"
  default = "{{ test_cluster }}"
  type = string
}

# Scaleway Specifics
variable "organization_id" {
  description = "Qovery Organization ID"
  default     = "{{ organization_id }}"
  type        = string
}

variable "scaleway_project_id" {
  description = "Scaleway project ID (namespace)"
  default     = "{{ scaleway_project_id }}"
  type        = string
}

variable "scaleway_access_key" {
  description = "Scaleway access key"
  default     = "{{ scaleway_access_key }}"
  type        = string
}

variable "scaleway_secret_key" {
  description = "Scaleway secret key"
  default     = "{{ scaleway_secret_key }}"
  type        = string
}

# Kubernetes

variable "kubernetes_cluster_id" {
  description = "Kubernetes cluster id"
  default     = "{{ kubernetes_cluster_id }}"
  type        = string
}

variable "kubernetes_cluster_name" {
  description = "Kubernetes cluster name"
  default     = "qovery-{{ kubernetes_cluster_id }}" # TODO(benjaminch): handle name creation in code
  type        = string
}

variable "scaleway_ks_version" {
  description = "Kubernetes cluster version"
  default = "{{ kubernetes_cluster_version }}"
  type    = string
}

variable "scaleway_ks_pool_autoscale" {
  description = "Enable built-in cluster autoscaler"
  default = true
  type    = bool
}

# Engine info

variable "qovery_engine_info" {
  description = "Qovery engine info"
  default = {
    "token" = "{{ engine_version_controller_token }}"
    "api_fqdn" = "{{ qovery_api_url }}"
  }
  type = map(string)
}

variable "qovery_engine_replicas" {
  description = "This variable is used to get random ID generated for the engine"
  default = "2"
  type = number
}

# Agent info

variable "qovery_agent_info" {
  description = "Qovery agent info"
  default = {
    "token" = "{{ agent_version_controller_token }}"
    "api_fqdn" = "{{ qovery_api_url }}"
  }
  type = map(string)
}

variable "qovery_agent_replicas" {
  description = "This variable is used to get random ID generated for the agent"
  default = "1"
  type = number
}

# Qovery features

variable "log_history_enabled" {
  description = "Enable log history"
  default = {{ log_history_enabled }}
  type = bool
}

variable "metrics_history_enabled" {
  description = "Enable metrics history"
  default = {{ metrics_history_enabled }}
  type = bool
}

{%- if resource_expiration_in_seconds is defined %}
# Pleco ttl
variable "resource_expiration_in_seconds" {
  description = "Resource expiration in seconds"
  default = {{ resource_expiration_in_seconds }}
  type = number
}
{% endif %}
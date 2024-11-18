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

variable "organization_long_id" {
  description = "Qovery Organization long ID"
  default     = "{{ organization_long_id }}"
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

variable "kubernetes_cluster_long_id" {
  description = "Kubernetes cluster long id"
  default     = "{{ kubernetes_cluster_long_id}}"
  type        = string
}

variable "kubernetes_cluster_id" {
  description = "Kubernetes cluster id"
  default     = "{{ kubernetes_cluster_id }}"
  type        = string
}

variable "kubernetes_cluster_name" {
  description = "Kubernetes cluster name"
  default     = "{{ kubernetes_cluster_name }}"
  type        = string
}

variable "scaleway_ks_version" {
  description = "Kubernetes cluster version"
  default = "{{ kubernetes_cluster_version }}"
  type    = string
}

variable "scaleway_ks_type" {
  description = "Kubernetes cluster version"
  default = "{{ kubernetes_cluster_type }}"
  type    = string
}

# Qovery features

{%- if resource_expiration_in_seconds > -1 %}
# Pleco ttl
variable "resource_expiration_in_seconds" {
  description = "Resource expiration in seconds"
  default = {{ resource_expiration_in_seconds }}
  type = number
}
{% endif %}
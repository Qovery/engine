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
  default = {
    "cluster_name" = "{{ cluster_name }}"
    "cluster_id" = "{{ kubernetes_cluster_id }}"
    "region" = "{{ region }}"
    "q_client_id" = "{{ owner_id }}"
    "q_environment_id" = "{{ environment_id }}"
    "q_project_id" = "{{ project_id }}"
    {% if resource_expiration_in_seconds is defined %}"ttl" = "{{ resource_expiration_in_seconds }}" {% endif %}
  }
  type = map
}

{%- if resource_expiration_in_seconds is defined %}
# Pleco ttl
variable "resource_expiration_in_seconds" {
description = "Resource expiration in seconds"
default = {{ resource_expiration_in_seconds }}
type = number
}
{% endif %}

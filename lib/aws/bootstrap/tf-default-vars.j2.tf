# Qovery

variable "cloud_provider" {
  description = "Cloud provider name"
  default = "aws"
  type = string
}

variable "region" {
  description = "AWS region to store terraform state and lock"
  default     = "{{ aws_region }}"
  type        = string
}

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

# AWS specific
variable "aws_availability_zones" {
  description = "AWS availability zones"
  default = {{ aws_availability_zones }}
  type = list(string)
}

variable "vpc_cidr_block" {
  description = "VPC CIDR block"
  default = "{{ vpc_cidr_block }}"
  type = string
}

# Kubernetes
variable "eks_k8s_versions" {
  description = "Kubernetes version"
  default = {
    "masters": "{{ eks_masters_version }}",
    "workers": "{{ eks_workers_version }}",
  }
  type = map(string)
}

variable "kubernetes_cluster_long_id" {
  description = "Kubernetes cluster long id"
  default     = "{{ kubernetes_cluster_long_id }}"
  type        = string
}

variable "kubernetes_cluster_id" {
  description = "Kubernetes cluster id"
  default     = "{{ kubernetes_cluster_id }}"
  type        = string
}

variable "kubernetes_cluster_name" {
  description = "Kubernetes cluster name"
  default     = "qovery-{{ kubernetes_cluster_id }}"
  type        = string
}

variable "eks_access_cidr_blocks" {
  description = "Kubernetes CIDR Block"
  default     = {{ eks_access_cidr_blocks }}
  type        = list(string)
}

variable "cloudwatch_eks_log_groups" {
  description = "AWS cloudwatch log group for EKS"
  default = "{{ cloudwatch_eks_log_group }}"
  type = string
}

variable "aws_cloudwatch_eks_logs_retention_days" {
  description = "AWS cloudwatch log group retention in days"
  default = {{ aws_cloudwatch_eks_logs_retention_days }}
  type = number
}

variable "ec2_metadata_imds_version" {
  description = "Set the imds version"
  default = "{{ ec2_metadata_imds_version }}"
  type = string
}

# Databases

variable "database_postgresql_allowed_cidrs" {
  description = "PostgreSQL allowed CIDR Block"
  default = {{ database_postgresql_allowed_cidrs }}
  type = list(string)
}

variable "database_mysql_allowed_cidrs" {
  description = "MySQL allowed CIDR Block"
  default = {{ database_mysql_allowed_cidrs }}
  type = list(string)
}

variable "database_redis_allowed_cidrs" {
  description = "Redis allowed CIDR Block"
  default = {{ database_redis_allowed_cidrs }}
  type = list(string)
}

variable "database_mongodb_allowed_cidrs" {
  description = "MongoDB allowed CIDR Block"
  default = {{ database_mongodb_allowed_cidrs }}
  type = list(string)
}

# S3 bucket name

variable "s3_bucket_kubeconfig" {
  description = "S3 bucket containing kubeconfigs"
  default = "{{ s3_kubeconfig_bucket }}"
  type = string
}

variable "enable_vpc_flow_logs" {
  description = "Enable VPC flow logs"
  default = {{ aws_enable_vpc_flow_logs }}
  type = bool
}

variable "vpc_flow_logs_retention_days" {
  description = "Set VPC flow logs retention in days"
  default = {{ vpc_flow_logs_retention_days }}
  type = number
}

variable "aws_iam_user_mapper_sso_enabled" {
  description = "Enable SSO"
  default = "{{ aws_iam_user_mapper_sso_enabled }}"
  type = string
}

variable "aws_iam_user_mapper_sso_role_arn" {
  description = "Set cluster SSO role ARN"
  default = "{{ aws_iam_user_mapper_sso_role_arn }}"
  type = string
}

variable "s3_flow_logs_bucket_name" {
  description = "S3 bucket containing flow logs"
  default = "{{ s3_flow_logs_bucket_name }}"
  type = string
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

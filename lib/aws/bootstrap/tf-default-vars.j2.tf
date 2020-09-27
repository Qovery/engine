# AWS specific
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

variable "vpc_cidr_block" {
  description = "VPC CIDR block"
  default = "{{ vpc_cidr_block }}"
  type = string
}

variable "test_cluster" {
  description = "Is this a test cluster?"
  default = "false"
  type = string
}

# Kubernetes

variable "eks_subnets_zone_a" {
  description = "EKS subnets Zone A"
  default = {{ eks_zone_a_subnet_blocks }}
  type = list(string)
}

variable "eks_subnets_zone_b" {
  description = "EKS subnets Zone B"
  default = {{ eks_zone_b_subnet_blocks }}
  type = list(string)
}

variable "eks_subnets_zone_c" {
  description = "EKS subnets Zone C"
  default = {{ eks_zone_c_subnet_blocks }}
  type = list(string)
}

variable "eks_cidr_subnet" {
  description = "EKS CIDR (x.x.x.x/CIDR)"
  default     = {{ eks_cidr_subnet }}
  type        = number
}

variable "eks_k8s_versions" {
  description = "Kubernetes version"
  default = {
    "masters": "{{ eks_masters_version }}",
    "workers": "{{ eks_workers_version }}",
  }
  type = map(string)
}

variable "eks_cluster_id" {
  description = "Kubernetes cluster name with region"
  default     = "{{ eks_cluster_id }}"
  type        = string
}

variable "eks_cluster_name" {
  description = "Kubernetes cluster name"
  default     = "{{ eks_cluster_name }}"
  type        = string
}

variable "eks_access_cidr_blocks" {
  description = "Kubernetes cluster name"
  default     = ["185.162.179.5/32", "78.192.247.93/32"]
  type        = list(string)
}

variable "eks_cloudwatch_log_group" {
  description = "AWS cloudwatch log group for EKS"
  default = "{{ eks_cloudwatch_log_group }}"
  type = string
}

# S3 bucket name

variable "s3_bucket_kubeconfig" {
  description = "S3 bucket with kubeconfigs"
  default = "{{ s3_kubeconfig_bucket }}"
  type = string
}

variable "s3_bucket_qengine_resources" {
  description = "S3 bucket containing qengine resources (libs)"
  default = "prod-qengine-resources"
  type = string
}

# EC2 SSH default SSH key

variable "ec2_ssh_default_key" {
  description = "Default SSH key"
  default = {
    "key_name" = "{{ eks_cluster_id }}-qovery"
    "public_key" = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQDN5AylaONAOt684AVqNL+jtOspRnwAXF3VmHYl02bmsFcxaAcbVal89o/lXrfg787J1D+5wR9thYDCctlvccrHgiTotBFA8HPafDeIvzGLXsCCGCmr9ctN1qtO7BhfwpyGrGJD6I5XmW67R7yhNlawsF2RtwJbQA+Jz/FdvBu/JZHuOaG3dh556Am89wfGyp/Lep5/Hph6iiDP2yujz206zoiRVNyaIYbzaQISU8Jwg39EKJ2YaDTG2Vb4EJ6hjRGguZPZSW0o77CV5CPICFCtMWMb8DGAxd4BTGP1tZmTnDP9mWSFD/5WPkudiwFKSsN2ZKAomOfPB8bZXi7mgmQKTrzsDEkWdz8CUC7TUW1mmIbXDfoFdaTMuZpsox2v9970554PyiSyez3SZ6GiJPy1VQifWeQlLraAgFoXICRiQwYUhszzM9dfuQE9RDM3r5K/mXRfiuzkEK/TH6I+gi08ZZzh6TsyGQhaX2bgYID6TFBFcnLpL/PGckrt2Ub50TU= qovery"
  }
  type = map(string)
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

# Agent info

variable "qovery_agent_replicas" {
  description = "This variable is used to get random ID generated for the agent"
  default = "1"
  type = number
}

# RDS

variable "rds_subnets_zone_a" {
  description = "RDS subnets Zone A"
  default = {{ rds_zone_a_subnet_blocks }}
  type = list(string)
}

variable "rds_subnets_zone_b" {
  description = "RDS subnets Zone B"
  default = {{ rds_zone_b_subnet_blocks }}
  type = list(string)
}

variable "rds_subnets_zone_c" {
  description = "RDS subnets Zone C"
  default = {{ rds_zone_c_subnet_blocks }}
  type = list(string)
}

variable "rds_cidr_subnet" {
  description = "RDS CIDR (x.x.x.x/CIDR)"
  default     = {{ rds_cidr_subnet }}
  type        = number
}

# DocumentDB

variable "documentdb_subnets_zone_a" {
  description = "DocumentDB subnets Zone A"
  default = {{ documentdb_zone_a_subnet_blocks }}
  type = list(string)
}

variable "documentdb_subnets_zone_b" {
  description = "DocumentDB subnets Zone B"
  default = {{ documentdb_zone_b_subnet_blocks }}
  type = list(string)
}

variable "documentdb_subnets_zone_c" {
  description = "DocumentDB subnets Zone C"
  default = {{ documentdb_zone_c_subnet_blocks }}
  type = list(string)
}

variable "documentdb_cidr_subnet" {
  description = "DocumentDB CIDR (x.x.x.x/CIDR)"
  default     = {{ documentdb_cidr_subnet }}
  type        = number
}

# Elasticsearch

# Must start with a lowercase alphabet and be at least 3 and no more than 28 characters long.
# Valid characters are a-z (lowercase letters), 0-9, and - (hyphen).
variable "elasticsearch_q_logs_domain_name" {
  description = "ES domain name"
  default = "{{ eks_cluster_id }}-q-logs"
  type = string
}

variable "elasticsearch_node_number" {
  description = "Number of Elasticsearch nodes"
  default = 3
  type = number
}

variable "elasticsearch_volume_size" {
  description = "Disk size per node"
  default = 50
  type = number
}

variable "elasticsearch_subnets_zone_a" {
  description = "Elasticsearch subnets Zone A"
  default = {{ elasticsearch_zone_a_subnet_blocks }}
  type = list(string)
}

variable "elasticsearch_subnets_zone_b" {
  description = "Elasticsearch subnets Zone B"
  default = {{ elasticsearch_zone_b_subnet_blocks }}
  type = list(string)
}

variable "elasticsearch_subnets_zone_c" {
  description = "Elasticsearch subnets Zone C"
  default = {{ elasticsearch_zone_c_subnet_blocks }}
  type = list(string)
}

variable "elasticsearch_cidr_subnet" {
  description = "Elasticsearch CIDR (x.x.x.x/CIDR)"
  default     = {{ elasticsearch_cidr_subnet }}
  type        = number
}

variable "elasticsearch_logs_curator" {
  description = "Curator config"
  default = {
    "cron": "0 0 * * *"
    "days_to_keep": "7"
  }
  type = map(string)
}
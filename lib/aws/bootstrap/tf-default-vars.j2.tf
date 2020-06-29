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

variable "vpc_cidr_block" {
  description = "VPC CIDR block"
  default = "10.0.0.0/16"
  type = string
}

variable "test_cluster" {
  description = "Is this a test cluster?"
  default = "false"
  type = string
}

# Kubernetes

variable "k8s_versions" {
  description = "Kubernetes version"
  default = {
    "masters": "{{ eks_masters_version }}",
    "workers": "{{ eks_workers_version }}",
  }
  type = map(string)
}

variable "region_cluster_name" {
  description = "Kubernetes cluster name with region"
  default     = "{{ eks_region_cluster_name }}"
  type        = string
}

variable "cluster_name" {
  description = "Kubernetes cluster name"
  default     = "{{ eks_cluster_name }}"
  type        = string
}

variable "k8s_access_cidr_blocks" {
  description = "Kubernetes cluster name"
  default     = ["185.162.179.5/32", "78.192.247.93/32"]
  type        = list(string)
}

variable "k8s_nb_subnets_per_zone" {
  description = "Kubernetes workers, number of desired subnets (3 zones used)"
  default     = 21
  type        = number
}

variable "k8s_cidr_subnet" {
  description = "Kubernetes workers CIDR (x.x.x.x/CIDR)"
  default     = 23
  type        = number
}

variable "k8s-workers" {
  description = "Kubernetes workers type"
  default = {
    "instance-type": "{{ eks_workers_instance_type }}",
    "min-size": "{{ eks_workers_min_size }}",
    "max-size": "{{ eks_workers_max_size }}",
    "desired-capacity": "{{ eks_workers_desired_capacity }}"
  }
  type = map(string)
}

# ECR

variable "ecr_name" {
  description = "ECR name"
  default = ""
  type = string
}

# RDS

variable "rds_nb_subnets_per_zone" {
  description = "RDS number of desired subnets (3 zones used)"
  default     = 7
  type        = number
}

variable "rds_cidr_subnet" {
  description = "RDS CIDR (x.x.x.x/CIDR)"
  default     = 23
  type        = number
}

# Redis
variable "redis_version" {
  description = "Redis for Qovery"
  default = "5.0.6"
  type = string
}

variable "redis_parameters" {
  description = "Redis additional parameters"
  default = []
  type = list(map(any))
}

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

variable "parameter_group_family" {
  description = "RDS parameter group family"
  default = "{{ parameter_group_family }}"
  type = string
}

variable "storage_type" {
  description = "One of 'standard' (magnetic), 'gp2' (general purpose SSD), or 'io1' (provisioned IOPS SSD)."
  default = "{{ database_disk_type }}"
  type = string
}

variable "disk_iops" {
  description = "The amount of provisioned IOPS. Setting this implies a storage_type of 'io1' or 'io2'. Can only be set when storage_type is 'io1', 'io2' or 'gp3'. Cannot be specified for gp3 storage if the allocated_storage value is below a per-engine threshold"
  default = {{ database_disk_iops }}
  type = number
}

variable "encrypt_disk" {
  description = "Enable disk encryption"
  default = "{{ encrypt_disk }}"
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

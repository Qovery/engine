# documentdb instance basics

variable "documentdb_identifier" {
  description = "Documentdb cluster name (Cluster identifier)"
  default = "{{ fqdn_id }}"
  type = string
}

variable "documentdb_instances_number" {
  description = "DocumentDB instance numbers"
  default = 1
  type = number
}

variable "port" {
  description = "Documentdb instance port"
  default = {{ database_port }}
  type = number
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
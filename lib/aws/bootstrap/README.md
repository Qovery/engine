# Naming convention

* providers: providers files should be named tf-providers-<cloud-provider-name>
* variables: should be named with _, never with - (in the tf-default-vars.j2.tf)
* variables should start with the name of the service associated (ex: eks_, rds_...)
* tf filenames: filenames should never contain _ or they may not be interpreted
* resources names should not contain - but _ instead (resource "type" "name" {})
* name field in resource should not contain _ and - instead to avoid RFC 1123 limitation (resource "type" "name" {name = "this name"})
* the length of fields names should never exceed 64 chars and prefer less than 32 chars, still for RFC 1123
* any and only *.j2.tf files will be rendered by jinja 
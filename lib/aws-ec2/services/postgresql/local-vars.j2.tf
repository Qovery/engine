locals {
  postgres_database_tags = merge (var.database_tags, {
    database_identifier = var.postgresql_identifier
    creationDate = time_static.on_db_create.rfc3339
  })
}
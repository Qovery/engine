# Container registry naming strategy

## New naming strategy
### AWS ECR
base_url/<cluster_short_id>-<sanitized_git_url>:<tag>

### GCP Artifact Registry
base_url/<gcp_project_id>/<cluster_short_id>-<sanitized_git_url>/built-by-qovery:<tag>

### Scaleway CR
base_url/<cluster_short_id>-<sanitized_git_url>/built-by-qovery:<tag>

### Github CR
base_url/<client_(orga/username)>/<cluster_short_id>-<sanitized_git_url>:<tag>


## Deprecated
### AWS ECR
base_url/<service_short_id>

### GCP Artifact Registry
base_url/<gcp_project_id>/qovery-<service_short_id>/<service_short_id>:<tag>

### Scaleway CR
base_url/<service_short_id>/<service_short_id>:<tag>

### Github CR
base_url/<client_(orga/username)>/<service_short_id>:<tag>


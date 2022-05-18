# Tests

Several kind of tests exists in the Engine:
* Unit tests: offer simple tests on some parts of the system
* Functional tests: they are used to ensure the finality of a deployment/update/delete

Of course functional tests takes a longer time to deploy and they requests several specific access to be used (ex: AWS credentials for AWS cloud provider testing)

# Usage

In order to perform functional tests, you can use environment variables or Vault. Environment variables always overrides vault values.

In order to run functional tests, here are the minimum environment variables:
* LIB_ROOT_DIR=<projects_dir>/engine/lib-engine/lib
* RUST_LOG=info
* WORKSPACE_ROOT_DIR=<projects_dir>/engine

## Other options

### Vault
Others option will also be necessary and can be found in the `FuncTestsSecrets` struct in `test_utilities` folder.

(Qovery internal) As a lot of them are requested, the simplest way to use them is to use Vault:
* VAULT_ADDR=https://<vault_address>
* VAULT_TOKEN=<vault_token>

### TTL
By default, all deployed tests resources are going to be tagged with a TTL, to be automatically cleaned with [Pleco](https://github.com/Qovery/pleco) if a test fail for some reasons.

This ttl is set by default to 1h, but you can override it with a `ttl` environment variable in seconds like: `ttl=7200`.

### Terraform dry run
If you just want to render Terraform without applying changes, you can set `dry_run_deploy` environment variable to anything to enable it like `dry_run_deploy=true`.

### Custom cluster id
It can be useful sometimes to be able to add a custom cluster id during tests. In order to do that, simply use `custom_cluster_id` environment variable with the desired name.

Note: remind that you can't need to use valid chars https://datatracker.ietf.org/doc/html/rfc8117

### Forced upgrade
By default, helm charts are applied only when they do not exist or when they receive an update.

During chart upgrade or atomic rollback, Terraform is not able to catch those changes and requires an upgrade.
In order to perform it, you need the variable `forced_upgrade` to `true` to ensure everything is up to date.

The advantage of having it set to `false` by default, is the deployment speed. Only helm changes are going to be applied. The drawback is you can't
be 100% sure of what you've deployed is what you asked for on your infra.
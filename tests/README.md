# Tests

Several kind of tests exists in the Engine:
* Unit tests: offer simple tests on some parts of the system
* Functional tests: they are used to ensure the finality of a deployment/update/delete

Of course functional tests takes a longer time to deploy and they requests several specific access to be used (ex: AWS credentials for AWS cloud provider testing)

# Usage

In order to perform functional tests, you can use environment variables or Vault. Environment variables always overrides vault values.

In order to run functional tests, here are the minimum environment variables:
* LIB_ROOT_DIR=$HOME/qovery/engine/lib
* RUST_LOG=info
* WORKSPACE_ROOT_DIR=$HOME/qovery/engine

## Other options

### Vault
Others option will also be necessary and can be found in the `FuncTestsSecrets` struct in `test_utilities` folder.

(Qovery internal) As a lot of them are requested, the simplest way to use them is to use Vault:
* VAULT_ADDR=https://<vault_address>
* VAULT_TOKEN=<vault_token>

### TTL
By default all deployed tests resources are tagged with a TTL, to be automatically cleaned with [Pleco](https://github.com/Qovery/pleco) if a test fail for some reasons.

This ttl is set by default to 1h, but you can override it with a `ttl` environment variable in seconds like: `ttl=7200`.

### Terraform dry run
If you just want to render Terraform without applying changes, you can set `dry_run_deploy` environment variable to anything to enable it like `dry_run_deploy=true`.
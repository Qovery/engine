<p align="center">
  <a href="https://www.qovery.com">
    <img src="https://raw.githubusercontent.com/Qovery/public-resources/master/qovery_logo.svg" width="318px" alt="Qovery logo" />
  </a>
</p>
<h3 align="center">Deploy complex application, seamlessly</h3>
<p align="center">Deploy your apps on any Cloud providers in just a few seconds ⚡</p>

<p align="center">
<a href="https://discord.qovery.com"> <img alt="Discord" src="https://img.shields.io/discord/688766934917185556?label=discord&style=flat-square"> </a>
</p>

<br />

<p align="center">
    <img src="https://raw.githubusercontent.com/Qovery/public-resources/master/qovery_kubernetes_cloudproviders.svg" height="450px" alt="Qovery stack on top of Kubernetes and Cloud providers" />
</p>

**Qovery Engine** is an open-source abstraction layer product that makes apps deployment on **AWS**, **GCP**, **Azure** and others Cloud providers easy to do. The engine is coded in [Rust](https://www.rust-lang.org) and take advantage of [Terraform](https://www.terraform.io), [Helm](https://helm.sh), [Kubectl](https://kubernetes.io/docs/reference/kubectl/overview), [Docker](https://www.docker.com) to manage resources.

- Website: https://www.qovery.com
- Full doc: https://docs.qovery.com
- Qovery Engine doc: *coming soon*
- Community: [Join us](https://discord.qovery.com) on Discord

**Please note**: We take Qovery's security and our users' trust very seriously. If you believe you have found a security issue in Qovery, please responsibly disclose by contacting us at security@qovery.com.

## ✨ Features

- **Zero infrastructure management:** Qovery Engine initialized, configure and manage your Cloud account for you.
- **Multi Cloud:** Qovery Engine is built to work on AWS, GCP, Azure and any kind of Cloud provider.
- **On top of Kubernetes:** Qovery Engine takes advantage of the power of Kubernetes at a higher level of abstraction.
- **Terraform and Helm:** Qovery Engine uses Terraform and Helm files to manage the infrastructure and app deployment.
- **Powerful CLI:** Use the provided Qovery Engine CLI to seamlessly deploy your app on your Cloud account.  
- **Web Interface:** Qovery provides a web interface through [qovery.com](https://www.qovery.com)

### 🔌 Plugins
Qovery engine supports a number of build methods and target Cloud providers out of the box and more can be easily added:
- **Cloud providers:** [AWS](https://docs.qovery.com/docs/using-qovery/configuration/business/cloud-account/amazon-web-services/), Digital Ocean ([in progress](https://docs.qovery.com/docs/using-qovery/configuration/business/cloud-account/digital-ocean/)), Azure ([vote](https://docs.qovery.com/docs/using-qovery/configuration/business/cloud-account/azure/)), GCP ([vote](https://docs.qovery.com/docs/using-qovery/configuration/business/cloud-account/google-cloud-platform/)), Scaleway ([vote](https://docs.qovery.com/docs/using-qovery/configuration/business/cloud-account/scaleway/))
- **Build platforms:** [Qovery CI](https://docs.qovery.com/docs/using-qovery/configuration/business/build-platform/qovery-ci/), Circle CI ([vote](https://docs.qovery.com/docs/using-qovery/configuration/business/build-platform/circle-ci/)), Gitlab CI ([vote](https://docs.qovery.com/docs/using-qovery/configuration/business/build-platform/gitlab-ci/)), Github Actions ([vote](https://docs.qovery.com/docs/using-qovery/configuration/business/build-platform/github-actions/))
- **Container registries:** [ECR](https://docs.qovery.com/docs/using-qovery/configuration/business/container-registry/elastic-container-registry/), [DockerHub](https://docs.qovery.com/docs/using-qovery/configuration/business/container-registry/docker-hub/), DOCR ([vote](https://docs.qovery.com/docs/using-qovery/configuration/business/container-registry/digital-ocean-container-registry/)), ACR ([vote](https://docs.qovery.com/docs/using-qovery/configuration/business/container-registry/azure-container-registry/)), SCR ([vote](https://docs.qovery.com/docs/using-qovery/configuration/business/container-registry/scaleway-container-registry/))
- **DNS providers:** Cloudflare
- **Monitoring services:** Datadog ([vote](https://docs.qovery.com/docs/using-qovery/configuration/business/monitoring/datadog/)), Newrelic ([vote](https://docs.qovery.com/docs/using-qovery/configuration/business/monitoring/new-relic/))

**[See more on our website](https://www.qovery.com)**.

## Getting Started
TODO

### Installation
TODO

### Usage

#### CLI
TODO

#### Rust lib
Initialize Kubernetes on AWS 
```rust
let x = "TODO";
```

Deploy an app on AWS
```rust
let y = "TODO";
```

## Documentation
Full, comprehensive documentation is available on the Qovery website: https://docs.qovery.com

## Contributing
Please read our [Contributing Guide](./CONTRIBUTING.md) before submitting a Pull Request to the project.

## Community support
For general help using Qovery Engine, please refer to [the official Qovery Engine documentation](https://docs.qovery.com). For additional help, you can use one of these channels to ask a question:

- [Discord](https://discord.qovery.com) (For live discussion with the Community and Qovery team)
- [GitHub](https://github.com/qovery/engine) (Bug reports, Contributions)
- [Roadmap](https://roadmap.qovery.com) (Roadmap, Feature requests)
- [Twitter](https://twitter.com/qovery_) (Get the news fast)

## Roadmap
Check out our [roadmap](https://roadmap.qovery.com) to get informed of the latest features released and the upcoming ones. You may also give us insights and vote for a specific feature.

## FAQ
### Why Qovery exists?
TODO

### What is the difference between `Qovery` and `Qovery Engine`?
TODO

### Why the Qovery Engine is made in Rust?
TODO

### Why do you use Terraform, Helm and Kubectl binaries?
TODO

## License

See the [LICENSE](./LICENSE) file for licensing information.

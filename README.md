<p align="center">
  <a href="https://www.qovery.com">
    <img src="https://raw.githubusercontent.com/Qovery/public-resources/master/qovery-engine-logo.svg" width="318px" alt="Qovery logo" />
  </a>
</p>
<h3 align="center">The simplest way to deploy your apps in the Cloud</h3>
<p align="center">Deploy your apps on any Cloud providers in just a few seconds ⚡</p>

<p align="center">
<img src="https://img.shields.io/badge/stability-work_in_progress-lightgrey.svg?style=flat-square" alt="work in progress badge">
<img src="https://github.com/Qovery/engine/workflows/functionnal-tests/badge.svg?style=flat-square" alt="Func tests">
<a href="https://discord.qovery.com"> <img alt="Discord" src="https://img.shields.io/discord/688766934917185556?label=discord&style=flat-square"> </a>
</p>

<br />

<p align="center">
    <img src="https://raw.githubusercontent.com/Qovery/public-resources/master/qovery_kubernetes_cloudproviders.svg" height="450px" alt="Qovery stack on top of Kubernetes and Cloud providers" />
</p>

**Qovery Engine** is an open-source abstraction layer library that turns easy application deployment on **AWS**, **GCP**, **Azure**, and other Cloud providers in just a few minutes. The Qovery Engine is written in [Rust](https://www.rust-lang.org) and takes advantage of [Terraform](https://www.terraform.io), [Helm](https://helm.sh), [Kubectl](https://kubernetes.io/docs/reference/kubectl/overview), and [Docker](https://www.docker.com) to manage resources.

- Website: https://www.qovery.com
- Qovery documentation: https://hub.qovery.com/docs
- Community: [Join us](https://discord.qovery.com) on Discord and on our [Q&A forum](https://discuss.qovery.com)

**Please note**: We take Qovery's security and our users' trust very seriously. If you believe you have found a security issue in Qovery, please responsibly disclose by contacting us at security@qovery.com.

## ✨ Features

- **Zero infrastructure management:** Qovery Engine initializes, configures, and manages your Cloud account for you.
- **Multi Cloud:** Qovery Engine is built to work on AWS, GCP, Azure and any Cloud provider.
- **On top of Kubernetes:** Qovery Engine takes advantage of the power of Kubernetes at a higher level of abstraction.
- **Terraform and Helm:** Qovery Engine uses Terraform and Helm files to manage the infrastructure and app deployment.
- **Powerful CLI:** Use the provided Qovery Engine CLI to deploy your app on your Cloud account seamlessly.
- **Web Interface:** Qovery provides a web interface through [qovery.com](https://www.qovery.com)

### 🔌 Plugins
<p align="center">
    <img src="https://docs.qovery.com/img/policy-complete-flow.png" width="800px" alt="Qovery engine workflow" />
</p>

Qovery engine supports a number of different plugins to compose your own deployment flow:
- **Cloud providers:** [AWS](https://hub.qovery.com/docs/using-qovery/configuration/cloud-service-provider/amazon-web-services/), Scaleway ([in beta](https://hub.qovery.com/docs/using-qovery/configuration/cloud-service-provider/scaleway/)), Azure ([vote](https://hub.qovery.com/docs/using-qovery/configuration/cloud-service-provider/microsoft-azure/)), GCP ([vote](https://hub.qovery.com/docs/using-qovery/configuration/cloud-service-provider/google-cloud-platform/))
- **Build platforms:** [Qovery CI](https://hub.qovery.com/docs/using-qovery/addon/continuous-integration/qovery-ci/), Circle CI ([vote](https://hub.qovery.com/docs/using-qovery/addon/continuous-integration/circle-ci/)), Gitlab CI ([vote](https://hub.qovery.com/docs/using-qovery/addon/continuous-integration/gitlab-ci/)), GitHub Actions ([vote](https://hub.qovery.com/docs/using-qovery/addon/continuous-integration/github-actions/))
- **Container registries:** AWS ECR, DockerHub, ACR, Scaleway Container Registry
- **DNS providers:** Cloudflare
- **Monitoring services:** Datadog ([vote](https://hub.qovery.com/docs/using-qovery/addon/monitoring/datadog/)), Newrelic ([vote](https://hub.qovery.com/docs/using-qovery/addon/monitoring/new-relic/))

**[See more on our website](https://www.qovery.com)**.

## Demo

Here is a demo from [Qovery CLI](https://docs.qovery.com/docs/using-qovery/interface/cli/) from where we use the Qovery Engine.

[![Qovery CLI](https://asciinema.org/a/370072.svg)](https://asciinema.org/a/370072)

## Getting Started
### Installation
Use the Qovery Engine as a Cargo dependency.
```toml
qovery-engine = { git = "https://github.com/Qovery/engine", branch="main" }
```

### Usage

#### Rust lib
Initialize EKS (AWS Kubernetes) and ECR (AWS container registry) on AWS
```rust
let engine = Engine::new(
    context, // parameters
    local_docker, // initialize Docker as a Build Platform
    ecr, // initialize Elastic Container Registry
    aws, // initialize AWS account
    cloudflare, // initialize Cloudflare as DNS Nameservers
);

let session = match engine.session() {
    Ok(session) => session, // get the session
    Err(config_error) => panic!("configuration error {:?}", config_error),
};

let mut tx = session.transaction();

// create EKS (AWS managed Kubernetes cluster)
tx.create_kubernetes(&eks);

// create the infrastructure and wait for the result
match tx.commit() {
    TransactionResult::Ok => println!("OK"),
    TransactionResult::Rollback(commit_err) => println!("ERROR but rollback OK"),
    TransactionResult::UnrecoverableError(commit_err, rollback_err) => println!("FATAL ERROR")
};
```

Deploy an app from a Github repository on AWS
```rust
// create a session before
//------------------------

let mut environment = Environment {...};

let app = Application {
    id: "app-id-1".to_string(),
    name: "app-name-1".to_string(),
    action: Action::Create, // create the application, you can also do other actions
    git_url: "https://github.com/Qovery/node-simple-example.git".to_string(),
    git_credentials: GitCredentials {
        login: "github-login".to_string(), // if the repository is a private one, then use credentials
        access_token: "github-access-token".to_string(),
        expired_at: Utc::now(), // it's provided by the Github API
    },
    branch: "main".to_string(),
    commit_id: "238f7f0454783defa4946613bc17ebbf4ccc514a".to_string(),
    dockerfile_path: "Dockerfile".to_string(),
    private_port: Some(3000),
    total_cpus: "1".to_string(),
    cpu_burst: "1.5".to_string(),
    total_ram_in_mib: 256,
    min_instances: 1,
    max_instances: 4,
    storage: vec![], // you can add persistent storage here
    environment_variables: vec![], // you can include env var here
};

// add the app to the environment that we want to deploy
environment.applications.push(app);

// open a transaction
let mut tx = session.transaction();

// request to deploy the environment
tx.deploy_environment(&EnvironmentAction::Environment(environment));

// commit and deploy the environment
tx.commit();
```
*Note: the repository needs to have a Dockerfile at the root.*

## Documentation
Full, comprehensive documentation is available on the Qovery website: https://docs.qovery.com

## Contributing
Please read our [Contributing Guide](./CONTRIBUTING.md) before submitting a Pull Request to the project.

## Community support
For general help to use Qovery Engine, please refer to [the official Qovery Engine documentation](https://hub.qovery.com/docs). For additional help, you can use one of these channels to ask a question:

- [Discord](https://discord.qovery.com) (For live discussion with the Community and Qovery team)
- [GitHub](https://github.com/qovery/engine) (Bug reports, Contributions)
- [Roadmap](https://roadmap.qovery.com) (Roadmap, Feature requests)
- [Twitter](https://twitter.com/qovery_) (Get the news fast)

## Roadmap
Check out our [roadmap](https://roadmap.qovery.com) to get informed of the latest features released and the upcoming ones. You may also give us insights and vote for a specific feature.

## FAQ
### Why does Qovery exist?
At Qovery, we believe that the Cloud must be simpler than what it is today. Our goal is to consolidate the Cloud ecosystem and makes it accessible to any developer, DevOps, and company. Qovery helps people to focus on what they build instead of wasting time doing plumbing stuff.

### What is the difference between `Qovery` and `Qovery Engine`?
[Qovery](https://www.qovery.com) is a Container as a Service platform for developers. It combines the simplicity of Heroku, the reliability of AWS, and the power of Kubernetes. It makes the developer and DevOps life easier to deploy complex applications.

**Qovery Engine** is the Open Source abstraction layer used by Qovery to abstract the deployment of containers and databases on any Cloud provider.

### Why is the Qovery Engine written in Rust?
Rust is underrated in the Cloud industry. At Qovery, we believe that Rust can help in building resilient, efficient, and performant products. Qovery wants to contribute to make Rust being a significant player in the Cloud industry for the next 10 years.

### Why do you use Terraform, Helm and Kubectl binaries?
The Qovery Engine is designed to operate as an administrator and takes decisions on the output of binaries, service, API, etc. Qovery uses the most efficient tools available in the market to manage resources.

## License

See the [LICENSE](./LICENSE) file for licensing information.

## Qovery

Qovery is a [CNCF](https://landscape.cncf.io/format=members&selected=qovery-member) and [Linux Foundation](https://www.linuxfoundation.org/membership/members/) silver member.

<img src="https://raw.githubusercontent.com/cncf/artwork/master/other/cncf-member/silver/color/cncf-member-silver-color.svg" width="300px" alt="CNCF Silver Member logo" />

## Release notes engine v1.360.1   
### Bug fixes   
* [90eb3660449f7c61a53a7c2e95a849da20d35dbd](https://github.com/Qovery/engine/commit/90eb3660449f7c61a53a7c2e95a849da20d35dbd): fix(operator): grant per-worker PDB permissions  (Pierre Gerbelot)
   
### Others   
* [ce5b4f4775607311f233a5417cb245280edcaecb](https://github.com/Qovery/engine/commit/ce5b4f4775607311f233a5417cb245280edcaecb): feat(platform-catalog): publish Karpenter and add optional BYOK layer  (Pierre Gerbelot)
   
* [23c6e6a3e7406d83df420631ea8d3df5a1eecf07](https://github.com/Qovery/engine/commit/23c6e6a3e7406d83df420631ea8d3df5a1eecf07): perf(QOV-2206): resize env warm pods, public HPA cap and engine CPU limit  (Guillaume Dubroeucq)
## Release notes engine v1.360.0   
### Bug fixes   
* [90eb3660449f7c61a53a7c2e95a849da20d35dbd](https://github.com/Qovery/engine/commit/90eb3660449f7c61a53a7c2e95a849da20d35dbd): fix(operator): grant per-worker PDB permissions  (Pierre Gerbelot)
   
### Others   
* [ce5b4f4775607311f233a5417cb245280edcaecb](https://github.com/Qovery/engine/commit/ce5b4f4775607311f233a5417cb245280edcaecb): feat(platform-catalog): publish Karpenter and add optional BYOK layer  (Pierre Gerbelot)
   
* [23c6e6a3e7406d83df420631ea8d3df5a1eecf07](https://github.com/Qovery/engine/commit/23c6e6a3e7406d83df420631ea8d3df5a1eecf07): perf(QOV-2206): resize env warm pods, public HPA cap and engine CPU limit  (Guillaume Dubroeucq)
## Release notes engine v1.359.0   
### Others   
* [aa7a25d0dddc130d025203e8fc99382fcffafc83](https://github.com/Qovery/engine/commit/aa7a25d0dddc130d025203e8fc99382fcffafc83): feat(QOV-2222): extract build settings into dedicated BuildSettings struct  (Killian Colla)
   
* [f8072fe98d38e751203327f57836872af42ab367](https://github.com/Qovery/engine/commit/f8072fe98d38e751203327f57836872af42ab367): feat(platform-catalog): add Karpenter v2 configuration models  (Pierre Gerbelot)
## Release notes engine v1.358.0   
### Others   
* [c0522febae1242052cd781af0b6e1581102921a4](https://github.com/Qovery/engine/commit/c0522febae1242052cd781af0b6e1581102921a4): feat(QOV-2206): size the public env fleet at private parity with overprovisioning  (Guillaume Dubroeucq)
   
* [899c7f8d5869d985e16178b2f13d8caf8bed8f90](https://github.com/Qovery/engine/commit/899c7f8d5869d985e16178b2f13d8caf8bed8f90): feat(platform-catalog): support structured configuration fields  (Pierre Gerbelot)
## Release notes engine v1.357.0   
### Features   
* [7d0a57a8ca569e3e7460df0dff3feaf5e5021756](https://github.com/Qovery/engine/commit/7d0a57a8ca569e3e7460df0dff3feaf5e5021756): feat(blueprint): let a blueprint own its adopted database's DNS name  (Antoine Promerova)
   
### Bug fixes   
* [955870576be8eefd38fcae8937920566e10fba94](https://github.com/Qovery/engine/commit/955870576be8eefd38fcae8937920566e10fba94): fix(aws): tag Karpenter nodes, ECR repositories and S3 buckets with aws-apn-id  (Guillaume Da Silva)
## Release notes engine v1.356.1   
### Others   
* [bc6142c261eacfb605c08c4d26fb311018dd0541](https://github.com/Qovery/engine/commit/bc6142c261eacfb605c08c4d26fb311018dd0541): feat(engine-v2): group Helm deployment logs  (Pierre Gerbelot)
## Release notes engine v1.356.0   
### Others   
* [bc6142c261eacfb605c08c4d26fb311018dd0541](https://github.com/Qovery/engine/commit/bc6142c261eacfb605c08c4d26fb311018dd0541): feat(engine-v2): group Helm deployment logs  (Pierre Gerbelot)
## Release notes engine v1.355.1   
### Others   
* [eb26faae11f9dbf240f1b68183778370ae116cb0](https://github.com/Qovery/engine/commit/eb26faae11f9dbf240f1b68183778370ae116cb0): feat(engine-v2): implement platform preflight checks  (Pierre Gerbelot)
## Release notes engine v1.354.0   
### Bug fixes   
* [14408bb57c28ac8274d5b4164affd5d72a6ddf6e](https://github.com/Qovery/engine/commit/14408bb57c28ac8274d5b4164affd5d72a6ddf6e): fix(tests): use mounted-file fixture without APT dependencies  (Pierre Gerbelot)
   
### Others   
* [d7e6b142fda01d782601934a6df83e16eca5531c](https://github.com/Qovery/engine/commit/d7e6b142fda01d782601934a6df83e16eca5531c): feat(QOV-2210): mount build variables as BuildKit secrets  (Fabien FLEUREAU)
   
* [72d72d2e8cf14b201d7845d77bccdd4def05e9e2](https://github.com/Qovery/engine/commit/72d72d2e8cf14b201d7845d77bccdd4def05e9e2): fix(QOV-1841): drop deprecated azurerm arguments from AKS federated identity credentials  (Guillaume Dubroeucq)
## Release notes engine v1.355.0   
### Internal changes   
* [8924b372ba77b17ee5dd90001ad1a18469ea8d9e](https://github.com/Qovery/engine/commit/8924b372ba77b17ee5dd90001ad1a18469ea8d9e): chore(byok)!: leave Operator installation to platform catalog #BREAKING (Pierre Gerbelot)
## Release notes engine v1.353.2   
### Internal changes   
* [8924b372ba77b17ee5dd90001ad1a18469ea8d9e](https://github.com/Qovery/engine/commit/8924b372ba77b17ee5dd90001ad1a18469ea8d9e): chore(byok)!: leave Operator installation to platform catalog #BREAKING (Pierre Gerbelot)
## Release notes engine v1.353.1   
### Others   
* [86e344da6c7270ba6bf6a70fd69b28b409939eee](https://github.com/Qovery/engine/commit/86e344da6c7270ba6bf6a70fd69b28b409939eee): fix(platform-catalog): grant Operator access to worker Pods  (Pierre Gerbelot)
## Release notes engine v1.353.0   
### Others   
* [ed3fca71a034de0a5c35e89e972bc1eb68c302a2](https://github.com/Qovery/engine/commit/ed3fca71a034de0a5c35e89e972bc1eb68c302a2): feat(engine-v2): clarify platform deployment result  (Pierre Gerbelot)
## Release notes engine v1.352.0   
### Others   
* [2560fe97a5de6a3a2ddacb203b5948f7af723e2d](https://github.com/Qovery/engine/commit/2560fe97a5de6a3a2ddacb203b5948f7af723e2d): feat(engine-v2): add platform preflight execution  (Pierre Gerbelot)
## Release notes engine v1.351.0   
### Others   
* [b55f11cc65ce3e1db1a35a551ab6f23e76f8bef5](https://github.com/Qovery/engine/commit/b55f11cc65ce3e1db1a35a551ab6f23e76f8bef5): feat(QOV-2202): pin the private env + blueprint fleets to qovery-default-private  (Guillaume Dubroeucq)
## Release notes engine v1.350.1   
### Bug fixes   
* [cc9e6c50eeb2e12ffeb6e44784e883e943d59cc5](https://github.com/Qovery/engine/commit/cc9e6c50eeb2e12ffeb6e44784e883e943d59cc5): fix(blueprint): let core own the cluster context variables  (Antoine Promerova)
   
* [c02dfc2c3360d12c90f2f6f06c6b66de9d929888](https://github.com/Qovery/engine/commit/c02dfc2c3360d12c90f2f6f06c6b66de9d929888): fix(security): YAML-escape customer input in service chart templates  (Antoine Promerova)
   
### Internal changes   
* [1f8b6b098ee37d0489976c89a19b777ec6de5844](https://github.com/Qovery/engine/commit/1f8b6b098ee37d0489976c89a19b777ec6de5844): chore: remove Operator image tag fallback  (Pierre Gerbelot)
## Release notes engine v1.350.0   
### Internal changes   
* [0a6ee9d88de4131e5eb2f041541e136b31f74399](https://github.com/Qovery/engine/commit/0a6ee9d88de4131e5eb2f041541e136b31f74399): chore: remove platform agent tag fallbacks  (Pierre Gerbelot)
   
### Others   
* [27eaaa7466b613c2e9ced41c46092991751ea724](https://github.com/Qovery/engine/commit/27eaaa7466b613c2e9ced41c46092991751ea724): feat(qov-2147) Set eso webhook reliable  (Melvin Zottola)
   
* [d035662b81051ff5e22c85a335814677d77afa16](https://github.com/Qovery/engine/commit/d035662b81051ff5e22c85a335814677d77afa16): fix(QOV-2127): do not fail an environment stop on Terraform services  (Fabien FLEUREAU)
## Release notes engine v1.349.1   
### Others   
* [a644a8e08acf05d8bf01deced0373929bcc41c74](https://github.com/Qovery/engine/commit/a644a8e08acf05d8bf01deced0373929bcc41c74): ci(engine): publish worker versions after deployment  (Pierre Gerbelot)
## Release notes engine v1.349.0   
### Others   
* [f4efe8ba4a2b087cff584007db162f7b082e48bb](https://github.com/Qovery/engine/commit/f4efe8ba4a2b087cff584007db162f7b082e48bb): feat(QOV-2201): deploy qovery-engine-env-public fleet via chained CI job  (Guillaume Dubroeucq)
## Release notes engine v1.348.1   
### Bug fixes   
* [49f226163f9008d2f567a3a10f12e4e702828d4f](https://github.com/Qovery/engine/commit/49f226163f9008d2f567a3a10f12e4e702828d4f): fix(blueprint): name the variable that breaks a values.yaml render  (Antoine Promerova)
   
* [9eeba29881358fe884dd85d3f3e5a19e0003a5db](https://github.com/Qovery/engine/commit/9eeba29881358fe884dd85d3f3e5a19e0003a5db): fix(terraform): prevent failed job restarts  (Fabien FLEUREAU)
   
### Others   
* [e05af6ccc177332698fedb4a30ca3f20e4a9d616](https://github.com/Qovery/engine/commit/e05af6ccc177332698fedb4a30ca3f20e4a9d616): fix(platform-catalog): align input types with q-core  (Pierre Gerbelot)
## Release notes engine v1.348.0   
### Others   
* [afcfbbc916880dc51f796c606ef04d642fb38a36](https://github.com/Qovery/engine/commit/afcfbbc916880dc51f796c606ef04d642fb38a36): feat(QOV-2198): add BUILDER_NODE_SELECTOR / BUILDER_TOLERATIONS builder placement plumbing  (Guillaume Dubroeucq)
## Release notes engine v1.347.4   
### Others   
* [5518c14e8030052212a841a1f9883db1c17bd6d5](https://github.com/Qovery/engine/commit/5518c14e8030052212a841a1f9883db1c17bd6d5): ci(engine): keep the build job from being evicted mid-compile  (Antoine Promerova)
   
* [aa52e7dff6ba1b0317baef200e877712a2285983](https://github.com/Qovery/engine/commit/aa52e7dff6ba1b0317baef200e877712a2285983): docs(engine): point deployment failures at the AI Copilot  (Antoine Promerova)
## Release notes engine v1.347.3   
### Bug fixes   
* [600512f9b00222e349e63f3c12024b8efca77479](https://github.com/Qovery/engine/commit/600512f9b00222e349e63f3c12024b8efca77479): fix(engine): bound blueprint preview  (Antoine Promerova)
## Release notes engine v1.347.2   
### Internal changes   
* [fe87e9d3abb26991e47eef152a779319e1e135f1](https://github.com/Qovery/engine/commit/fe87e9d3abb26991e47eef152a779319e1e135f1): chore(ci): renumber deploy jobs so public infra deploys follow the private ones  (Guillaume Dubroeucq)
   
### Others   
* [a820e64c6595c1e0bda2b6814adb51e24a6b21eb](https://github.com/Qovery/engine/commit/a820e64c6595c1e0bda2b6814adb51e24a6b21eb): fix(QOV-2094): set ECR cache retention to 90 days  (Pierre Gerbelot)
## Release notes engine v1.347.1   
### Features   
* [85f53ca80d52fac4bc2c52a4700a1602d4d59a35](https://github.com/Qovery/engine/commit/85f53ca80d52fac4bc2c52a4700a1602d4d59a35): feat: add started_at to StepRecord  (Romain Billard)
   
### Others   
* [01779ec0083787eaba0eb14782a221a4f9a5f2fb](https://github.com/Qovery/engine/commit/01779ec0083787eaba0eb14782a221a4f9a5f2fb): feat(QOV-2094): add ECR cache lifecycle  (Pierre Gerbelot)
   
* [6f26204d1ac26dc0521725776ba0767f57403a53](https://github.com/Qovery/engine/commit/6f26204d1ac26dc0521725776ba0767f57403a53): feat(QOV-2094): add ECR cache rule  (Pierre Gerbelot)
   
* [ab7ac2699a3b60913a3e37039348c1cd0298dead](https://github.com/Qovery/engine/commit/ab7ac2699a3b60913a3e37039348c1cd0298dead): feat(agentic-workflow): inject user environment variables into the workflow job  (Fabien FLEUREAU)
## Release notes engine v1.347.0   
### Internal changes   
* [4a028ce6991b6887c126f012269fd2327ee348eb](https://github.com/Qovery/engine/commit/4a028ce6991b6887c126f012269fd2327ee348eb): chore(rust): bump toolchain to 1.98.0 fix base-ci-engine  (Antoine Promerova)
   
### Others   
* [40541a054c382153a0be0fedf18c8fd100a0cd1f](https://github.com/Qovery/engine/commit/40541a054c382153a0be0fedf18c8fd100a0cd1f): fix(agentic-workflow): send periodic deployment status reports  (Fabien FLEUREAU)
## Release notes engine v1.346.2   
### Internal changes   
* [4a028ce6991b6887c126f012269fd2327ee348eb](https://github.com/Qovery/engine/commit/4a028ce6991b6887c126f012269fd2327ee348eb): chore(rust): bump toolchain to 1.98.0 fix base-ci-engine  (Antoine Promerova)
   
### Others   
* [40541a054c382153a0be0fedf18c8fd100a0cd1f](https://github.com/Qovery/engine/commit/40541a054c382153a0be0fedf18c8fd100a0cd1f): fix(agentic-workflow): send periodic deployment status reports  (Fabien FLEUREAU)
## Release notes engine v1.346.1   
### Others   
* [2e6f0d29ba4a7fd1518c94f876256c748009ad74](https://github.com/Qovery/engine/commit/2e6f0d29ba4a7fd1518c94f876256c748009ad74): docs: refresh the public engine README  (benjaminch)
   
* [afc532618c538ceeb124abc0f3ae6cd7ab93e629](https://github.com/Qovery/engine/commit/afc532618c538ceeb124abc0f3ae6cd7ab93e629): fix(eks-anywhere): validate Bottlerocket templates per machine group  (Pierre Gerbelot)
## Release notes engine v1.346.0   
### Others   
* [a433f7f82cddb06a807fc7b4e211767ff1c5a802](https://github.com/Qovery/engine/commit/a433f7f82cddb06a807fc7b4e211767ff1c5a802): feat(QOV-2179): ship public infra fleets with every release + pin private fleets to qovery-default-private  (Guillaume Dubroeucq)
## Release notes engine v1.345.2   
### Others   
* [beb18b8ba3cb899cffbeb572a12c805d3dd0fead](https://github.com/Qovery/engine/commit/beb18b8ba3cb899cffbeb572a12c805d3dd0fead): chore(QOV-2104): qovery demo to use envoy  (benjaminch)
## Release notes engine v1.345.1   
### Others   
* [a637086475cdcd2b41eb70cc51f2c8e378f8a03e](https://github.com/Qovery/engine/commit/a637086475cdcd2b41eb70cc51f2c8e378f8a03e): chore(gke-envoy): listenerset workaround referencegrant  (benjaminch)
## Release notes engine v1.345.0   
### Features   
* [31e0217e11b0a36b453a9c78a3b4a5015e9441a5](https://github.com/Qovery/engine/commit/31e0217e11b0a36b453a9c78a3b4a5015e9441a5): feat(catalog): add demo platform template  (Pierre Gerbelot)
   
* [806652c28124e4dd560554d1852b0691ff2d51ce](https://github.com/Qovery/engine/commit/806652c28124e4dd560554d1852b0691ff2d51ce): feat(catalog): align demo values  (Pierre Gerbelot)
## Release notes engine v1.344.0   
### Others   
* [2b08e701b90ef5f05ebbd9f4df28f188c3835023](https://github.com/Qovery/engine/commit/2b08e701b90ef5f05ebbd9f4df28f188c3835023): feat(QOV-2133): add qovery-engine-infra-public pilot fleet deploy jobs  (Guillaume Dubroeucq)
   
* [67ae9b00157159da7df947e2d3c9463df9c3de15](https://github.com/Qovery/engine/commit/67ae9b00157159da7df947e2d3c9463df9c3de15): feat(platform-catalog): carry the demo worker config in a qovery-demo overlay  (Pierre Gerbelot)
## Release notes engine v1.343.0   
### Others   
* [7873d3593ec9194f1b12d86e490ae088f48f929b](https://github.com/Qovery/engine/commit/7873d3593ec9194f1b12d86e490ae088f48f929b): feat(qov-2169) Support managed postgres 18  (Melvin Zottola)
## Release notes engine v1.342.0   
### Others   
* [a9feb0c5452dec02b48d745220a12171fa2b97f1](https://github.com/Qovery/engine/commit/a9feb0c5452dec02b48d745220a12171fa2b97f1):  feat(platform-catalog): configure engine image tag suffix  (Pierre Gerbelot)
   
* [38da3e96dd4d5bc1cdfbb7e8652afe77c8ca0287](https://github.com/Qovery/engine/commit/38da3e96dd4d5bc1cdfbb7e8652afe77c8ca0287): fix(platform-catalog): allow empty engine image tag suffix  (Pierre Gerbelot)
## Release notes engine v1.341.0   
### Others   
* [a4058d4521d8a43d3266638bb6cc41c6c74125ba](https://github.com/Qovery/engine/commit/a4058d4521d8a43d3266638bb6cc41c6c74125ba): feat(qov-2146) Send warning instead of err for external secret issue  (Melvin Zottola)
## Release notes engine v1.340.0   
### Bug fixes   
* [9e6d0910afb444842f9b5da81db57193c684dbcc](https://github.com/Qovery/engine/commit/9e6d0910afb444842f9b5da81db57193c684dbcc): fix(operator): report deployed image tag  (Pierre Gerbelot)
   
### Internal changes   
* [0936cb2dbe6ae4f2e42775236740ff76a7d60dcb](https://github.com/Qovery/engine/commit/0936cb2dbe6ae4f2e42775236740ff76a7d60dcb): chore: Set dry run loki base url envoy  (Melvin Zottola)
   
### Others   
* [b440f627aceac1adc5b3edf2aecb1167e48acf45](https://github.com/Qovery/engine/commit/b440f627aceac1adc5b3edf2aecb1167e48acf45): feat(qov-2146) Send error message on external secret install error  (Melvin Zottola)
## Release notes engine v1.339.0   
### Others   
* [d1e3e328eeefc16cd5e8fe8af56e49e43370178e](https://github.com/Qovery/engine/commit/d1e3e328eeefc16cd5e8fe8af56e49e43370178e): feat(QOV-2154): tolerate Karpenter override blocks holding only spot_enabled  (Guillaume Dubroeucq)
429: Too Many Requests
For more on scraping GitHub and how it may affect your rights, please review our Terms of Service (https://docs.github.com/en/site-policy/github-terms/github-terms-of-service).
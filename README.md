# kube-autorollout

![Build Status](https://github.com/juv/kube-autorollout/actions/workflows/docker-publish.yml/badge.svg)
![GitHub License](https://img.shields.io/github/license/juv/kube-autorollout?color=blue)
![Helm Chart](https://img.shields.io/badge/Helm_Chart-available-blue)
![Docker Images](https://img.shields.io/badge/Docker_images-GHCR-blue?logo=docker)
![Rust](https://shields.io/badge/-Rust-3776AB?style=flat&logo=rust&color=blue)

A lightweight Kubernetes controller that automatically triggers Kubernetes `Deployment` rollouts when container image
_digests_
change, ensuring your applications stay up-to-date without manual intervention 🚀

## Overview

kube-autorollout monitors Kubernetes deployments and automatically triggers rollouts when new container image versions
are available. Unlike traditional image update mechanisms that require changing tags, this tool is built to compare
container image _digests_ for the same, static tag.

When to use kube-autorollout?

- x
- y

## tl;dr

1) Install kube-autorollout using the Helm Chart, configure container registries
2) Select `Deployment` resources for auto-rollout by adding the label `kube-autorollout/enabled=true`
3) Push images to your container registry with the same _static_ tag
4) ???
5) Profit

### Key Features

- **Digest-based updates**: Monitors container image digests rather than semver tags by using the manifest endpoint of
  the [OCI Distribution Specification](https://github.com/opencontainers/distribution-spec/blob/main/spec.md), successor
  of
  the [Docker Registry HTTP API v2](https://github.com/distribution/distribution/blob/5cb406d511b7b9163bff9b6439072e4892e5ae3b/docs/spec/api.md)
- **Label-based selection**: Uses Kubernetes labels to selectively monitor deployments
- **Multiple OCI registry support**: Supports multiple container registries in a single instance of kube-autorollout.
  Including Docker Hub, GitHub Container
  Registry (
  GHCR.io), JFrog Artifactory, and custom registries
- **GitOps compatiblity**: Compatible to GitOps tools like ArgoCD and FluxCD
- **JFrog Artifactory compatiblity**: Special handling for JFrog Artifactory with Repository Path Method
- **Multi-container rollout**: Special handling for JFrog Artifactory with Repository Path Method
- **Flexible authentication**: Supports various authentication methods including API tokens, personal access tokens, and
  OAuth2 flows
- **Cron-based scheduling**: Configurable scheduling of the main controller loop with cron expressions
- **Custom CA certificates**: Support for custom certificate authority certificates for secure TLS connections to
  private registries
- **Lightweight**: Low container image size, low memory and cpu footprint

## Installation

### Using Helm

kube-autorollout is supposed to be installed using the [Helm Chart](charts/kube-autorollout).

kube-autorollout is meant to be installed in each namespace where you want to enable rollouts.

```bash

# todo 
```

### Configuration

Create a values file that covers all registries for your deployments that are labeled with
`kube-autorollout/enabled=true`.

For full field reference, see the [Helm Chart](charts/kube-autorollout) README.

```yaml
webserver:
  port: 8080

registries:
  #JFrog Artifactory registry with "subdomain method for docker" https://jfrog.com/help/r/jfrog-artifactory-documentation/the-subdomain-method-for-docker
  - hostnamePattern: "*.artifactory.example.com"
    secret:
      # -- Kubernetes Secret name to reference that contains the Docker Registry API token
      name: kube-autorollout-jfrog-api-token
      # -- The key to reference in the secret, will be referenced in the config automatically if .token is unset
      key: REGISTRY_TOKEN

  #JFrog Artifactory registry with "repository path method for docker" https://jfrog.com/help/r/jfrog-artifactory-documentation/the-repository-path-method-for-docker
  - hostnamePattern: "another-artifactory.example.com"
    secret:
      name: kube-autorollout-jfrog-api-token
      key: REGISTRY_TOKEN

  - hostnamePattern: "ghcr.io"
    username: "github-username-123"
    secret:
      name: kube-autorollout-github-pat
      key: PERSONAL_ACCESS_TOKEN

  - hostnamePattern: "docker.io"
    username: "docker-username-12345"
    secret:
      name: kube-autorollout-docker-io-pat
      key: PERSONAL_ACCESS_TOKEN

featureFlags:
  #Enables a fallback for Artifactory's "repository path method for docker" setup
  enableJfrogArtifactoryFallback: true
```

kube-autorollout expects your Kubernetes secrets to be existing before installing the Helm Chart.
For a quick start, you can create the above-mentioned secret examples like this:

JFrog Artifactory:

```
kubectl create secret generic kube-autorollout-jfrog-api-token --from-literal=REGISTRY_TOKEN=<jfrog-identity-token-here>
```

GitHub personal access token:

```
kubectl create secret generic kube-autorollout-github-pat --from-literal=PERSONAL_ACCESS_TOKEN=<github-personal-access-token-here>
```

Docker personal access token:

```
kubectl create secret generic kube-autorollout-docker-io-pat --from-literal=PERSONAL_ACCESS_TOKEN=<docker-personal-access-token-here>
```

### Select `Deployment` resources for auto-rollout

After configuring your registry credentials, add the **label** `kube-autorollout/enabled=true` to any of your
deployments.
That's it. Your pods can have any number of containers. Your image tag can be any static tag, it does not necessarily be
`latest`.
Example:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: my-app
  labels:
    kube-autorollout/enabled: "true"
spec:
  # ...
  template:
    #  ...
    spec:
      containers:
        - name: my-app
          image: ghcr.io/myorg/my-app:latest
```

### Environment Variables

- `CONFIG_FILE`: The Helm chart automatically configures the required `CONFIG_FILE` environment variable automatically
- Registry secrets are mounted as pod environment variables and referenced in the application config using
  `${ENV_VAR_NAME}` syntax automatically

## Supported container registries

- **Docker Hub** (`docker.io` / `registry-1.docker.io`) - Requires username and personal access token
- **GitHub Container Registry** (`ghcr.io`) - Requires username and personal access token
- **JFrog Artifactory** - Requires an identity token. Supports both
  the [subdomain method for docker](https://jfrog.com/help/r/jfrog-artifactory-documentation/the-subdomain-method-for-docker)
  and [repository path method for docker](https://jfrog.com/help/r/jfrog-artifactory-documentation/the-repository-path-method-for-docker)
  setup

Other registries are untested but potentially work in some combination as long as they follow the
the [OCI Distribution Specification](https://github.com/opencontainers/distribution-spec/blob/main/spec.md) or
[Docker Registry HTTP API v2](https://github.com/distribution/distribution/blob/5cb406d511b7b9163bff9b6439072e4892e5ae3b/docs/spec/api.md)
, please create a pull request to this README.md file to let other users know that a certain registry is supported -
thanks :-).

## Security considerations

- Store sensitive tokens in Kubernetes secrets rather than plain text
- Use least-privilege access tokens for registry authentication
- Regularly rotate your tokens
- Consider using image signatures for additional security

## Metrics

todo

## Troubleshooting

1. Registry authentication failures
    - Verify token validity and permissions
    - Check hostname pattern matching
    - Ensure correct secrets are referenced in your Helm values

2. No `Deployment` rollouts occur
    - Ensure kube-autorollout is running in the correct Kubernetes namespace
    - Verify the `kube-autorollout/enabled=true` label is present on each `Deployment` of interest
    - Check kube-autorollout log for error messages
    - Check RBAC permissions for your kube-autorollout `serviceaccount` in case you are not using the
      `rbac.enabled=true` Helm Chart configuration
    - Check the cache settings for image metadata of your registry
    - Push your image, duh

## License

This project is licensed under the Apache License 2.0 - see [LICENSE](LICENSE).

## Support

- Report bugs and feature requests in [GitHub issues](https://github.com/juv/kube-autorollout/issues)
- Ask questions in the [GitHub discussions](https://github.com/juv/kube-autorollout/discussions)

## Roadmap

- [ ] Support for StatefulSets and DaemonSets
- [ ] Prometheus metrics exporter
- [ ] Prometheus alerts in the Helm Chart
- [ ] Rollout threshold per deployment
- [ ] Webhook-based trigger mechanisms
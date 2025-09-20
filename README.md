# kube-autorollout

![Rust](https://shields.io/badge/-Rust-3776AB?style=flat&logo=rust&color=blue)
![Build Status](https://github.com/juv/kube-autorollout/actions/workflows/docker-publish.yml/badge.svg)
[![GitHub License](https://img.shields.io/github/license/juv/kube-autorollout?color=blue)](./LICENSE)
[![Docker Images](https://img.shields.io/badge/Docker_images-GHCR-blue?logo=docker)](https://github.com/juv/kube-autorollout/pkgs/container/kube-autorollout)
[![Artifact Hub](https://img.shields.io/endpoint?color=blue&url=https://artifacthub.io/badge/repository/kube-autorollout)](https://artifacthub.io/packages/search?repo=kube-autorollout)

A lightweight Kubernetes controller that automatically triggers Kubernetes `Deployment` rollouts when container image
_digests_ change, ensuring your applications stay up-to-date without manual intervention 🚀

## Overview

kube-autorollout monitors Kubernetes deployments and automatically triggers rollouts when new container image versions
are available. Unlike other image update mechanisms that require changing tags via semver version bump, this tool
is built to compare container [image digests](https://docs.docker.com/dhi/core-concepts/digests/) (`@sha256:...`) for
the same, static tag.

When to use kube-autorollout?

- Deploying your application's frequently changing, static tag like `latest`, `main`, `nightly`, etc. and you want
  your up-to-date baseline being executed in the Kubernetes cluster. Particularly suited for development environments.
- CI/CD pipelines are less complex and stay declarative. No imperative tasks, no fake Helm chart version bumps, no
  additional git commits in your pipelines to trigger rollouts
- Immediate feedback loop in combination with your existing Prometheus alerts, e.g. "pod is stuck in a crash loop" or "
  ArgoCD application going into degraded health state"
- ArgoCD Image Updater only supports ArgoCD applications but your development environments contains both ArgoCD
  applications as well as manually installed Helm chart releases, for which you want automated rollouts.
  kube-autorollout will automate rollouts for the supported Kubernetes resources, no matter which tool installed
  them in the first place.

## tl;dr

1) Install kube-autorollout using the Helm chart, configure container registries
2) Target `Deployment` resources for auto-rollouts by adding the label `kube-autorollout/enabled=true`
3) Push images to your container registry with the same **static** tag, e.g. `latest`, `main`, `nightly`
4) ???
5) Profit

## Key Features

- **Digest-based updates**: Monitors container [image digests](https://docs.docker.com/dhi/core-concepts/digests/)
  rather than semver tags by using the manifests endpoint of
  the [OCI Distribution Specification](https://github.com/opencontainers/distribution-spec/blob/main/spec.md), which can
  be seen as a more vendor-neutral, interoperable standard of
  the [Docker Registry HTTP API v2](https://github.com/distribution/distribution/blob/5cb406d511b7b9163bff9b6439072e4892e5ae3b/docs/spec/api.md)
- **Label-based selection**: Uses Kubernetes labels to selectively monitor deployments
- **Multiple OCI registry support**: Supports multiple container registries in a single instance of kube-autorollout.
  Including Docker Hub (docker.io, registry-1.docker.io), GitHub Container Registry (ghcr.io), JFrog Artifactory, and
  custom registries as long as they implement the OCI Distribution Specification
- **GitOps compatiblity**: Compatible to GitOps tools like ArgoCD and FluxCD
- **JFrog Artifactory compatiblity**: Special handling for JFrog Artifactory
  with a configuration of
  the [repository path method for docker](https://jfrog.com/help/r/jfrog-artifactory-documentation/the-repository-path-method-for-docker)
- **Multi-container rollout**: Supports automated rollouts for Deployments with a pod template containing multiple
  containers
- **Flexible authentication**: Supports various authentication methods including API tokens, personal access tokens, and
  OAuth2 flows
- **Cron-based scheduling**: Configurable scheduling of the main controller loop with cron expressions
- **Custom CA certificates**: Support for custom certificate authority certificates for secure TLS connections to
  private registries
- **Lightweight**: Low container image size, low memory and cpu footprint

## How does it work?

todo

## Installation

### Using Helm

kube-autorollout is supposed to be installed using the [Helm chart](charts/kube-autorollout).

kube-autorollout is supposed to be installed in each Kubernetes namespace where you want to enable automated rollouts.

```bash

# todo 
```

### Configuration

Create a Helm values file that covers all registries for your deployments that are labeled with
`kube-autorollout/enabled=true`.

For full field reference, see the [Helm chart](charts/kube-autorollout) README.

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
      key: IDENTITY_TOKEN

  #JFrog Artifactory registry with "repository path method for docker" https://jfrog.com/help/r/jfrog-artifactory-documentation/the-repository-path-method-for-docker
  - hostnamePattern: "another-artifactory.example.com"
    secret:
      name: kube-autorollout-jfrog-api-token
      key: IDENTITY_TOKEN

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
  #Enables an automated fallback for Artifactory's "repository path method for docker" setup
  enableJfrogArtifactoryFallback: true
```

kube-autorollout expects your Kubernetes secrets to be existing before installing the Helm chart.
For a quick start, you can create the above-mentioned secret examples like this:

JFrog Artifactory:

```
kubectl create secret generic kube-autorollout-jfrog-api-token --from-literal=IDENTITY_TOKEN=<jfrog-identity-token-here>
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
          # ...
        - name: another-app
          image: ghcr.io/another-org/another-app:main
```

### Environment Variables

- `CONFIG_FILE`: The Helm chart automatically configures the required `CONFIG_FILE` environment variable automatically
  and mounts the config file into the kube-autorollout pod
- Registry secrets are mounted as pod environment variables and referenced in the application config using
  `${ENV_VAR_NAME}` syntax automatically

## Supported container registries

- **Docker Hub** (`docker.io` / `registry-1.docker.io`) - Requires username and personal access token
- **GitHub Container Registry** (`ghcr.io`) - Requires username and personal access token
- **JFrog Artifactory** - Requires an Artifactory identity token. Supports both
  the [subdomain method for docker](https://jfrog.com/help/r/jfrog-artifactory-documentation/the-subdomain-method-for-docker)
  and [repository path method for docker](https://jfrog.com/help/r/jfrog-artifactory-documentation/the-repository-path-method-for-docker)
  setup

Other registries are untested but likely work in some combination as long as they follow the
the [OCI Distribution Specification](https://github.com/opencontainers/distribution-spec/blob/main/spec.md), please
create a pull request to this README.md file to let other users know that a certain registry is supported -
thank you :-).

## Security considerations

- Store sensitive tokens in Kubernetes secrets rather than plain text in the Helm chart
- Use least-privilege api tokens for registry authentication
- Regularly rotate your api tokens

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
    - Make sure you pushed your image, duh
    - Check kube-autorollout log for error messages
    - Check RBAC permissions for your kube-autorollout `serviceaccount` in case you are not using the
      `rbac.enabled=true` Helm chart configuration
    - Check the cache settings for image metadata of your registry

## License

This project is licensed under the Apache License 2.0 - see [LICENSE](LICENSE).

## Support

- Report bugs and feature requests in [GitHub issues](https://github.com/juv/kube-autorollout/issues)
- Ask questions in the [GitHub discussions](https://github.com/juv/kube-autorollout/discussions)
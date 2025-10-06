# kube-autorollout

![Rust](https://shields.io/badge/-Rust-3776AB?style=flat&logo=rust&color=blue)
![Build Status](https://github.com/juv/kube-autorollout/actions/workflows/docker-publish.yml/badge.svg)
[![GitHub License](https://img.shields.io/github/license/juv/kube-autorollout?color=blue)](./LICENSE)
[![Docker Images](https://img.shields.io/badge/Docker_images-GHCR-blue?logo=docker)](https://github.com/juv/kube-autorollout/pkgs/container/kube-autorollout)
[![Artifact Hub](https://img.shields.io/endpoint?color=blue&url=https://artifacthub.io/badge/repository/kube-autorollout)](https://artifacthub.io/packages/search?repo=kube-autorollout)
[![crates.io](https://img.shields.io/crates/v/kube-autorollout.svg?color=blue)](https://crates.io/crates/kube-autorollout)

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

1) Install kube-autorollout using the Helm chart, [configure container registries](#Configuration)
2) Target `Deployment` resources for auto-rollouts by adding the label `kube-autorollout/enabled=true`
3) Push images to your container registry with the same **static** tag, e.g. `latest`, `main`, `nightly`
4) ???
5) Profit

## Key Features

- **Digest-based updates**: Monitors container [image digests](https://docs.docker.com/dhi/core-concepts/digests/)
  rather than SemVer tags by using the manifests endpoint of
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
- **Cron-based scheduling**: Configurable scheduling of the main controller loop with cron expressions
- **Custom CA certificates**: Support for custom certificate authority certificates for secure TLS connections to
  private registries
- **Lightweight**: Low container image size (~9 MB), low memory and cpu footprint

## How does it work?

//todo: add diagram, description

## Installation

### Using Helm

kube-autorollout is supposed to be installed using the [Helm chart](charts/kube-autorollout).

kube-autorollout is supposed to be installed in each Kubernetes namespace where you want to enable automated rollouts.

The Helm Chart is available on Artifact Hub:
[![Artifact Hub](https://img.shields.io/endpoint?color=blue&url=https://artifacthub.io/badge/repository/kube-autorollout)](https://artifacthub.io/packages/search?repo=kube-autorollout)

### Configuration

Create a Helm values file/override that covers all registries for your deployments that are labeled with
`kube-autorollout/enabled=true`. For some quick examples, see the snippet below.

For full field reference, see the [Helm chart](charts/kube-autorollout) README.

```yaml
config:
  registries:
    # -- GitHub container registry with ImagePullSecret
    - hostnamePattern: "ghcr.io"
      secret:
        # -- REQUIRED: The type of the secret - ImagePullSecret, Opaque, None. <ImagePullSecret> must define keys "name" and "mountPath". <Opaque> with Kubernetes Secret must define keys "name" and "key", optionally "username". <Opaque> with hardcoded token must define keys "token". <None> will ignore authentication to the registry.
        type: ImagePullSecret
        # -- ImagePullSecret secret name to reference that contains the ghcr.io docker config
        name: ghcr-io-registry-creds
        # -- REQUIRED FOR <ImagePullSecret>: The mount path of the ImagePullSecret within the kube-autorollout pod. Must be unique across registry secrets.
        mountPath: /etc/secrets/registries/ghcr.io

    # -- DockerHub registry with ImagePullSecret, covers both docker.io and registry-1.docker.io
    - hostnamePattern: "docker.io"
      secret:
        type: ImagePullSecret
        name: docker-io-registry-creds
        mountPath: /etc/secrets/registries/docker.io

    # -- Wildcard-match for JFrog Artifactory registry with "subdomain method for docker" https://jfrog.com/help/r/jfrog-artifactory-documentation/the-subdomain-method-for-docker
    - hostnamePattern: "*.artifactory.example.com"
      secret:
        type: Opaque
        # -- Kubernetes Secret name of secret type Opaque to reference. The secret should contain the Docker Registry API token, personal access token, JFrog Artifactory identity token, etc.
        name: jfrog-artifactory-registry-creds
        # -- OPTIONAL FOR <Opaque>: The key to reference of the secret. Will be referenced in the config automatically if .token is unset
        key: IDENTITY_TOKEN

    # -- JFrog Artifactory registry with "repository path method for docker" https://jfrog.com/help/r/jfrog-artifactory-documentation/the-repository-path-method-for-docker
    - hostnamePattern: "another-artifactory.example.com"
      secret:
        name: jfrog-artifactory-registry-creds
        key: IDENTITY_TOKEN

  featureFlags:
    # -- Enables an automated fallback for Artifactory's "repository path method for docker" setup
    enableJfrogArtifactoryFallback: true
```

kube-autorollout expects your Kubernetes secrets to be existing before installing the Helm chart.
For a quick start, you can create the above-mentioned secret examples like this:

JFrog Artifactory, secret type `Opaque`:

```bash
kubectl create secret generic jfrog-artifactory-registry-creds --from-literal=IDENTITY_TOKEN=<jfrog-identity-token-here>
```

GitHub personal access token, secret type `ImagePullSecret`:

```bash
kubectl create secret docker-registry ghcr-io-registry-creds --docker-server=https://ghcr.io --docker-username=<github-username-here> --docker-password=<github-personal-access-token-here>
```

Docker personal access token, secret type `ImagePullSecret`:

```bash
kubectl create secret docker-registry docker-io-registry-creds --docker-server=https://docker.io --docker-username=<docker-io-username-here> --docker-password=<docker-io-personal-access-token-here>
```

### Select `Deployment` resources for auto-rollout

After configuring your registry credentials, add the **label** `kube-autorollout/enabled=true` to any of your
deployments.
That's it. Your pods can have any number of containers. Your image tag can be any static tag, it does not necessarily be
`latest`.

kube-autorollout will print warnings into the log for containers that do not set `imagePullPolicy: Always`. Make sure
you set that imagePullPolicy, otherwise the updated
image [will not be downloaded](https://kubernetes.io/docs/concepts/containers/images/#image-pull-policy) by the kubelet
upon next pod creation. Example:

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
          imagePullPolicy: Always
          # ...
        - name: another-container
          image: ghcr.io/another-org/whatever:main
          imagePullPolicy: Always
```

### GitOps state drift detection support (ArgoCD and FluxCD compatibility)

To ensure compatibility to the state drift detection in GitOps tools like ArgoCD and FluxCD, enable the feature flag
`enableKubectlAnnotation` in your Helm Chart values file:

```yaml
#...
config:
  #...
  featureFlags:
    #...
    enableKubectlAnnotation: true
```

This changes the kube-autorollout patch `annotation` key (that internally triggers the redeployment of the pods) from
`kube-autorollout/restartedAt` to `kubectl.kubernetes.io/restartedAt`.
The latter annotation is applied by `kubectl` when executing the command `kubectl rollout restart`.
Most GitOps tools like ArgoCD and FluxCD ignore the kubectl annotation from state drift detection. If you are not using
this value on "true" you might need to add further configuration to ArgoCD and FluxCD to not show the kube-autorollout
annotation as a state drift.

### Custom CA certificates

When connecting to private registries that present a TLS certificate that is not signed by a well-known/public
certificate authority, you need to provide the custom ca certificates as part of the Helm Chart values.

```yaml 
#...
config:
  #...
  tls:
    customCaCertificates:
      enabled: true
      secrets:
        - # -- The name of the secret to reference that includes the custom CA certificate chain
          name: custom-ca-01-secret
          # -- The key / subPath within the secret to mount in kube-autorollout
          subPath: ca-01.crt
          # -- The mountPath within kube-autoroll, will be auto-wired in the config
          mountPath: /etc/secrets/ca/custom-ca-01.crt
```

This snippet will mount the subPath `ca-01.crt` of Kubernetes secret `custom-ca-01-secret` into the kube-autorollout
pod. The `mountPath` needs to be a unique value when multiple ca certificates are mounted. The Helm Chart is auto-wiring
all `mountPath` values into the config file automatically.

kube-autorollout expects your Kubernetes secrets to be existing before installing the Helm chart. For a quick start, you
can create the above-mentioned secret example like this:

```bash
kubectl create secret generic custom-ca-01-secret --from-file=ca-01.crt={path/to/ca-01.crt}
```

## Supported container registries

- **Docker Hub** (`docker.io` / `registry-1.docker.io`) - Requires username and personal access token
- **GitHub Container Registry** (`ghcr.io`) - Requires username and personal access token
- **JFrog Artifactory** - Requires an Artifactory identity token. Both
  the [subdomain method for docker](https://jfrog.com/help/r/jfrog-artifactory-documentation/the-subdomain-method-for-docker)
  and [repository path method for docker](https://jfrog.com/help/r/jfrog-artifactory-documentation/the-repository-path-method-for-docker)
  setups are supported.

Other registries are untested but likely work in some combination as long as they follow the
the [OCI Distribution Specification](https://github.com/opencontainers/distribution-spec/blob/main/spec.md), please
create a pull request to this README.md file to let other users know that a certain registry is supported -
thank you :-).

## Security considerations

- Store sensitive tokens in Kubernetes secrets rather than plain text in the Helm chart values
- Use least-privilege api tokens for registry authentication
- Regularly rotate your api tokens
- Make sure to cover the secrets used in the kube-autorollout configuration as part of your "token rotation" process.
  These secrets might go unnoticed after a while and use expired api tokens, potentially causing locked user accounts.
  This is especially the case for self-hosted JFrog Artifactory registries, where a handful of forbidden requests lock
  the entire user account until manual intervention of an admin.

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

## Development

### Building from source

```bash
# Clone the repository
git clone https://github.com/juv/kube-autorollout.git
cd kube-autorollout

# Build the binary
cargo build --release

# Build Docker image
docker build -t kube-autorollout:latest .
```

### Running tests

```bash
# Run tests
cargo test
```

### Executing locally

To execute kube-autorollout locally, set these environment variables:

- `CONFIG_FILE`: Required -- the file path to the config file. Minimal config:

```yaml 
cronSchedule: "*/45 * * * * *"
webserver:
  port: 8080
registries:
  - hostnamePattern: "docker.io"
    secret:
      type: ImagePullSecret
      mountPath: "C:/Users/<YourUser>/Desktop/kube-autorollout/docker-io"
  - hostnamePattern: "*.artifactory.example.com"
    secret:
      type: Opaque
      token: ${REGISTRY_TOKEN}
  - hostnamePattern: "ghcr.io"
    secret:
      type: ImagePullSecret
      mountPath: "C:/Users/<YourUser>/Desktop/kube-autorollout/ghcr-io"
tls:
  caCertificatePaths: [ ]
featureFlags:
  enableJfrogArtifactoryFallback: false
  enableKubectlAnnotation: false
```

- Registry secrets of type `Opaque` should be present as environment variables and referenced in the application config
  using `${ENV_VAR_NAME}` syntax
- Registry secrets of type `ImagePullSecret` must include a `mountPath` that points to an existing folder, which
  includes a file `.dockerconfigjson` with a content like this:

```json
{
  "auths": {
    "your.private.registry.example.com": {
      "username": "janedoe",
      "password": "xxxxxxxxxxx",
      "email": "jdoe@example.com",
      "auth": "c3R...zE2"
    }
  }
}
```

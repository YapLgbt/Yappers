# Yap

Yap is a neighborhood social app for queer adults. People post local plans, start conversations and find groups on iPhone, Android, Mac or the web. A *home* is the server that accepts your posts and shares them with the communities it hosts. The public home is at yap.lgbt. Outside peering is currently closed.

[Open Yap in your browser](https://yap.lgbt/app/) · [Features by platform](PRODUCT.md) · [Peering covenant](https://yap.lgbt/peering/)

## Run on an iPhone simulator

On an Apple Silicon Mac, install full Xcode, Homebrew, ripgrep and rustup. Then run these commands from the project directory.

```sh
./scripts/bootstrap.sh
open iosApp/Yappers.xcodeproj
```

Select the **Yappers** scheme and an iPhone simulator, then press **⌘R**. The bootstrap script sets up JDK 21, XcodeGen and the Rust iOS targets. To run on a physical iPhone, select your development team in Xcode’s Signing & Capabilities.

For other targets, see [Android tester APKs](docs/android.md), [macOS and IntelliJ setup](docs/desktop.md) or [building the browser app](docs/web-app.md#website-and-pwa). To try federation on your Mac, start two local homes with `./yap start` and check them with `./yap doctor`. [Local home setup and invitations](docs/local-federation.md)

## Run a private home with Docker Compose

From the repository root, build and start a private home, then copy its invitation.

```sh
docker compose up -d --build --wait
docker compose cp home:/var/lib/yap/home/join-0.json "$HOME/yap-home-invite.json"
```

Paste the invitation JSON into **You → Join community** in Yap. Treat the invitation like a password. The home uses Iroh’s public relays and needs outbound internet access. No host ports are published.

Run `docker compose -p yap-home-b up -d --build --wait` for a second independent home. Use the same `-p` name when copying its invitation or managing it. Each project has its own Home data volume and a separate private report inbox volume. `docker compose down` preserves both volumes. `docker compose down -v` deletes them.

To manage a running Home in your browser, start the optional admin service and issue a one-time code.

```sh
docker compose up -d --build admin
docker compose exec admin yappers-server admin code --config /var/lib/yap/home/config.json
```

Open `http://127.0.0.1:8787` and paste the code. The admin service uses the host network so it can bind directly to host loopback. On Docker Desktop 4.34 or later, enable host networking in Settings. Set `YAP_ADMIN_PORT` when starting another project's admin service if port 8787 is already in use. Use `docker compose stop admin` to close the listener. [Docker host networking](https://docs.docker.com/engine/network/drivers/host/#platform-support) · [Home admin guide](docs/netkit/operations.md#browser-admin)

Compose Homes accept in-app reports by default. Review them under **Reports** in Home admin. Set `YAP_REPORT_INBOX=` in the Compose environment and recreate both services to disable reporting on an unstaffed Home.

### Update using a published container

From the repository root, choose a [published Home image](https://github.com/YapLgbt/Yappers/pkgs/container/yappers-home) by version or digest. Keep the same Compose project name when updating an existing Home. Take a [protected backup](docs/netkit/deletion.md#storage-and-backup) before changing versions.

```sh
export YAP_HOME_IMAGE=ghcr.io/yaplgbt/yappers-home:0.1.0
docker compose -f compose.yaml -f deploy/compose/registry.yaml --profile admin pull
docker compose -f compose.yaml -f deploy/compose/registry.yaml --profile admin up -d --wait
```

Omit `--profile admin` if you only run the Home. The pull runs while the old Home is serving. Compose preserves both volumes when it replaces the running containers.

## How it works

The native apps share a Kotlin UI in `composeApp/`. The browser app loads the Rust client as WebAssembly. In `netkit/`, clients sign events and homes verify and replicate them over Iroh. The home executable and operator CLI live in `backend/`.

- [Networking and federation](docs/netkit/README.md)
- [Account ownership and recovery](docs/ownership.md)
- [What a peer can verify after deletion](docs/netkit/deletion.md)
- [Development guide](docs/development.md) and [test commands](docs/netkit/validation.md)

## License

The home server is licensed under AGPL-3.0-only. The shared Rust client and protocol code are available under MIT or Apache-2.0. [License details](LICENSING.md) · [Download the running server source on GitHub](https://github.com/YapLgbt/Yappers/releases/tag/home-v0.1.0)

# Yap

Yap is a neighborhood social app for queer adults. People post local plans, start conversations and find groups on iPhone, Android, Mac or the web. A *home* is the server that accepts your posts and shares them with the communities it hosts. The public Home at yap.lgbt automatically admits Homes that sign the peering covenant.

[Open Yap in your browser](https://yap.lgbt/app/) · [Run your own Home](docs/run-home.md) · [Features by platform](PRODUCT.md) · [Peering covenant](https://yap.lgbt/peering/)

## Run a private home with Docker Compose

Use the published Linux x86-64 image to start a Home and sign in to its admin panel. The guide covers inviting members, reviewing reports, updating the Home and joining the public Yap network. [Run your own Home](docs/run-home.md)

## Run on an iPhone simulator

On an Apple Silicon Mac, install full Xcode, Homebrew, ripgrep and rustup. Then run these commands from the project directory.

```sh
./scripts/bootstrap.sh
open iosApp/Yappers.xcodeproj
```

Select the **Yappers** scheme and an iPhone simulator, then press **⌘R**. The bootstrap script sets up JDK 21, XcodeGen and the Rust iOS targets. To run on a physical iPhone, select your development team in Xcode’s Signing & Capabilities.

For other targets, see [Android tester APKs](docs/android.md), [macOS and IntelliJ setup](docs/desktop.md) or [building the browser app](docs/web-app.md#website-and-pwa). To try federation on your Mac, start two local homes with `./yap start` and check them with `./yap doctor`. [Local home setup and invitations](docs/local-federation.md)

## How it works

The native apps share a Kotlin UI in `composeApp/`. The browser app loads the Rust client as WebAssembly. In `netkit/`, clients sign events and homes verify and replicate them over Iroh. The home executable and operator CLI live in `backend/`.

- [Networking and federation](docs/netkit/README.md)
- [Account ownership and recovery](docs/ownership.md)
- [What a peer can verify after deletion](docs/netkit/deletion.md)
- [Development guide](docs/development.md) and [test commands](docs/netkit/validation.md)

## License

The home server is licensed under AGPL-3.0-only. The shared Rust client and protocol code are available under MIT or Apache-2.0. [License details](LICENSING.md) · [Download the running server source on GitHub](https://github.com/YapLgbt/Yappers/releases/tag/home-v0.1.2)

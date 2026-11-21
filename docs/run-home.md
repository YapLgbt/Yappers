# Run your own Yap Home

A Home hosts a private feed you can share by invitation. You can also enroll your Home in the public Yap network for access to the shared main feed and federated public groups.

## Start your Home

Use a Linux x86-64 machine with Git, Docker Engine, a current Docker Compose v2 plugin, and outbound internet access for Iroh. The published image is available without a GitHub account. If you use Docker Desktop, enable host networking for the admin panel. [Docker host networking](https://docs.docker.com/engine/network/drivers/host/#platform-support)

```sh
git clone --depth 1 https://github.com/YapLgbt/Yappers.git
cd Yappers
export COMPOSE_FILE=compose.yaml:deploy/compose/registry.yaml
export YAP_HOME_IMAGE=ghcr.io/yaplgbt/yappers-home:0.1.1
docker compose --profile admin up -d --wait
```

On the first start, Docker downloads the image and waits for the Home to connect to the Iroh relays. Run later Compose commands from the same directory. In a new terminal, set `COMPOSE_FILE` and `YAP_HOME_IMAGE` again.

## Sign in and invite a member

Generate a one-time code on the Docker host.

```sh
docker compose --profile admin exec admin yappers-server admin code \
  --config /var/lib/yap/home/config.json
```

Open Home admin on that machine and enter the code. It expires after five minutes. For a remote host, forward port 8787 over SSH and open the same localhost address. [Home admin](http://127.0.0.1:8787) · [Remote access](netkit/operations.md#browser-admin)

Under **Invitations**, choose **Download Home invitation**. On Home 0.1.0, download the JSON for **Main Yap feed** if your Home has joined the federation, or for your local feed if it runs independently. In the native Yap app, open **You → Join community**, paste the JSON, and choose **Join instance**. In the web app, choose **Create an account** and expand **Have an invitation to another home?**. Paste the JSON there. Anyone with this invitation can join the feed it opens, so share it privately. [Yap web app](https://yap.lgbt/app/)

## Run and update your Home

- **Reports and moderation.** Members can report content on Compose Homes by default. Review the private inbox in **Reports**. Use **Moderation** to hide or restore a post or reply. To disable reports on an unstaffed Home, put `YAP_REPORT_INBOX=` in the project’s `.env` and rerun Compose up for both services. [Report handling](../deploy/peering/REPORTS.md)
- **Updates.** Take a protected backup, set `YAP_HOME_IMAGE` to a published version or digest, then run the commands below. Keep the same Compose project name so Docker reuses your Home and report inbox volumes. A standalone Home is briefly unavailable during container replacement. [Protected backup procedure](netkit/deletion.md#storage-and-backup) · [Published Home images](https://github.com/YapLgbt/Yappers/pkgs/container/yappers-home)
- **Status and logs.** Run `docker compose --profile admin ps` and `docker compose logs --tail 100 home` if startup stalls. Set `YAP_ADMIN_PORT` to use a free localhost port. Omit `--profile admin` on first startup to run the Home alone.
- **Stop and restart.** Use `docker compose --profile admin stop` to stop both services and the same `up` command to start them again. `docker compose down -v` deletes the Home and report inbox volumes.

```sh
docker compose --profile admin pull
docker compose --profile admin up -d --wait
```

To build from a source checkout, run `docker compose -f compose.yaml --profile admin up -d --build --wait`.

## Optional: join the public Yap network

Read the covenant before signing. You’ll need Python 3.9 or newer on the Docker host, an operator name, and a working public contact. Your Home signs with its existing key and shares the signed receipt with peers, including the operator name and contact you provide. [Covenant v2](https://yap.lgbt/peering/v2.txt) · [Network discovery](https://yap.lgbt/.well-known/yap-peering.json)

```sh
./scripts/join-yap-federation --operator 'Your operator name' \
  --contact 'Your public contact' --accept-version 2
```

Run this from the same Compose project directory and environment you used to start the Home. It verifies the covenant and the running image’s corresponding source, signs with your Home’s key, and restarts the Home and any running admin service. The command reports success after the main Yap Home confirms peering. If confirmation times out, check `docker compose logs --tail 100 home`, then retry the same command to reuse the signed receipt.

**Download Home invitation** now opens the main Yap feed through your Home. Invited members can read posts, publish, and reply across participating Homes without another invitation or covenant signature. They can also discover and join public groups. **Other community invitations** are still available for local feeds. On Home 0.1.0, use **Main Yap feed** in the invitation list. Without an admin service, copy the same shared invitation with `docker compose cp home:/var/lib/yap/home/join-shared.json ./yap-home-invitation.json`. Existing members who joined a local feed can import the shared invitation after delivering pending posts. [Peering and suspension guide](../deploy/peering/OPERATIONS.md)

After updating to a new image, run the command again to sign for its source revision. For a custom build, add `--source-url` with the public HTTPS URL of your corresponding source archive and `--revision` with that archive’s full lowercase SHA-256.

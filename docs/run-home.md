# Run your own Yap Home

A Home hosts a private feed you can share by invitation. You can also enroll your Home in the public Yap network for access to the shared main feed and federated public groups.

## Start your Home

Use a Linux x86-64 machine with Git, Docker Engine, a current Docker Compose v2 plugin, and outbound internet access for Iroh. The published image is available without a GitHub account. If you use Docker Desktop, enable host networking for the admin panel. [Docker host networking](https://docs.docker.com/engine/network/drivers/host/#platform-support)

```sh
git clone --depth 1 https://github.com/YapLgbt/Yappers.git
cd Yappers
export COMPOSE_FILE=compose.yaml:deploy/compose/registry.yaml
export YAP_HOME_IMAGE=ghcr.io/yaplgbt/yappers-home:0.1.0
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

Under **Invitations**, download the JSON. In the native Yap app, open **You → Join community**, paste the JSON, and choose **Join instance**. In the web app, choose **Create an account** and expand **Have an invitation to another home?**. Paste the JSON there. Anyone with this invitation can join your feed, so share it privately. [Yap web app](https://yap.lgbt/app/)

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

Use the following example with the published image `ghcr.io/yaplgbt/yappers-home:0.1.0`. You’ll need `curl`, `jq`, an operator name and a working public contact. The signed receipt is shared with peers. Read the covenant before signing. Use the corresponding source for your own build if you modify the software. [Covenant v2](https://yap.lgbt/peering/v2.txt) · [Network discovery](https://yap.lgbt/.well-known/yap-peering.json)

```sh
docker compose exec home yappers-server covenant-show

state=/var/lib/yap/home
revision=f9114c80cd1904eeaf88958c030175ba604a0eaddbc21e609d46b25ea386be29
source_url="https://github.com/YapLgbt/Yappers/releases/download/home-v0.1.0/$revision.tar.gz"

bootstrap=$(curl -fsS https://yap.lgbt/.well-known/yap-home.json | jq -ce '.join.serverAddr')
main_home=$(printf '%s\n' "$bootstrap" | jq -er '.id')
printf '%s\n' "$bootstrap" | docker compose exec -T home sh -ec \
  'umask 077; cat > /var/lib/yap/home/covenant-bootstrap.json'

docker compose exec home yappers-server covenant-accept \
  --config "$state/config.json" --operator 'Your operator name' \
  --contact 'Your public contact' --accept-version 2 \
  --source-url "$source_url" --revision "$revision" \
  --output "$state/acceptance-$revision.json"
docker compose exec home yappers-server covenant-verify \
  --receipt "$state/acceptance-$revision.json"
docker compose exec home yappers-server covenant-enable \
  --config "$state/config.json" --receipt "$state/acceptance-$revision.json" \
  --main-home "$main_home" --bootstrap "$state/covenant-bootstrap.json"

docker compose --profile admin restart home admin
docker compose --profile admin up -d --wait
```

Once the public Home has admitted yours, it appears under **Federation peers**. Under **Invitations**, choose **Main Yap feed** to invite members to that feed through your Home. If you’re running without admin, restart just `home` and copy its `/var/lib/yap/home/join-shared.json` invitation with `docker compose cp`. Sign a new receipt for a new source revision and apply it using the same main Home ID. [Peering and suspension guide](../deploy/peering/OPERATIONS.md)

// SPDX-License-Identifier: AGPL-3.0-only
//! Operator CLI and durable development client for Yappers home instances.
//!
//! Private configuration, join capabilities, and client identities use mode-0600 atomic
//! files. `demo` runs two real Iroh homes with independent logs; it is not a mock server.
//! See docs/netkit/operations.md for trust configuration, backup, and recovery rules.

mod account;
mod health;
mod private_file;

use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use private_file::{PrivateFileLock, private_write};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use yappers_netkit::{
    client::{Client, ClientConfig},
    groups::{Locality, NewGroup},
    model::*,
    server::{PeerConfig, Server, ServerConfig},
    store::Store,
};

#[derive(Parser)]
#[command(
    about = "Yap federated Iroh home instance and development client",
    version
)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum ReportOperation {
    List,
    Show {
        case: String,
    },
    /// Close a reviewed case and erase its submitted evidence.
    Close {
        case: String,
        #[arg(long,value_parser=["actionTaken","noAction"])]
        outcome: String,
    },
}
#[derive(Subcommand)]
enum Commands {
    /// Review the private report inbox on this machine. Requires read/write access to its file.
    Reports {
        #[arg(long)]
        inbox: PathBuf,
        #[command(subcommand)]
        operation: ReportOperation,
    },
    /// Print the covenant text whose digest is included in signed acceptances.
    CovenantShow,
    /// Sign a public operator acceptance. Local admission is a separate operator decision.
    CovenantAccept {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        operator: String,
        #[arg(long)]
        contact: String,
        #[arg(long)]
        source_url: String,
        #[arg(long)]
        revision: String,
        #[arg(long)]
        accept_version: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Verify the home signature, covenant version and exact public source declaration.
    CovenantVerify {
        #[arg(long)]
        receipt: PathBuf,
    },
    /// Sign a private-service delegation with the logical home signing key.
    HaAuthorize {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        standby: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Apply local moderation atomically to the live private HA ledger.
    HaModerate {
        #[arg(long)]
        ledger: PathBuf,
        #[arg(long)]
        grant: PathBuf,
        #[arg(long)]
        event: String,
        #[arg(long)]
        unhide: bool,
    },
    /// Export the latest committed home database to a new private file.
    HaExport {
        #[arg(long)]
        ledger: PathBuf,
        #[arg(long)]
        grant: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Create a protected backup of a standalone home using its current deletion guard.
    Backup {
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Restore a protected backup to a fresh path using the current independent guard.
    Restore {
        #[arg(long)]
        guard: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Submit an owner-signed deletion for a post or reply.
    Delete {
        #[arg(long)]
        join: PathBuf,
        #[arg(long)]
        identity: PathBuf,
        #[arg(long)]
        event: String,
    },
    /// Seed an empty authority from a quiesced dedicated home, then exit.
    HaBootstrap {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        grant: PathBuf,
    },
    /// Serve a fenced HA home. The standby-config and ledger flags select the commit authority.
    HaRun {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        grant: PathBuf,
        #[arg(long, requires = "ledger")]
        standby_config: Option<PathBuf>,
        #[arg(long, requires = "standby_config")]
        ledger: Option<PathBuf>,
    },
    /// User-owned portable accounts, encrypted exports, and signed home migration.
    Account {
        #[command(subcommand)]
        command: account::AccountCommand,
    },
    /// Create a private instance config. Does not start a network service.
    Init {
        #[arg(long)]
        directory: PathBuf,
        #[arg(long, default_value = "local")]
        network: String,
        /// Enable public group discovery/enrollment; private communities stay explicitly peered.
        #[arg(long)]
        auto_federate: bool,
        /// File containing a public EndpointAddr JSON, not a private join invitation.
        #[arg(long)]
        bootstrap: Vec<PathBuf>,
    },
    /// Serve the dedicated open-membership home and publish its HTTPS enrollment manifest.
    PublicHome {
        #[arg(long)]
        directory: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
    },
    /// Start one configured instance and its federation pull loop.
    Run {
        #[arg(long)]
        config: PathBuf,
    },
    /// Start two mutually trusted local instances, persisting keys and logs.
    Demo {
        #[arg(long, default_value = "netkit-data/demo")]
        directory: PathBuf,
    },
    /// Fetch a feed using a private join file.
    Feed {
        #[arg(long)]
        join: PathBuf,
        #[arg(long)]
        identity: PathBuf,
    },
    /// Browse the public local/worldwide group catalog through a home.
    Groups {
        #[arg(long)]
        join: PathBuf,
        #[arg(long)]
        identity: PathBuf,
    },
    /// Create a public group owned by this home; omit city/country for worldwide scope.
    CreateGroup {
        #[arg(long)]
        join: PathBuf,
        #[arg(long)]
        identity: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: String,
        #[arg(long, default_value = "Friends")]
        category: String,
        #[arg(long, requires = "country")]
        city: Option<String>,
        #[arg(long, requires = "city")]
        country: Option<String>,
        #[arg(long, default_value = "")]
        region: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Join and automatically host a discovered group; writes a private group invitation.
    JoinGroup {
        #[arg(long)]
        join: PathBuf,
        #[arg(long)]
        identity: PathBuf,
        #[arg(long)]
        group: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Sign, durably enqueue, submit, and acknowledge a post.
    Post {
        #[arg(long)]
        headline: Option<String>,
        #[arg(long)]
        join: PathBuf,
        #[arg(long)]
        identity: PathBuf,
        #[arg(long)]
        body: String,
        #[arg(long, default_value = "Chatter")]
        topic: String,
        #[arg(long, default_value = "Neighbor")]
        name: String,
        #[arg(long)]
        anonymous: bool,
    },
    /// Retry all previously enqueued events without generating new IDs.
    Retry {
        #[arg(long)]
        join: PathBuf,
        #[arg(long)]
        identity: PathBuf,
    },
    /// Instance-local moderation. Does not erase replicated data.
    Moderate {
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        event: String,
        #[arg(long)]
        unhide: bool,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Identity {
    secret: String,
    next_sequence: u64,
    #[serde(default)]
    pending: Vec<SignedEvent>,
}
fn private_json(path: &Path, value: &impl Serialize) -> Result<()> {
    private_write(path, &serde_json::to_vec_pretty(value)?)
}
fn lock_identity(path: &Path) -> Result<PrivateFileLock> {
    PrivateFileLock::acquire(&path.with_added_extension("identity-lock"))
        .context("client identity is already in use")
}
fn identity(path: &Path) -> Result<Identity> {
    if path.exists() {
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    } else {
        let identity = Identity {
            secret: new_secret(),
            next_sequence: 1,
            pending: vec![],
        };
        private_json(path, &identity)?;
        Ok(identity)
    }
}
fn config(directory: &Path, mode: NetworkMode) -> Result<ServerConfig> {
    let directory = fs::canonicalize(directory)?;
    let secret = new_secret();
    let id = hex::encode(signing_key(&secret)?.verifying_key().as_bytes());
    Ok(ServerConfig {
        public_replica_feeds: Default::default(),
        private_key: secret,
        database_path: directory.join("events.sqlite"),
        communities: BTreeMap::from([(format!("{id}/neighborhood"), new_secret())]),
        peers: vec![],
        network_mode: mode,
        auto_federation: Default::default(),
    })
}
async fn flush(client: &Client, identity: &mut Identity, path: &Path) -> Result<()> {
    while let Some(event) = identity.pending.first().cloned() {
        client.submit(event).await?;
        identity.pending.remove(0);
        private_json(path, identity)?;
    }
    Ok(())
}
async fn client_operation(
    join: PathBuf,
    path: PathBuf,
    action: Option<Action>,
    retry: bool,
) -> Result<()> {
    let join: JoinConfig = serde_json::from_slice(&fs::read(join)?)?;
    let _lock = lock_identity(&path)?;
    let mut identity = identity(&path)?;
    let client = Client::open(ClientConfig {
        full_home_standby: None,
        account_chain: vec![],
        join,
        author_secret: identity.secret.clone(),
        next_sequence: identity.next_sequence,
    })
    .await?;
    if let Some(action) = action {
        let prepared = client.prepare(action)?;
        identity.next_sequence = prepared.next_sequence;
        identity.pending.push(prepared.event);
        private_json(&path, &identity)?;
        flush(&client, &mut identity, &path).await?;
        println!("post accepted");
    } else if retry {
        flush(&client, &mut identity, &path).await?;
        println!("outbox delivered");
    } else {
        println!("{}", serde_json::to_string_pretty(&client.feed().await?)?);
    }
    client.close().await;
    Ok(())
}
async fn group_operation(
    join: PathBuf,
    path: PathBuf,
    details: Option<NewGroup>,
    group: Option<String>,
    output: Option<PathBuf>,
) -> Result<()> {
    let identity = {
        let _lock = lock_identity(&path)?;
        identity(&path)?
    };
    let client = Client::open(ClientConfig {
        full_home_standby: None,
        account_chain: vec![],
        join: serde_json::from_slice(&fs::read(join)?)?,
        author_secret: identity.secret,
        next_sequence: identity.next_sequence,
    })
    .await?;
    let response = if let Some(details) = details {
        client.create_group(details).await
    } else if let Some(group) = group {
        client.join_group(group).await
    } else {
        client.groups().await
    };
    client.close().await;
    let response = response?;
    if let (Some(path), Response::GroupJoined { join, .. }) = (output, &response) {
        private_json(&path, join)?;
    }
    // Never print a returned private join capability as part of a public summary.
    match response {
        Response::GroupJoined { group, .. } => {
            println!("{}", serde_json::to_string_pretty(&group)?)
        }
        response => println!("{}", serde_json::to_string_pretty(&response)?),
    }
    Ok(())
}

async fn serve_background(server: &Server, period: Duration) {
    let ticker = || {
        let mut interval = tokio::time::interval(period);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval
    };
    let push = async {
        let mut interval = ticker();
        loop {
            interval.tick().await;
            if let Err(error) = server.dispatch_notifications().await {
                eprintln!("push dispatch: {error:#}");
            }
        }
    };
    let federation = async {
        let mut interval = ticker();
        loop {
            interval.tick().await;
            if let Err(error) = server.sync_once().await {
                eprintln!("federation sync: {error:#}");
            }
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        () = push => {},
        () = federation => {},
    }
}

async fn serve_one(path: PathBuf, public_manifest: Option<PathBuf>) -> Result<()> {
    let config: ServerConfig = serde_json::from_slice(&fs::read(&path)?)?;
    let mode = config.network_mode;
    let communities: Vec<_> = config.communities.keys().cloned().collect();
    let server = Server::start(config).await?;
    health::listen(&server).await?;
    private_json(
        &path.with_file_name("address.json"),
        &server.public_address(),
    )?;
    for (index, community) in communities.iter().enumerate() {
        private_json(
            &path.with_file_name(format!("join-{index}.json")),
            &server.join(community, mode)?,
        )?;
    }
    println!(
        "instance {} listening; private join files written beside config",
        server.endpoint.id()
    );
    if let Some(manifest) = public_manifest {
        let scope = format!("{}/public", server.endpoint.id());
        ensure!(
            communities == [scope.clone()],
            "expected a dedicated public home"
        );
        let join = server.join(&scope, mode)?;
        private_json(&manifest, &serde_json::json!({"version": 1, "join": join}))?;
    }
    serve_background(&server, Duration::from_secs(2)).await;
    server.close().await;
    Ok(())
}
async fn demo(directory: PathBuf) -> Result<()> {
    fs::create_dir_all(directory.join("a"))?;
    fs::create_dir_all(directory.join("b"))?;
    let a_path = directory.join("a/config.json");
    let b_path = directory.join("b/config.json");
    let mut a = if a_path.exists() {
        serde_json::from_slice(&fs::read(&a_path)?)?
    } else {
        config(&directory.join("a"), NetworkMode::Local)?
    };
    let community = a
        .communities
        .keys()
        .next()
        .context("demo community missing")?
        .clone();
    let mut b: ServerConfig = if b_path.exists() {
        serde_json::from_slice(&fs::read(&b_path)?)?
    } else {
        config(&directory.join("b"), NetworkMode::Local)?
    };
    if !b.communities.contains_key(&community) {
        b.communities = BTreeMap::from([(community.clone(), new_secret())]);
    }
    a.peers.clear();
    b.peers.clear();
    a.auto_federation.enabled = true;
    b.auto_federation.enabled = true;
    // Local ports change between launches; refresh discovery seeds after binding.
    a.auto_federation.seeds.clear();
    b.auto_federation.seeds.clear();
    let server_a = Server::start(a.clone()).await?;
    let server_b = Server::start(b.clone()).await?;
    let scope = BTreeSet::from([community.clone()]);
    a.peers = vec![PeerConfig {
        address: server_b.endpoint.addr(),
        communities: scope.clone(),
    }];
    b.peers = vec![PeerConfig {
        address: server_a.endpoint.addr(),
        communities: scope,
    }];
    server_a.set_peers(a.peers.clone())?;
    server_b.set_peers(b.peers.clone())?;
    b.auto_federation.seeds = vec![server_a.public_address()];
    server_b.set_discovery_seeds(b.auto_federation.seeds.clone())?;
    let local = server_a.create_group(NewGroup {
        name: "Neighborhood book club".into(),
        description: "Bring a book, find your people. A public group for New York readers.".into(),
        category: "Friends".into(),
        locality: Some(Locality {
            city: "New York".into(),
            region: "NY".into(),
            country: "US".into(),
        }),
    })?;
    server_a.create_group(NewGroup {
        name: "London after hours".into(),
        description: "Low-key plans and new connections around London.".into(),
        category: "Plans".into(),
        locality: Some(Locality {
            city: "London".into(),
            region: "".into(),
            country: "GB".into(),
        }),
    })?;
    server_a.create_group(NewGroup {
        name: "Queer everywhere".into(),
        description: "A worldwide corner for stories, friendship, and everyday queer life.".into(),
        category: "Chatter".into(),
        locality: None,
    })?;
    server_b.sync_once().await?;
    let group_b = server_b.join_group(&local.id).await?;
    private_json(
        &directory.join("join-group-a.json"),
        &server_a.join_group(&local.id).await?,
    )?;
    private_json(&directory.join("join-group-b.json"), &group_b)?;
    private_json(
        &directory.join("address-a.json"),
        &server_a.public_address(),
    )?;
    private_json(
        &directory.join("address-b.json"),
        &server_b.public_address(),
    )?;
    private_json(&a_path, &a)?;
    private_json(&b_path, &b)?;
    private_json(
        &directory.join("join-a.json"),
        &server_a.join(&community, NetworkMode::Local)?,
    )?;
    private_json(
        &directory.join("join-b.json"),
        &server_b.join(&community, NetworkMode::Local)?,
    )?;
    println!(
        "Two local instances running. Private join files: {}/join-a.json and join-b.json",
        directory.display()
    );
    println!("Community: {community}");
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = interval.tick() => {
                if let Err(error) = server_a.sync_once().await { eprintln!("A sync: {error:#}"); }
                if let Err(error) = server_b.sync_once().await { eprintln!("B sync: {error:#}"); }
            }
        }
    }
    server_a.close().await;
    server_b.close().await;
    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    match Args::parse().command {
        Commands::Reports { inbox, operation } => {
            let mut inbox = yappers_netkit::report_inbox::Inbox::open(&inbox, None)?;
            match operation {
                ReportOperation::List => {
                    println!("{}", serde_json::to_string_pretty(&inbox.pending()?)?)
                }
                ReportOperation::Show { case } => {
                    println!("{}", serde_json::to_string_pretty(&inbox.inspect(&case)?)?)
                }
                ReportOperation::Close { case, outcome } => {
                    inbox.close(&case, &outcome)?;
                    println!("Case closed and submitted evidence removed.");
                }
            }
        }
        Commands::CovenantShow => print!("{}", yappers_netkit::peering::COVENANT),
        Commands::CovenantAccept {
            config,
            operator,
            contact,
            source_url,
            revision,
            accept_version,
            output,
        } => {
            ensure!(
                accept_version == yappers_netkit::peering::VERSION,
                "read covenant-show and specify its version"
            );
            let cfg: ServerConfig = serde_json::from_slice(&fs::read(config)?)?;
            let receipt = yappers_netkit::peering::Acceptance::sign(
                &signing_key(&cfg.private_key)?,
                operator,
                contact,
                source_url,
                revision,
            )?;
            // Do not overwrite an existing file, including the private signing configuration.
            let mut out = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output)?;
            serde_json::to_writer_pretty(&mut out, &receipt)?;
            out.sync_all()?;
        }
        Commands::CovenantVerify { receipt } => {
            let acceptance: yappers_netkit::peering::Acceptance =
                serde_json::from_slice(&fs::read(receipt)?)?;
            acceptance.validate()?;
            println!(
                "Signature and covenant verified. Operator identity, source and behavior require review."
            );
        }
        Commands::HaAuthorize {
            config,
            standby,
            output,
        } => {
            let config: ServerConfig = serde_json::from_slice(&fs::read(config)?)?;
            let grant = yappers_netkit::takeover::FullHomeStandby::issue(
                &signing_key(&config.private_key)?,
                serde_json::from_slice(&fs::read(standby)?)?,
            )?;
            private_json(&output, &grant)?;
        }
        Commands::HaModerate {
            ledger,
            grant,
            event,
            unhide,
        } => {
            yappers_netkit::server::moderate_ha(
                &ledger,
                &serde_json::from_slice(&fs::read(grant)?)?,
                &event,
                !unhide,
            )?;
        }
        Commands::Backup { database, output } => Store::backup_existing(&database, &output)?,
        Commands::Restore {
            guard,
            input,
            output,
        } => yappers_netkit::backup::restore(&guard, &input, &output)?,
        Commands::Delete {
            join,
            identity,
            event,
        } => {
            client_operation(
                join,
                identity,
                Some(Action::Delete { target: event }),
                false,
            )
            .await?
        }
        Commands::HaExport {
            ledger,
            grant,
            output,
        } => {
            yappers_netkit::server::export_ha(
                &ledger,
                &serde_json::from_slice(&fs::read(grant)?)?,
                &output,
            )?;
        }
        Commands::HaBootstrap { config, grant } => {
            Server::bootstrap_ha(
                serde_json::from_slice(&fs::read(config)?)?,
                serde_json::from_slice(&fs::read(grant)?)?,
            )
            .await?;
            println!("Initial home state committed to the authorized standby");
        }
        Commands::HaRun {
            config,
            grant,
            standby_config,
            ledger,
        } => {
            use yappers_netkit::server::{HaOptions, HaRole};
            let grant = serde_json::from_slice(&fs::read(grant)?)?;
            let role = if let Some(node) = standby_config {
                let node: ServerConfig = serde_json::from_slice(&fs::read(node)?)?;
                HaRole::Standby {
                    transport_key: node.private_key,
                    ledger: ledger.context("standby requires ledger")?,
                }
            } else {
                HaRole::Primary
            };
            let settings = serde_json::from_slice(&fs::read(&config)?)?;
            let server = Server::start_ha(settings, HaOptions { grant, role }).await?;
            health::listen(&server).await?;
            private_json(
                &config.with_file_name("ha-address.json"),
                &server.public_address(),
            )?;
            serve_background(&server, Duration::from_secs(15)).await;
            server.close().await;
        }
        Commands::Account { command } => account::execute(command).await?,
        Commands::Init {
            directory,
            network,
            auto_federate,
            bootstrap,
        } => {
            ensure!(
                network == "local" || network == "n0",
                "network must be local or n0"
            );
            let path = directory.join("config.json");
            ensure!(!path.exists(), "config already exists");
            fs::create_dir_all(&directory)?;
            let mut config = config(
                &directory,
                if network == "n0" {
                    NetworkMode::N0
                } else {
                    NetworkMode::Local
                },
            )?;
            ensure!(
                bootstrap.is_empty() || auto_federate,
                "--bootstrap requires --auto-federate"
            );
            config.auto_federation.enabled = auto_federate;
            config.auto_federation.seeds = bootstrap
                .iter()
                .map(|path| Ok(serde_json::from_slice(&fs::read(path)?)?))
                .collect::<Result<_>>()?;
            private_json(&path, &config)?;
            println!("private configuration created: {}", path.display());
        }
        Commands::PublicHome {
            directory,
            manifest,
        } => {
            fs::create_dir_all(&directory)?;
            let path = directory.join("public-home.json");
            if !path.exists() {
                let mut home = config(&directory, NetworkMode::N0)?;
                let id = hex::encode(signing_key(&home.private_key)?.verifying_key().as_bytes());
                home.communities = BTreeMap::from([(format!("{id}/public"), new_secret())]);
                home.auto_federation.enabled = true;
                private_json(&path, &home)?;
            }
            serve_one(path, Some(manifest)).await?;
        }
        Commands::Run { config } => serve_one(config, None).await?,
        Commands::Demo { directory } => demo(directory).await?,
        Commands::Groups { join, identity } => {
            group_operation(join, identity, None, None, None).await?
        }
        Commands::JoinGroup {
            join,
            identity,
            group,
            output,
        } => group_operation(join, identity, None, Some(group), Some(output)).await?,
        Commands::CreateGroup {
            join,
            identity,
            name,
            description,
            category,
            city,
            country,
            region,
            output,
        } => {
            let locality = city.map(|city| Locality {
                city,
                region,
                country: country.unwrap_or_default().to_uppercase(),
            });
            group_operation(
                join,
                identity,
                Some(NewGroup {
                    name,
                    description,
                    category,
                    locality,
                }),
                None,
                Some(output),
            )
            .await?;
        }
        Commands::Feed { join, identity } => client_operation(join, identity, None, false).await?,
        Commands::Retry { join, identity } => client_operation(join, identity, None, true).await?,
        Commands::Post {
            headline,
            join,
            identity,
            body,
            topic,
            name,
            anonymous,
        } => {
            client_operation(
                join,
                identity,
                Some(if let Some(headline) = headline {
                    Action::PostV2 {
                        area: None,
                        body,
                        topic,
                        anonymous,
                        headline,
                        profile: None,
                        display_name: if anonymous {
                            "Anonymous neighbor".into()
                        } else {
                            name
                        },
                    }
                } else {
                    Action::Post {
                        body,
                        topic,
                        anonymous,
                        display_name: if anonymous {
                            "Anonymous neighbor".into()
                        } else {
                            name
                        },
                    }
                }),
                false,
            )
            .await?
        }
        Commands::Moderate {
            database,
            event,
            unhide,
        } => {
            Store::moderate_existing(&database, &event, !unhide)?;
            println!("local moderation updated");
        }
    }
    Ok(())
}

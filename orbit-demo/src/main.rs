



use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{bail, Context as _, Result};
use aranya_client::{AfcMsg, Client, Label};
use aranya_daemon::{
    config::{AfcConfig, Config},
    Daemon,
};

use aranya_daemon_api::{DeviceId, KeyBundle, NetIdentifier, Role};
use aranya_util::Addr;
use backon::{ExponentialBuilder, Retryable};
use tempfile::tempdir;
use tokio::{fs, task, time::sleep};
use tracing::{debug, info, Metadata};
use tracing_subscriber::{
    layer::{Context, Filter},
    prelude::*,
    EnvFilter,
};

struct TeamCtx {
    owner: UserCtx,
    admin: UserCtx,
    operator: UserCtx,
    membera: UserCtx,
    memberb: UserCtx,
}

/// TeamCtx is a struct that contains the context for a team.
impl TeamCtx {
    pub async fn new(name: String, work_dir: PathBuf) -> Result<Self> {
        let owner = UserCtx::new(name.clone(), "owner".into(), work_dir.join("owner")).await?;
        let admin = UserCtx::new(name.clone(), "admin".into(), work_dir.join("admin")).await?;
        let operator =
            UserCtx::new(name.clone(), "operator".into(), work_dir.join("operator")).await?;
        let membera =
            UserCtx::new(name.clone(), "membera".into(), work_dir.join("membera")).await?;
        let memberb =
            UserCtx::new(name.clone(), "memberb".into(), work_dir.join("memberb")).await?;

        Ok(Self {
            owner,
            admin,
            operator,
            membera,
            memberb,
        })
    }
}

struct UserCtx {
    client: Client,
    pk: KeyBundle,
    id: DeviceId,
}

impl UserCtx {
    pub async fn new(team_name: String, name: String, work_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(work_dir.clone()).await?;

        // Setup daemon config.
        let uds_api_path = work_dir.join("uds.sock");
        let any = Addr::new("localhost", 0).expect("should be able to create new Addr");
        let shm_path = format!("/shm_{}_{}", team_name, name).to_string();
        let max_chans = 100;
        let cfg = Config {
            name: "daemon".into(),
            work_dir: work_dir.clone(),
            uds_api_path: uds_api_path.clone(),
            pid_file: work_dir.join("pid"),
            sync_addr: any,
            afc: AfcConfig {
                shm_path: shm_path.clone(),
                unlink_on_startup: true,
                unlink_at_exit: true,
                create: true,
                max_chans,
            },
        };
        // Load daemon from config.
        // TODO: start daemons from binary rather than objects.
        let daemon = Daemon::load(cfg.clone())
            .await
            .context("unable to init daemon")?;
        // Start daemon.
        task::spawn(async move {
            daemon
                .run()
                .await
                .expect("expected no errors running daemon")
        });
        // give daemon time to setup UDS API.
        sleep(Duration::from_millis(100)).await;

        // Initialize the user library.
        let mut client = (|| {
            Client::connect(
                &cfg.uds_api_path,
                Path::new(&cfg.afc.shm_path),
                cfg.afc.max_chans,
                cfg.sync_addr.to_socket_addrs(),
            )
        })
        .retry(ExponentialBuilder::default())
        .await
        .context("unable to initialize client")?;

        // Get device id and key bundle.
        let pk = client.get_key_bundle().await.expect("expected key bundle");
        let id = client.get_device_id().await.expect("expected device id");

        Ok(Self { client, pk, id })
    }

    async fn aranya_local_addr(&self) -> Result<SocketAddr> {
        Ok(self.client.aranya_local_addr().await?)
    }

    async fn afc_local_addr(&self) -> Result<SocketAddr> {
        Ok(self.client.afc_local_addr().await?)
    }
}

/// Repeatedly calls `poll_afc_data`, followed by `handle_afc_data`, until all
/// of the clients are pending.
macro_rules! do_poll {
    ($($client:expr),*) => {
        debug!(
            clients = stringify!($($client),*),
            "start `do_poll`",
        );
        loop {
            tokio::select! {
                biased;
                $(data = $client.poll_afc_data() => {
                    $client.handle_afc_data(data?).await?
                },)*
                _ = async {} => break,
            }
        }
        debug!(
            clients = stringify!($($client),*),
            "finish `do_poll`",
        );
    };
}

struct DemoFilter {
    env_filter: EnvFilter,
}

impl<S> Filter<S> for DemoFilter {
    fn enabled(&self, metadata: &Metadata<'_>, context: &Context<'_, S>) -> bool {
        if metadata.target().starts_with(module_path!()) {
            true
        } else {
            self.env_filter.enabled(metadata, context.clone())
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let filter = DemoFilter {
        env_filter: EnvFilter::try_from_env("ARANYA_EXAMPLE")
            .unwrap_or_else(|_| EnvFilter::new("off")),
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_file(false)
                .with_target(false)
                .compact()
                .with_filter(filter),
        )
        .init();

    info!("starting example Aranya application");

    let sync_interval = Duration::from_millis(100);
    let sleep_interval = sync_interval * 6;

    let tmp = tempdir()?;
    let work_dir = tmp.path().to_path_buf();

    let mut team = TeamCtx::new("test_afc_router".into(), work_dir).await?;

    // create team.
    info!("creating team");
    let team_id = team
        .owner
        .client
        .create_team()
        .await
        .expect("expected to create team");
    info!(?team_id);

    // get sync addresses.
    let owner_addr = team.owner.aranya_local_addr().await?;
    let admin_addr = team.admin.aranya_local_addr().await?;
    let operator_addr = team.operator.aranya_local_addr().await?;
    let membera_addr = team.membera.aranya_local_addr().await?;
    let memberb_addr = team.memberb.aranya_local_addr().await?;

    // get afc addresses.
    let membera_afc_addr = team.membera.afc_local_addr().await?;
    let memberb_afc_addr = team.memberb.afc_local_addr().await?;

    // setup sync peers.
    let mut owner_team = team.owner.client.team(team_id);
    let mut admin_team = team.admin.client.team(team_id);
    let mut operator_team = team.operator.client.team(team_id);
    let mut membera_team = team.membera.client.team(team_id);
    let mut memberb_team = team.memberb.client.team(team_id);

    info!("adding sync peers");
    owner_team
        .add_sync_peer(admin_addr.into(), sync_interval)
        .await?;
    owner_team
        .add_sync_peer(operator_addr.into(), sync_interval)
        .await?;
    owner_team
        .add_sync_peer(membera_addr.into(), sync_interval)
        .await?;

    admin_team
        .add_sync_peer(owner_addr.into(), sync_interval)
        .await?;
    admin_team
        .add_sync_peer(operator_addr.into(), sync_interval)
        .await?;
    admin_team
        .add_sync_peer(membera_addr.into(), sync_interval)
        .await?;

    operator_team
        .add_sync_peer(owner_addr.into(), sync_interval)
        .await?;
    operator_team
        .add_sync_peer(admin_addr.into(), sync_interval)
        .await?;
    operator_team
        .add_sync_peer(membera_addr.into(), sync_interval)
        .await?;

    membera_team
        .add_sync_peer(owner_addr.into(), sync_interval)
        .await?;
    membera_team
        .add_sync_peer(admin_addr.into(), sync_interval)
        .await?;
    membera_team
        .add_sync_peer(operator_addr.into(), sync_interval)
        .await?;
    membera_team
        .add_sync_peer(memberb_addr.into(), sync_interval)
        .await?;

    memberb_team
        .add_sync_peer(owner_addr.into(), sync_interval)
        .await?;
    memberb_team
        .add_sync_peer(admin_addr.into(), sync_interval)
        .await?;
    memberb_team
        .add_sync_peer(operator_addr.into(), sync_interval)
        .await?;
    memberb_team
        .add_sync_peer(membera_addr.into(), sync_interval)
        .await?;

    // add admin to team.
    info!("adding admin to team");
    owner_team.add_device_to_team(team.admin.pk).await?;
    owner_team.assign_role(team.admin.id, Role::Admin).await?;

    // wait for syncing.
    sleep(sleep_interval).await;

    // add operator to team.
    info!("adding operator to team");
    owner_team.add_device_to_team(team.operator.pk).await?;

    // wait for syncing.
    sleep(sleep_interval).await;

    admin_team
        .assign_role(team.operator.id, Role::Operator)
        .await?;

    // wait for syncing.
    sleep(sleep_interval).await;

    // add membera to team.
    info!("adding membera to team");
    operator_team.add_device_to_team(team.membera.pk).await?;

    // add memberb to team.
    info!("adding memberb to team");
    operator_team.add_device_to_team(team.memberb.pk).await?;

    // wait for syncing.
    sleep(sleep_interval).await;

    // operator assigns labels for AFC channels.
    let label1 = Label::new(1);
    operator_team.create_label(label1).await?;
    operator_team.assign_label(team.membera.id, label1).await?;
    operator_team.assign_label(team.memberb.id, label1).await?;

    let label2 = Label::new(2);
    operator_team.create_label(label2).await?;
    operator_team.assign_label(team.membera.id, label2).await?;
    operator_team.assign_label(team.memberb.id, label2).await?;

    // assign network addresses.
    operator_team
        .assign_afc_net_identifier(team.membera.id, NetIdentifier(membera_afc_addr.to_string()))
        .await?;
    operator_team
        .assign_afc_net_identifier(team.memberb.id, NetIdentifier(memberb_afc_addr.to_string()))
        .await?;

    // wait for syncing.
    sleep(sleep_interval).await;

    // membera creates bidi channel with memberb
    let afc_id1 = team
        .membera
        .client
        .create_afc_bidi_channel(team_id, NetIdentifier(memberb_afc_addr.to_string()), label1)
        .await?;

    // membera creates bidi channel with memberb
    let afc_id2 = team
        .membera
        .client
        .create_afc_bidi_channel(team_id, NetIdentifier(memberb_afc_addr.to_string()), label2)
        .await?;

    // wait for ctrl message to be sent.
    sleep(Duration::from_millis(100)).await;

    do_poll!(team.membera.client, team.memberb.client);

    let msg = "hello world label1";
    team.membera
        .client
        .send_afc_data(afc_id1, msg.as_bytes())
        .await?;
    debug!(?msg, "sent message");

    let msg = "hello world label2";
    team.membera
        .client
        .send_afc_data(afc_id2, msg.as_bytes())
        .await?;
    debug!(?msg, "sent message");

    sleep(Duration::from_millis(100)).await;
    do_poll!(team.membera.client, team.memberb.client);

    let Some(AfcMsg { data, label, .. }) = team.memberb.client.try_recv_afc_data() else {
        bail!("no message available!")
    };
    debug!(
        n = data.len(),
        ?label,
        "received message: {:?}",
        core::str::from_utf8(&data)?
    );

    let Some(AfcMsg { data, label, .. }) = team.memberb.client.try_recv_afc_data() else {
        bail!("no message available!")
    };
    debug!(
        n = data.len(),
        ?label,
        "received message: {:?}",
        core::str::from_utf8(&data)?
    );

    info!("completed example Aranya application");

    Ok(())
}

// std lib imports
use std::{
    collections::BTreeMap,
    future,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

// external crate imports   
use anyhow::{bail, Context as _, Result};
use aranya_client::{AfcMsg, Client, Label};
use aranya_daemon::{
    config::{AfcConfig, Config},
    Daemon,
};

// aranya-platform crate imports
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


const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Environment variables for application executable.
#[derive(Debug)]
pub struct EnvVars {
    /// Space Aranya address.
    pub space_aranya_addr: Addr,
    /// Space APS address.
    pub space_aps_addr: Addr,
    /// MOC Aranya address.
    pub moc_aranya_addr: Addr,
    /// MOC APS address.
    pub moc_aps_addr: Addr,
}

impl EnvVars {
    /// Create a new environment variable instance.
    pub fn new() -> Result<Self> {
        debug!("env vars: {:?}", std::env::vars());
        Ok(EnvVars {
            space_aranya_addr: env_var("SPACE_ARANYA_ADDR")?,
            space_aps_addr: env_var("SPACE_APS_ADDR")?,
            moc_aranya_addr: env_var("MOC_ARANYA_ADDR")?,
            moc_aps_addr: env_var("MOC_APS_ADDR")?,
        })
    }
}


/// SpaceTeamCtx is a struct that contains the context for a space team.
struct SpaceTeamCtx {
    space: UserCtx,
    moc: UserCtx,
}

/// impl SpaceTeamCtx is a struct that contains the context for a space team.
impl SpaceTeamCtx {
    pub async fn new(name: String, work_dir: PathBuf) -> Result<Self> {
        let space = UserCtx::new(team_name.clone(), "space".into(), work_dir.join("space")).await?;
        let moc = UserCtx::new(team_name.clone(), "moc".into(), work_dir.join("moc")).await?;
        Ok(Self { 
            space,
            moc 
        })
    }
}

/// UserCtx is a struct that contains the context for a user.
struct UserCtx {
    client: Client,
    pk: KeyBundle,
    id: DeviceId,
}

/// impl UserCtx is a struct that contains the context for a user.
impl UserCtx {
    pub async fn new(team_name: String, name: String, work_dir: PathBuf) -> Result<Self> {
        // Create working directory.
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

/// DemoFilter is a filter that logs messages with the `orbit-demo` module.
struct DemoFilter {
    env_filter: EnvFilter,
}

/// impl DemoFilter is a filter that logs messages with the `orbit-demo` module.
impl<S> Filter<S> for DemoFilter {
    fn enabled(&self, metadata: &Metadata<'_>, context: &Context<'_, S>) -> bool {
        if metadata.target().starts_with(module_path!()) {
          true
        } else {
          self.env_filter.enabled(metadata, context)
        }
    }
}



#[derive(Debug, Parser)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Working directory.
    work_dir: PathBuf,
}


/// Main function for the new space demo.
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
}

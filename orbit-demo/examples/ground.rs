// Copyright (c) SpiderOak, Inc. All rights reserved.

//! Application script running on Ground machine in HSA demo.

use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use application::{
    tarpc::RPCClient,
    testaps,
    utils::{
        client::{retry, UdsClient},
        env::env_var,
        exec::{DaemonCtx, ExecutionCtx, User},
        json::read_json,
    },
};
use aranya_crypto::UserId;
use aranya_fast_channels::Label;
use chrono::Utc;
use clap::Parser;
use daemon::{addr::Addr, config::Peer, policies::base::vm_policy::Role, Proxy};
use tarpc::context;
use tokio::{task, time::sleep};
use tracing::{debug, error, info, info_span, trace, Instrument};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Environment variables for application executable.
#[derive(Debug, Clone)]
pub struct EnvVars {
    /// Remote image path.
    pub remote_image_path: String,
    /// MOC Aranya address.
    pub moc_aranya_addr: Addr,
    /// MOC APS address.
    pub moc_aps_addr: Addr,
    /// MOC Tarpc address.
    pub moc_tarpc_addr: Addr,
    /// Ground Aranya address.
    pub ground_aranya_addr: Addr,
    /// Ground APS address.
    pub ground_aps_addr: Addr,
    /// Operator Aranya address.
    pub operator_aranya_addr: Addr,
    /// Operator APS address
    pub operator_aps_addr: Addr,
}

impl EnvVars {
    /// Create a new environment variable instance.
    pub fn new() -> Result<Self> {
        debug!("env vars: {:?}", std::env::vars());
        Ok(EnvVars {
            remote_image_path: env_var("IMAGE")?,
            moc_aranya_addr: env_var("MOC_ARANYA_ADDR")?,
            moc_aps_addr: env_var("MOC_APS_ADDR")?,
            moc_tarpc_addr: env_var("MOC_TARPC_ADDR")?,
            ground_aranya_addr: env_var("GROUND_ARANYA_ADDR")?,
            ground_aps_addr: env_var("GROUND_APS_ADDR")?,
            operator_aranya_addr: env_var("OPERATOR_ARANYA_ADDR")?,
            operator_aps_addr: env_var("OPERATOR_APS_ADDR")?,
        })
    }
}

#[derive(Debug, Parser, Clone)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Working directory.
    work_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Ground starting! Release: {}", VERSION);

    let args = Args::parse();
    debug!("working directory: {:?}", &args.work_dir);

    let log_dir = args.work_dir.join("logs");
    let file_appender = tracing_appender::rolling::hourly(log_dir, "log.txt");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::registry()
        .with(fmt::Layer::new().with_ansi(false).with_writer(non_blocking))
        .with(EnvFilter::from_env("ORBITSECURE_DAEMON"))
        .init();

    let env = EnvVars::new()?;

    info!("Ground starting! Release: {}", VERSION);

    // Generate users
    let label = Label::new(1);
    let (users, peers, proxy) = generate_users(&env, label).await?;
    info!("user generation done");

    // Setup config files for daemons
    let ctx = ExecutionCtx::new(&args.work_dir, users, peers, Some(proxy), false).await?;

    // Start daemons
    println!("Ground daemon starting!");
    for (i, d) in ctx.daemons.into_iter().enumerate() {
        info!("starting daemon {}", i);
        println!("starting daemon {}", i);
        let name = ctx.handles[i].name.clone();
        let role = ctx.handles[i].role;
        task::spawn(async move {
            // Make sure every span mentions the daemon's name.
            let span = info_span!("daemon", daemon = name);
            async move {
                d.run()
                    .await
                    .with_context(|| format!("daemon name: {} role: {}", name, role))
                    .expect("expected no errors");
            }
            .instrument(span)
            .await
        });
    }

    // give daemons time to setup UDS APIs.
    sleep(Duration::from_millis(500)).await;

    let appgnd = ctx
        .handles
        .first()
        .context("could not find ground user")?
        .clone();

    let operator = ctx
        .handles
        .last()
        .context("could not find operator user")?
        .clone();

    let operator_args = args.clone();
    let operator_env = env.clone();

    let appgnd_handle = task::spawn(async move {
        if let Err(e) = run_ground_app(&appgnd, &args, &env, label).await {
            error!(?e);
            Err(e)
        } else {
            info!("appgnd ran successfully");
            Ok(())
        }
    });

    run_ground_operator(&operator, &operator_args, &operator_env)
        .await
        .context("expected operator to be able to send capture command")?;

    appgnd_handle.await?.context("failed to run ground app")?;

    println!("Ground done!");
    Ok(())
}

async fn run_ground_operator(operator: &DaemonCtx, args: &Args, env: &EnvVars) -> Result<()> {
    // TODO: get the appspace user id from the graph.
    let appspace_user_id =
        read_json::<UserId>(args.work_dir.join("appgnd").join("appspace_user_id"))?;

    // Send image capture command.
    info!(
        "sending image capture command from ground operator to space, path: {:?}",
        env.remote_image_path.clone()
    );
    println!(
        "sending image capture command from ground operator to space, path: {:?}",
        env.remote_image_path.clone()
    );
    operator
        .client
        .capture_image(appspace_user_id, env.remote_image_path.clone())
        .await?;
    info!("sent image capture command");

    Ok(())
}

async fn run_ground_app(
    appgnd: &DaemonCtx,
    args: &Args,
    env: &EnvVars,
    label: Label,
) -> Result<()> {
    // TODO: get the appspace user id from the graph.
    let appspace_user_id =
        read_json::<UserId>(args.work_dir.join("appgnd").join("appspace_user_id"))?;
    appgnd
        .client
        .set_user_addr(appspace_user_id, env.moc_aps_addr)
        .await?;

    let appgnd_user_id = appgnd.client.get_user_id().await?;
    appgnd
        .client
        .set_user_addr(appgnd_user_id, env.ground_aps_addr)
        .await?;

    let moc_tarpc_addr = env.moc_tarpc_addr.lookup().await?;

    // Attempt to send image capture command.
    // This will fail because apps do not have permission to send the image capture command.
    let path = "/home/nonroot/app/out/images/imaginary_image.png".to_string();
    info!(
        "attempting to send image capture command from ground app to space, path: {:?}",
        path
    );
    println!(
        "attempting to send image capture command from ground app to space, path: {:?}",
        path
    );
    if let Err(e) = appgnd
        .client
        .capture_image_no_retries(appspace_user_id, path)
        .await
    {
        error!("ground app unable to send image capture command: {}", e);
        println!(
            "error: ground app unable to send image capture command: {}",
            e
        );
    }

    // Download APS datagrams from MOC and send them to ground daemon to process.
    info!("tarpc client connecting to: {}", moc_tarpc_addr);
    let tarpc_client = retry(|| async {
        RPCClient::new_tcp(SocketAddr::V4(moc_tarpc_addr))
            .await
            .context("connecting to tarpc server")
    })
    .await?;
    info!("tarpc client connected to: {}", moc_tarpc_addr);

    task::spawn(
        process_aps_datagrams(tarpc_client, appgnd.client.clone())
            .instrument(info_span!("aps datagrams")),
    );

    info!("waiting for APS channel to be ready");
    while appgnd
        .client
        .is_aps_channel_ready(appspace_user_id, label)
        .await
        .is_err()
    {}
    info!("APS channel is ready");

    println!("waiting until APS data is ready.");

    // Capture image from Space sent via APS.
    let appgnd_aps_path = appgnd.cfg.internal_app_aps_path.clone();
    info!("starting testaps recv task");

    match Path::new(&env.remote_image_path).file_name() {
        Some(filename) => {
            let timestamp = Utc::now();
            let filename = format!(
                "{:?}-{}",
                timestamp,
                filename
                    .to_str()
                    .expect("expected to convert filename to str")
            );
            let local_image_path = args.work_dir.join("images").join(filename);
            while let Err(e) = testaps::recv_file(&local_image_path, &appgnd_aps_path)
                .await
                .with_context(|| "aps recv appgnd".to_string())
            {
                error!(?e);
                sleep(Duration::from_millis(100)).await;
            }

            info!("received image from space: {:?}", local_image_path);
            println!("received image from space: {:?}", local_image_path);
        }
        None => bail!("failed to parse filename"),
    }

    Ok(())
}

async fn process_aps_datagrams(tarpc_client: RPCClient, uds_client: UdsClient) -> ! {
    let mut first = true;
    loop {
        match tarpc_client.get_next_aps_datagram(context::current()).await {
            Ok(Some(datagram)) => {
                debug!("received next APS datagram from tarpc");
                if let Err(err) = uds_client.recv_aps_datagram(datagram).await {
                    error!(?err, "error receiving next aps datagram");
                    continue;
                }
                if first {
                    first = false;
                    // Fall through to sleep for after control message.
                } else {
                    continue;
                }
            }
            Ok(None) => trace!("no datagram received"),
            Err(err) => error!(?err, "error getting next aps datagram"),
        }
        sleep(Duration::from_secs(1)).await;
    }
}

async fn generate_users(
    env: &EnvVars,
    label: Label,
) -> Result<(Vec<User>, BTreeMap<String, Peer>, Proxy)> {
    info!("generating users");
    let users = vec![
        User::from_addrs(
            "appgnd".into(),
            Role::App,
            vec!["appmoc".into(), "operator".into()],
            None,
            env.ground_aranya_addr,
            env.ground_aps_addr,
            None,
        )?,
        User::from_addrs(
            "operator".into(),
            Role::Operator,
            vec![],
            None,
            env.operator_aranya_addr,
            env.operator_aps_addr,
            None,
        )?,
    ];

    let mut peers: BTreeMap<String, Peer> = BTreeMap::new();
    peers.insert(
        "appmoc".into(),
        Peer {
            host: env.moc_aranya_addr.host().to_string(),
            port: env.moc_aranya_addr.port(),
            role: Role::App,
        },
    );
    peers.insert(
        "operator".into(),
        Peer {
            host: env.operator_aranya_addr.host().to_string(),
            port: env.operator_aranya_addr.port(),
            role: Role::Operator,
        },
    );

    let mut proxy: Proxy = BTreeMap::new();
    proxy.insert(label, env.moc_aps_addr.lookup().await?);

    Ok((users, peers, proxy))
}
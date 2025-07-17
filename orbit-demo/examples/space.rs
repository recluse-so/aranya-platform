// Copyright (c) SpiderOak, Inc. All rights reserved.

//! Application script running on Space machine in HSA demo.

use std::{
    collections::BTreeMap,
    future,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use application::{
    testaps,
    util::{
        env::env_var,
        exec::{ExecutionCtx, User},
        json::read_json,
    },
};
use aranya_crypto::UserId;
use aranya_fast_channels::Label;
use clap::Parser;
use daemon::{addr::Addr, config::Peer, policies::base::vm_policy::Role, Proxy};
use tokio::{task, time::sleep};
use tracing::{debug, info, info_span, Instrument};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

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

#[derive(Debug, Parser)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Working directory.
    work_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Space starting! Release: {}", VERSION);

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

    info!("Space starting! Release: {}", VERSION);

    // Generate users
    let label = Label::new(1);
    let (users, peers, proxy) = generate_users(&env, label).await?;

    // Setup config files for daemons
    let ctx = ExecutionCtx::new(&args.work_dir, users, peers, Some(proxy), false).await?;

    // Start daemons
    for (i, d) in ctx.daemons.into_iter().enumerate() {
        info!("starting daemon {}", i);
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

    let appspace = ctx.handles.first().context("could not find ground user")?;

    // TODO: get the appgnd user id from the graph.
    let appgnd_user_id =
        read_json::<UserId>(args.work_dir.join("appspace").join("appgnd_user_id"))?;

    appspace
        .client
        .set_user_addr(appgnd_user_id, env.moc_aps_addr)
        .await?;
    let appspace_user_id = appspace.client.get_user_id().await?;
    appspace
        .client
        .set_user_addr(appspace_user_id, env.space_aps_addr)
        .await?;

    // TODO: get the appgnd user id from the graph.
    let appgnd_user_id =
        read_json::<UserId>(args.work_dir.join("appspace").join("appgnd_user_id"))?;

    // Receive command to capture image from Ground.
    println!("Space waiting to receive image capture command from ground");
    let path = loop {
        if let Ok(path) = appspace.client.poll_capture_image().await {
            break path;
        }
        sleep(Duration::from_millis(100)).await;
    };
    info!("received image capture command from ground, path: {}", path);
    println!("received image capture command from ground, path: {}", path);

    // Disabling MOC sync peer to conserve bandwidth.
    appspace
        .client
        .disable_sync_peer(env.moc_aranya_addr)
        .await
        .context("error disabling moc sync peer")?;

    // TODO: this only needs to be a uni channel for the demo.
    info!("creating bidirectional APS channel with ground");
    println!("creating bidirectional APS channel with ground");
    appspace
        .client
        .create_aps_bidi_channel(appgnd_user_id, label)
        .await
        .context("error creating bidi channel")?;

    info!("waiting for APS channel to be ready");
    println!("waiting for APS channel to be ready");
    while appspace
        .client
        .is_aps_channel_ready(appgnd_user_id, label)
        .await
        .is_err()
    {
        sleep(Duration::from_millis(100)).await;
    }

    // TODO: send to MOC APS address when proxy is setup.
    info!("sending image with testaps: {:?}", path);
    println!("sending image with testaps: {:?}", path);
    testaps::send_file(
        env.moc_aps_addr.lookup().await?,
        label,
        Path::new(&path),
        &appspace.cfg.internal_aps_path,
    )
    .await?;

    println!("Space complete");

    // Keep executable running so daemon can complete APS file transfer.
    future::pending().await
}

async fn generate_users(
    env: &EnvVars,
    label: Label,
) -> Result<(Vec<User>, BTreeMap<String, Peer>, Proxy)> {
    let users = vec![User::from_addrs(
        "appspace".into(),
        Role::App,
        vec!["appmoc".into()],
        None,
        env.space_aranya_addr,
        env.space_aps_addr,
        None,
    )?];

    let mut peers: BTreeMap<String, Peer> = BTreeMap::new();
    peers.insert(
        "appmoc".into(),
        Peer {
            host: env.moc_aranya_addr.host().to_string(),
            port: env.moc_aranya_addr.port(),
            role: Role::App,
        },
    );

    let mut proxy: Proxy = BTreeMap::new();
    proxy.insert(label, env.moc_aps_addr.lookup().await?);

    Ok((users, peers, proxy))
}

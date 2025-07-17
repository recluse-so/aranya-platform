async fn main() -> Result<()> {
    // Initialize Aranya client (similar to what you saw in daemon.rs)
    let cfg = Config { /* ... */ };
    let daemon = Daemon::load(cfg).await?;
    
    // Get client from daemon setup
    let client = Arc::new(client);
    
    // Create or load team graph
    let (graph_id, _) = client.create_team(owner_keys, None).await?;
    
    // Initialize SSH access manager
    let ssh_manager = SshAccessManager::new(
        Arc::clone(&client),
        graph_id,
        PathBuf::from("/etc/aranya/ssh/keys"),
        PathBuf::from("/etc/aranya/ssh/hosts")
    );
    ssh_manager.initialize().await?;
    
    // Start background sync
    ssh_manager.start_sync_daemon(300).await?; // Sync every 5 minutes
    
    // Add a user with admin SSH access
    let user_keys = KeyBundle { /* ... */ };
    let user_id = ssh_manager.add_ssh_user(user_keys, true).await?;
    
    // Grant access to specific hosts
    ssh_manager.grant_host_access(user_id, "server1.example.com").await?;
    ssh_manager.grant_host_access(user_id, "server2.example.com").await?;
    
    Ok(())
}
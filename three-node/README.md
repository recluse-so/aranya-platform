# Three Node Rust Application

This is a `cargo-generate` template.

## Description of main.rs

The `main.rs` file is a simple Rust application that demonstrates how to use the `aranya-client` library to send encrypted data between 3 different users.

The application creates a team with 3 users and then sends a message from one user to another.

## Description of Code Components in main.rs

The main.rs file contains several key components:

### Data Structures

#### TeamCtx
- Represents a team context containing 5 users: owner, admin, operator, and two members (membera, memberb)
- Each user is represented by a UserCtx struct
- Implements a `new()` method to create a new team with all users

#### UserCtx 
- Represents an individual user context
- Contains:
  - An Aranya client instance
  - The user's public key bundle
  - The user's device ID
- Implements methods for:
  - Creating a new user with daemon configuration
  - Getting local addresses for Aranya and AFC communication

### Main Function Flow

1. **Initialization**
   - Initializes tracing for logging with configurable log levels

2. **Setup**
   - Creates a temporary directory for the application
   - Configures sync and sleep intervals

3. **Team Creation and Setup**
   - Creates a new TeamCtx with 5 users
   - Owner creates a new team
   - Retrieves sync addresses for all users
   - Gets AFC addresses for member users

4. **Daemon Configuration**
   - For each user, configures and starts an aranya-daemon instance
   - Sets up Unix Domain Socket paths
   - Configures shared memory paths
   - Initializes AFC (Aranya Fast Channel) settings

5. **Client Setup**
   - Connects clients to their respective daemons
   - Retrieves key bundles and device IDs
   - Establishes team connections for all users

The code demonstrates a complete setup for secure multi-user communication using the Aranya platform, handling everything from daemon configuration to client initialization and team management.

## Background Daemon Operation

During setup, the example application starts an instance of the `aranya-daemon` for each Aranya user in the background. The daemon automatically handles syncing Aranya graph states between peers to the Aranya client can focus on the operations it wants to perform on the team.

## References

- [Tracing Log Levels Documentation](https://docs.rs/tracing/latest/tracing/struct.Level.html)

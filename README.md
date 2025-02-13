# Aranya Platform Example

This is a `cargo-generate` template.

An example of how to use the `aranya-client` library to:

- Setup a team
- Sync Aranya graphs
- Create an Aranya Fast Channel
- Send encrypted data between 3 different users

During setup, the example application starts an instance of the `aranya-daemon` for each Aranya user in the background. The daemon automatically handles syncing Aranya graph states between peers to the Aranya client can focus on the operations it wants to perform on the team.

# Generate a new workspace from this template:

Install [cargo-generate](https://github.com/cargo-generate/cargo-generate).

Generate a workspace for the example:
```bash
cargo generate aranya-project/aranya templates/aranya-example
```

# Building the example

```bash
cargo build --release
```

# Running the example

```bash
target/release/aranya-example
```

Optionally, you can set the tracing log level with:
```bash
ARANYA_EXAMPLE=info target/release/aranya-example
```

Reference:
[tracing log levels](https://docs.rs/tracing/latest/tracing/struct.Level.html)
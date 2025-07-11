mod sieve_client;

use clap::Parser;
use sieve_client::SieveClient;

#[derive(Parser)]
#[command(name = "sieve-gui")]
#[command(about = "A ManageSieve client for managing Sieve scripts")]
#[command(version)]
struct Args {
    /// ManageSieve server hostname
    #[arg(long)]
    host: String,

    /// Username for authentication
    #[arg(short, long)]
    username: String,

    /// Password for authentication
    #[arg(short, long)]
    password: String,

    /// Server port (default: 4190)
    #[arg(long, default_value_t = 4190)]
    port: u16,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    println!("ManageSieve Client");
    println!(
        "Connecting to {}:{} as {}",
        args.host, args.port, args.username
    );

    // Connect to the ManageSieve server
    let result = SieveClient::connect(args.host, args.port, args.username, args.password).await;

    match result {
        Ok(client) => {
            println!("✓ Successfully connected to ManageSieve server!");

            // Display parsed capabilities
            let caps = client.capabilities();
            println!("\nServer Capabilities:");

            if let Some(impl_name) = &caps.implementation {
                println!("  Implementation: {}", impl_name);
            }

            if let Some(version) = &caps.version {
                println!("  Version: {}", version);
            }

            if !caps.sasl.is_empty() {
                println!("  SASL mechanisms: {}", caps.sasl.join(", "));
            }

            if !caps.sieve.is_empty() {
                println!("  Sieve extensions: {}", caps.sieve.join(", "));
            }

            if caps.starttls {
                println!("  STARTTLS: supported");
            }

            if let Some(max_redirects) = caps.maxredirects {
                println!("  Max redirects: {}", max_redirects);
            }

            if !caps.notify.is_empty() {
                println!("  Notify methods: {}", caps.notify.join(", "));
            }

            if let Some(language) = &caps.language {
                println!("  Language: {}", language);
            }

            if let Some(owner) = &caps.owner {
                println!("  Owner: {}", owner);
            }

            if !caps.other.is_empty() {
                println!("  Other capabilities:");
                for (name, value) in &caps.other {
                    println!("    {}: {}", name, value);
                }
            }

            // TODO: Add sieve script management operations here
        }
        Err(e) => {
            eprintln!("Failed to connect: {}", e);
        }
    }
}

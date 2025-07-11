mod sieve_client;

use clap::Parser;
use dotenv::dotenv;
use sieve_client::SieveClient;
use std::env;

#[derive(Parser)]
#[command(name = "sieve-gui")]
#[command(about = "A ManageSieve client for managing Sieve scripts")]
#[command(
    long_about = "A ManageSieve client for managing Sieve scripts.\n\nCredentials can be provided via command line arguments or environment variables.\nEnvironment variables can be loaded from a .env file."
)]
#[command(version)]
struct Args {
    /// ManageSieve server hostname (or set SIEVE_HOST)
    #[arg(long, env = "SIEVE_HOST")]
    host: Option<String>,

    /// Username for authentication (or set SIEVE_USERNAME)
    #[arg(short, long, env = "SIEVE_USERNAME")]
    username: Option<String>,

    /// Password for authentication (or set SIEVE_PASSWORD)
    #[arg(short, long, env = "SIEVE_PASSWORD")]
    password: Option<String>,

    /// Server port (default: 4190, or set SIEVE_PORT)
    #[arg(long, default_value_t = 4190, env = "SIEVE_PORT")]
    port: u16,
}

#[tokio::main]
async fn main() {
    // Load .env file if it exists
    let _ = dotenv();

    let args = Args::parse();

    // Get required values from args or environment
    let host = args
        .host
        .or_else(|| env::var("SIEVE_HOST").ok())
        .expect("Host must be provided via --host argument or SIEVE_HOST environment variable");

    let username = args.username.or_else(|| env::var("SIEVE_USERNAME").ok())
        .expect("Username must be provided via --username argument or SIEVE_USERNAME environment variable");

    let password = args.password.or_else(|| env::var("SIEVE_PASSWORD").ok())
        .expect("Password must be provided via --password argument or SIEVE_PASSWORD environment variable");

    println!("ManageSieve Client");
    println!("Connecting to {}:{} as {}", host, args.port, username);

    // Connect to the ManageSieve server
    let result = SieveClient::connect(host, args.port, username, password).await;

    match result {
        Ok(client) => {
            println!("✓ Successfully connected to ManageSieve server!");
            println!("✓ Authentication successful!");

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

            println!("\n✓ Ready for script management operations!");
        }
        Err(e) => {
            eprintln!("❌ Connection failed: {}", e);
            std::process::exit(1);
        }
    }
}

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "tokscale-bundle")]
#[command(about = "Bundle tokscale-discoverable session data for replay on another machine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Capture this device's Tokscale-discoverable session files into a zip.
    Export {
        #[arg(long, value_name = "ZIP")]
        output: PathBuf,
    },
    /// Upload this device's bundle to a tokscale-bundle server.
    Upload {
        #[arg(long, value_name = "URL")]
        server: String,
        #[arg(long, env = "TOKSCALE_BUNDLE_TOKEN", value_name = "TOKEN")]
        token: Option<String>,
        #[arg(long, value_name = "ID")]
        device_id: Option<String>,
        #[arg(long, value_name = "NAME")]
        device_name: Option<String>,
    },
    /// Run a payload proxy and bundle upload server.
    Serve {
        #[arg(long, default_value = tokscale_bundle::hub::default_bind(), value_name = "ADDR")]
        bind: String,
        #[arg(long, value_name = "DIR")]
        storage: PathBuf,
        #[arg(long, env = "TOKSCALE_BUNDLE_TOKEN", value_name = "TOKEN")]
        token: Option<String>,
        #[arg(
            long,
            default_value = tokscale_bundle::hub::default_upstream_url(),
            value_name = "URL"
        )]
        upstream_url: String,
        #[arg(long)]
        submit: bool,
    },
    /// Materialize an imported archive as a fake home for plain Tokscale.
    Unpack { archive: PathBuf },
    /// Append this device's local sessions into an existing unpack root.
    AddLocal { unpack_root: PathBuf },
    /// Remove an unpack root created by this tool.
    Cleanup { unpack_root: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Export { output } => tokscale_bundle::tokscale::export_current_machine(&output),
        Commands::Upload {
            server,
            token,
            device_id,
            device_name,
        } => tokscale_bundle::hub::upload_current_machine(tokscale_bundle::hub::UploadOptions {
            server,
            token,
            device_id,
            device_name,
        }),
        Commands::Serve {
            bind,
            storage,
            token,
            upstream_url,
            submit,
        } => tokscale_bundle::hub::serve(tokscale_bundle::hub::ServeOptions {
            bind,
            storage,
            token,
            upstream_url,
            submit,
        }),
        Commands::Unpack { archive } => {
            let bundle = tokscale_bundle::replay::unpack_bundle_archive(&archive)?;
            tokscale_bundle::replay::print_unpack_summary(&bundle);
            Ok(())
        }
        Commands::AddLocal { unpack_root } => {
            let result =
                tokscale_bundle::tokscale::add_current_machine_to_unpack_root(&unpack_root)?;
            tokscale_bundle::tokscale::print_add_local_summary(&result);
            Ok(())
        }
        Commands::Cleanup { unpack_root } => {
            let removed = tokscale_bundle::replay::cleanup_unpack_root(&unpack_root)?;
            println!("removed={}", removed.display());
            Ok(())
        }
    }
}

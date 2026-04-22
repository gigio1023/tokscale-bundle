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
    Export {
        #[arg(long, value_name = "ZIP")]
        output: PathBuf,
    },
    Unpack {
        archive: PathBuf,
    },
    Cleanup {
        unpack_root: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Export { output } => tokscale_bundle::tokscale::export_current_machine(&output),
        Commands::Unpack { archive } => {
            let bundle = tokscale_bundle::replay::unpack_bundle_archive(&archive)?;
            tokscale_bundle::replay::print_unpack_summary(&bundle);
            Ok(())
        }
        Commands::Cleanup { unpack_root } => {
            let removed = tokscale_bundle::replay::cleanup_unpack_root(&unpack_root)?;
            println!("removed={}", removed.display());
            Ok(())
        }
    }
}

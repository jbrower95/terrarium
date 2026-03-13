use clap::Parser;

#[derive(Parser)]
#[command(name = "terrarium", about = "Terrarium installer")]
struct Cli {
    /// Install terrarium
    #[arg(long)]
    install: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.install {
        println!("Installing terrarium...");
    } else {
        println!("terrarium");
    }

    Ok(())
}

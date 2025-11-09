use crate::packager::{command, BuildArgs, BuildEnv};
use anyhow::Result;
use clap::{Parser, Subcommand};
use localdesktop::core::packager::env::BuildEnv;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(subcommand)]
    command: Commands,
}

fn main() -> Result<()> {
    let args = Args::parse();
    args.command.run()
}

#[derive(Subcommand)]
enum Commands {
    Build {
        #[clap(flatten)]
        args: BuildArgs,
    },
    /// Run app on an attached device
    Run {
        #[clap(flatten)]
        args: BuildArgs,
    },
}

impl Commands {
    pub fn run(self) -> Result<()> {
        match self {
            Self::Build { args } => {
                let env = BuildEnv {
                    name: todo!(),
                    build_target: todo!(),
                    build_dir: todo!(),
                    cache_dir: todo!(),
                    icon: todo!(),
                    cargo: todo!(),
                    config: todo!(),
                    verbose: todo!(),
                    offline: todo!(),
                };
                command::build(&env)?;
            }
            Self::Run { args } => {
                let env = BuildEnv {};
                command::build(&env)?;
                command::run(&env)?;
            }
        }
        Ok(())
    }
}

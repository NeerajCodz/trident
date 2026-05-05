use crate::{Result, TridentConfig, TridentEngine};
use bytes::Bytes;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "trident")]
#[command(about = "Trident storage engine CLI")]
pub struct Cli {
    #[arg(long, global = true, default_value = ".trident")]
    data_dir: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Put {
        key: String,
        value: String,
    },
    Get {
        key: String,
    },
    Delete {
        key: String,
    },
    Scan {
        #[arg(long)]
        start: Option<String>,
        #[arg(long)]
        end: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Flush,
    Compact,
    Recover,
    Inspect,
    Bench {
        #[arg(long, default_value_t = 10_000)]
        writes: usize,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let engine = TridentEngine::open(TridentConfig::new(cli.data_dir))?;
    match cli.command {
        Command::Put { key, value } => {
            let sequence = engine.put(Bytes::from(key), Bytes::from(value))?;
            println!("{sequence}");
        }
        Command::Get { key } => match engine.get(key.as_bytes())? {
            Some(value) => println!("{}", String::from_utf8_lossy(&value)),
            None => println!(),
        },
        Command::Delete { key } => {
            let sequence = engine.delete(Bytes::from(key))?;
            println!("{sequence}");
        }
        Command::Scan { start, end, limit } => {
            for (key, value) in engine.scan(
                start.as_deref().map(str::as_bytes),
                end.as_deref().map(str::as_bytes),
                limit,
            )? {
                println!(
                    "{}={}",
                    String::from_utf8_lossy(&key),
                    String::from_utf8_lossy(&value)
                );
            }
        }
        Command::Flush => {
            println!("{:?}", engine.flush()?);
        }
        Command::Compact => {
            println!("{}", engine.compact()?);
        }
        Command::Recover => {
            println!("{}", serde_json::to_string_pretty(&engine.recover())?);
        }
        Command::Inspect => {
            println!("{}", serde_json::to_string_pretty(&engine.stats())?);
        }
        Command::Bench { writes } => {
            let started = std::time::Instant::now();
            for i in 0..writes {
                engine.put(
                    Bytes::from(format!("bench/{i:020}")),
                    Bytes::from(format!("value/{i:020}")),
                )?;
            }
            let elapsed = started.elapsed();
            println!(
                "{} writes in {:?} ({:.2} writes/sec)",
                writes,
                elapsed,
                writes as f64 / elapsed.as_secs_f64()
            );
        }
    }
    Ok(())
}

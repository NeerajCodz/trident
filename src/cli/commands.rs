use crate::manifest::ColumnFamilyDescriptor;
use crate::server::{RestServerConfig, serve_rest};
use crate::{ColumnFamily, Result, TridentConfig, TridentEngine};
use bytes::Bytes;
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
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
        #[arg(long, default_value = "default")]
        cf: String,
        key: String,
        value: String,
    },
    Get {
        #[arg(long, default_value = "default")]
        cf: String,
        key: String,
    },
    Delete {
        #[arg(long, default_value = "default")]
        cf: String,
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
    Verify,
    Inspect,
    Bench {
        #[arg(long, default_value_t = 10_000)]
        writes: usize,
    },
    Checkpoint,
    Gc,
    CreateCf {
        name: String,
    },
    ListCf,
    Serve {
        #[arg(long, default_value = "127.0.0.1:7070")]
        bind: SocketAddr,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let data_dir = cli.data_dir.clone();
    let engine = TridentEngine::open(TridentConfig::new(&data_dir))?;
    match cli.command {
        Command::Put { cf, key, value } => {
            let mut batch = crate::WriteBatch::new();
            batch.put(ColumnFamily(cf), Bytes::from(key), Bytes::from(value));
            let sequence = engine.write_batch(batch)?;
            println!("{sequence}");
        }
        Command::Get { cf, key } => {
            match engine.get_cf(&ColumnFamily(cf), key.as_bytes(), engine.snapshot())? {
                Some(value) => println!("{}", String::from_utf8_lossy(&value)),
                None => println!(),
            }
        }
        Command::Delete { cf, key } => {
            let mut batch = crate::WriteBatch::new();
            batch.delete(ColumnFamily(cf), Bytes::from(key));
            let sequence = engine.write_batch(batch)?;
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
        Command::Verify => {
            println!("{}", serde_json::to_string_pretty(&engine.verify()?)?);
        }
        Command::Checkpoint => {
            println!("{}", serde_json::to_string_pretty(&engine.checkpoint()?)?);
        }
        Command::Gc => {
            println!(
                "{}",
                serde_json::to_string_pretty(&engine.garbage_collect()?)?
            );
        }
        Command::CreateCf { name } => {
            engine.create_column_family(ColumnFamilyDescriptor {
                name,
                ..ColumnFamilyDescriptor::default()
            })?;
        }
        Command::ListCf => {
            println!(
                "{}",
                serde_json::to_string_pretty(&engine.list_column_families())?
            );
        }
        Command::Serve { bind } => {
            let config = TridentConfig::new(data_dir);
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(serve_rest(RestServerConfig {
                engine: config,
                bind,
            }))?;
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

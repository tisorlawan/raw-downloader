#![allow(dead_code)]

use itertools::Itertools;

use ansi_term::Color::{Blue, Green, Red};
use futures::future::join_all;
use sha2::{Digest, Sha256};
use shellexpand;
use std::io::BufRead;
use thiserror::Error;
use tokio::fs::File;
use tokio::io::ErrorKind;
use tokio::prelude::*;

#[derive(Error, Debug)]
enum Error {
    #[error("Can't get data from url")]
    RequestError(#[from] reqwest::Error),

    #[error("Status")]
    WithStatusError(reqwest::StatusCode),

    #[error("Can't open file")]
    FileError(#[from] tokio::io::Error),
}

#[derive(Debug, PartialEq)]
enum State {
    New,
    Update,
    Same,
}

fn hash_eq(buf1: &[u8], buf2: &[u8]) -> bool {
    let mut hasher = Sha256::new();
    hasher.input(buf1);
    let buf1_hash = hasher.result_reset();

    hasher.input(buf2);
    let buf2_hash = hasher.result_reset();

    buf1_hash == buf2_hash
}

async fn process(dt: &DownloadTask) -> Result<State, Error> {
    let response = reqwest::get(&dt.remote_url).await?;
    if !response.status().is_success() {
        return Err(Error::WithStatusError(response.status()));
    }

    let bytes_remote = response.bytes().await?;

    let mut state: State = State::Update;
    let local_path = shellexpand::full(&dt.local_path).unwrap().into_owned();

    let mut bytes_local = Vec::new();
    let mut file = match File::open(&dt.local_path).await {
        Ok(f) => f,
        Err(e) => {
            if e.kind() == ErrorKind::NotFound {
                state = State::New;

                // Create new file and read it
                tokio::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .open(&local_path)
                    .await?
            } else {
                return Err(Error::FileError(e));
            }
        }
    };
    file.read_to_end(&mut bytes_local).await?;

    if !hash_eq(&bytes_local, &bytes_remote) {
        if state != State::New {
            state = State::Update;
        }
        let mut file = File::create(&local_path).await?;
        file.write_all(&bytes_remote).await?;
    } else {
        state = State::Same;
    }
    Ok(state)
}

#[derive(Debug)]
struct DownloadTask {
    remote_url: String,
    local_path: String,
}

impl DownloadTask {
    fn new(remote_url: String, local_path: String) -> Self {
        Self {
            remote_url,
            local_path,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut files: Vec<DownloadTask> = Vec::new();

    let stdin = std::io::stdin();
    for (not_empty, mut line) in stdin
        .lock()
        .lines()
        .map(|l| l.unwrap())
        .group_by(|el| *el != "")
        .into_iter()
    {
        let remote_url = line.next().unwrap();
        let local_path = line.next().unwrap();
        if not_empty {
            files.push(DownloadTask::new(remote_url, local_path));
        }
    }

    let processes: Vec<_> = files.iter().map(|dt| process(dt)).collect();

    let results = join_all(processes).await;

    let width = files.iter().map(|f| f.local_path.len()).max().unwrap_or(0) + 2;
    for (r, f) in results.iter().zip(files) {
        let line = match r {
            Ok(State::Same) => format!("{:<width$}", f.local_path, width = width),
            Ok(State::Update) => format!(
                "{}",
                Blue.bold()
                    .paint(format!("{:<width$}", f.local_path, width = width))
            ),
            Ok(State::New) => format!(
                "{}",
                Green
                    .bold()
                    .paint(format!("{:<width$}", f.local_path, width = width))
            ),
            Err(Error::WithStatusError(status)) => format!(
                "{} {}",
                Red.bold()
                    .paint(format!("{:<width$}", f.local_path, width = width)),
                Red.bold().paint(status.as_str())
            ),
            Err(err) => format!(
                "{} {}",
                Red.bold()
                    .paint(format!("{:<width$}", f.local_path, width = width)),
                Red.bold().paint(err.to_string())
            ),
        };
        println!("{}", line);
    }
    Ok(())
}

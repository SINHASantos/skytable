/*
 * Created on Thu Jul 02 2020
 *
 * This file is a part of Skytable
 * Skytable (formerly known as TerrabaseDB or Skybase) is a free and open-source
 * NoSQL database written by Sayan Nandan ("the Author") with the
 * vision to provide flexibility in data modelling without compromising
 * on performance, queryability or scalability.
 *
 * Copyright (c) 2020, Sayan Nandan <ohsayan@outlook.com>
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program. If not, see <https://www.gnu.org/licenses/>.
 *
*/

#![deny(unused_crate_dependencies, unused_imports, unused_must_use)]
#![cfg_attr(feature = "nightly", feature(test))]

//! # Skytable
//!
//! The `skyd` crate (or the `server` folder) is Skytable's database server and maybe
//! is the most important part of the project. There are several modules within this crate; see
//! the modules for their respective documentation.

use {env_logger::Builder, std::env};

#[macro_use]
extern crate log;
#[macro_use]
pub mod util;
mod engine;

use {
    crate::util::exit_error,
    libsky::{URL, VERSION},
};

#[cfg(all(not(target_env = "msvc"), not(miri)))]
#[global_allocator]
/// Jemallocator - this is the default memory allocator for platforms other than msvc
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

/// The terminal art for `!noart` configurations
const TEXT: &str = "
███████ ██   ██ ██    ██ ████████  █████  ██████  ██      ███████
██      ██  ██   ██  ██     ██    ██   ██ ██   ██ ██      ██
███████ █████     ████      ██    ███████ ██████  ██      █████
     ██ ██  ██     ██       ██    ██   ██ ██   ██ ██      ██
███████ ██   ██    ██       ██    ██   ██ ██████  ███████ ███████
";

type IoResult<T> = std::io::Result<T>;
const SKY_PID_FILE: &str = ".sky_pid";

fn main() {
    Builder::new()
        .parse_filters(&env::var("SKY_LOG").unwrap_or_else(|_| "info".to_owned()))
        .init();
    println!("{TEXT}\nSkytable v{VERSION} | {URL}\n");
    entrypoint()
}

fn entrypoint() {
    let run = || {
        let f_rt_start = || {
            engine::set_context_init("locking PID file");
            let pid_file = util::os::FileLock::new(SKY_PID_FILE)?;
            engine::set_context_init("initializing runtime");
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .thread_name("server")
                .enable_all()
                .build()?;
            Ok((pid_file, runtime))
        };
        let (pid_file, runtime) = match f_rt_start() {
            Ok((pf, rt)) => (pf, rt),
            Err(e) => return (None, None, Err(e)),
        };
        let f_glob_init = runtime.block_on(async move {
            engine::set_context_init("binding system signals");
            let signal = util::os::TerminationSignal::init()?;
            let (config, global) = tokio::task::spawn_blocking(|| engine::load_all())
                .await
                .unwrap()?;
            engine::RuntimeResult::Ok((signal, config, global))
        });
        let (signal, config, global) = match f_glob_init {
            Ok((sig, cfg, g)) => (sig, cfg, g),
            Err(e) => return (Some(pid_file), None, Err(e)),
        };
        let g = global.global.clone();
        let result_start =
            runtime.block_on(async move { engine::start(signal, config, global).await });
        (Some(pid_file), Some(g), result_start)
    };
    let (pid_file, global, result) = run();
    if let Some(g) = global {
        info!("cleaning up data");
        engine::finish(g);
    }
    if let Some(_) = pid_file {
        if let Err(e) = std::fs::remove_file(SKY_PID_FILE) {
            error!("failed to remove PID file: {e}");
        }
    }
    match result {
        Ok(()) => println!("goodbye"),
        Err(e) => {
            error!("{e}");
            exit_error();
        }
    }
}

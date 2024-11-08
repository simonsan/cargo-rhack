use cargo_rhack::cmd::{Cli, Cmd};
use cargo_rhack::error::SilentExit;

use clap::Parser;

use std::io::{self, Write};
use std::process;

fn main() {
    if let Err(e) = Cli::parse().run() {
        match e.downcast::<SilentExit>() {
            Ok(SilentExit { code }) => process::exit(code),
            Err(e) => {
                let _ = writeln!(io::stderr(), "rhack: {e:?}");
                process::exit(1);
            }
        }
    }
}

mod edit;
mod undo;

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::{env, str::FromStr};
use toml_edit::DocumentMut;

pub use edit::Edit;
pub use undo::Undo;

const DEFAULT_RHACK_DIR_NAME: &str = ".rhack";
const RHACK_DIR_ENV_KEY: &str = "RHACK_DIR";
const PATCH_TABLE_NAME: &str = "patch";
const REGISTRY_TABLE_NAME: &str = "crates-io";
const CARGO_HOME_ENV_KEY: &str = if cfg!(test) {
    "CARGO_HOME_TEST"
} else {
    "CARGO_HOME"
};
const CARGO_CONFIG_FILE_NAME: &str = "config.toml";

pub trait Cmd {
    fn run(&self) -> Result<()>;
}

#[derive(Debug, Parser)]
#[clap(about, bin_name = "cargo", author, version)]
#[command(propagate_version = true)]
pub struct Cli {
    #[arg(hide = true)]
    cargo: String,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Edit(Edit),
    Undo(Undo),
}

impl Cmd for Cli {
    fn run(&self) -> Result<()> {
        self.command.run()
    }
}

impl Cmd for Commands {
    fn run(&self) -> Result<()> {
        match self {
            Self::Edit(cmd) => cmd.run(),
            Self::Undo(cmd) => cmd.run(),
        }
    }
}

fn load_path_from_config() -> Result<PathBuf> {
    let get_path_from_config_env: Result<PathBuf> = {
        let parse_config = |s: PathBuf| {
            Result::Ok(s)
                .map(|cargo_dir| cargo_dir.join(CARGO_CONFIG_FILE_NAME))
                .and_then(fs::read_to_string)
                .map_err(From::from)
                .and_then(|config_text| Ok(config_text.parse::<DocumentMut>()?))
                .and_then(|mut x| {
                    x.remove("env")
                        .ok_or_else(|| anyhow!("Failed to find 'env' key in config.toml."))
                })
                .and_then(|x| {
                    let p = x
                        .get("RHACK_DIR")
                        .filter(|x| x.is_str())
                        .ok_or_else(|| anyhow!("Failed to find 'RHACK_DIR' key in config.toml."))
                        .and_then(|x| {
                            x.as_str()
                                .filter(|s| s.is_empty() == false)
                                .ok_or_else(|| anyhow!("RHACK_DIR is empty."))
                        })
                        .map(|x| x.to_string());

                    p
                })
                .and_then(|p| Ok(PathBuf::from_str(&p)?))
        };
        let cwd_config = || {
            env::current_dir()
                .map_err(From::from)
                .map(|cwd| cwd.join(".cargo"))
        };
        let global_config = || {
            env::var(CARGO_HOME_ENV_KEY)
                .map_err(From::from)
                .and_then(|x| Ok(PathBuf::from_str(&x)?))
        };

        if cfg!(test) {
            global_config().or_else(|_| cwd_config())
        } else {
            cwd_config().or_else(|_| global_config())
        }
        .and_then(parse_config)
    };
    get_path_from_config_env
}
// Gives back user-difined rhack dir path. If none, the default will be given.
#[must_use]
pub fn rhack_dir() -> PathBuf {
    env::var(RHACK_DIR_ENV_KEY)
        .map(PathBuf::from)
        .or_else(|_| load_path_from_config())
        .or_else(|_| {
            home::home_dir()
                .map(|dir| dir.join(DEFAULT_RHACK_DIR_NAME))
                .ok_or(anyhow!("Failed to find home directory."))
        })
        .expect("Failed to get directory path.")
}

// Gives back the the path to Cargo.toml reffered from the working directory.
pub fn manifest_path() -> Result<PathBuf> {
    // Run "cargo locate-project" to find out Cargo.toml file's location.
    // See: https://doc.rust-lang.org/cargo/commands/cargo-locate-project.html
    let out = Command::new("cargo")
        .arg("locate-project")
        .arg("--workspace")
        .output();

    let out = match out {
        Ok(o) => o,
        Err(err) => return Err(anyhow!("failed to run \"cargo locate-project\": {:#}", err)),
    };
    let out: Value = serde_json::from_slice(&out.stdout)?;
    let Ok(path) = PathBuf::from_str(match out["root"].as_str() {
        Some(it) => it,
        None => return Err(anyhow!("could convert to path")),
    }) else {
        return Err(anyhow!("unexpected response from \"cargo locate-project\""));
    };
    Ok(path)
}

// Gives back the parsed Cargo.toml placed at the working directory.
pub fn load_manifest(manifest_path: &PathBuf) -> Result<DocumentMut> {
    let manifest = match fs::read_to_string(manifest_path) {
        Ok(b) => b,
        Err(err) => {
            return Err(anyhow!(
                "failed to read from {}: {:#}",
                &manifest_path.display(),
                err
            ))
        }
    };
    match manifest.parse::<DocumentMut>() {
        Ok(m) => Ok(m),
        Err(err) => Err(anyhow!(
            "failed to parse {}: {:#}",
            &manifest_path.display(),
            err
        )),
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    fn run_tests_inside_path(run_test: impl Fn(PathBuf)) {
        use std::time::{SystemTime, UNIX_EPOCH};

        fn config_text(path: PathBuf) -> String {
            format!(
                r#"
        [env]
        RHACK_DIR = {path:?}
        "#
            )
        }
        fn random() -> Result<String> {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .map(|n| n.to_string())
                .map_err(From::from)
        }

        let get_random_test_dir = random()
            .map_err(anyhow::Error::from)
            .map(|rand| env::temp_dir().join("rhack_tests").join(rand))
            .and_then(|test_dir| {
                fs::create_dir_all(&test_dir)?;
                Ok(test_dir)
            })
            .and_then(|test_dir| {
                fs::write(
                    &test_dir.join(CARGO_CONFIG_FILE_NAME),
                    config_text(test_dir.clone()),
                )?;
                Ok(test_dir)
            });

        let _b = get_random_test_dir
            .inspect(|path| env::set_var(CARGO_HOME_ENV_KEY, path))
            .inspect(|path| run_test(path.clone()))
            .inspect(|path| {
                let _ = fs::remove_dir_all(&path);
            })
            .inspect(|_| env::remove_var(CARGO_HOME_ENV_KEY))
            .expect("run_tests_inside_path");
    }
    #[test]
    fn local_file_test() {
        env::remove_var(RHACK_DIR_ENV_KEY);

        let result = load_path_from_config().expect("local_file_test");

        assert_eq!(result, PathBuf::from("./").join(DEFAULT_RHACK_DIR_NAME));
    }
    #[test]
    fn global_file_test() {
        env::remove_var(RHACK_DIR_ENV_KEY);

        run_tests_inside_path(|path| {
            let result = load_path_from_config().expect("global_file_test");
            assert_eq!(result, path);
        });
    }
    #[test]
    fn rhack_env_override_test() {
        env::remove_var(RHACK_DIR_ENV_KEY);

        run_tests_inside_path(|path| {
            let modified_path = path.join("modified");
            env::set_var(RHACK_DIR_ENV_KEY, modified_path.clone());
            let result = rhack_dir();
            assert_eq!(result, modified_path);
            assert_ne!(result, load_path_from_config().expect("rhack_env_dir_test"));
        });
    }
}

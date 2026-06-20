use anyhow::{bail, Context, Result};
use lazy_static::lazy_static;
use std::{path::PathBuf, process::Command};

#[derive(serde::Deserialize)]
pub struct Config {
    pub build_target: String,
    pub file_list: String,
    pub default_version: Option<String>,
    pub file_list_removed_prefixes: Option<Vec<String>>,
    pub no_object_check_for: Option<Vec<String>>,
    pub decomp_me: Option<ConfigDecompMe>,
}

#[derive(serde::Deserialize)]
pub struct ConfigDecompMe {
    /// Must specify either ( Compiler and Flags ) or ( Preset Id ).

    /// Name of the compiler used to compile the code
    pub compiler_name: Option<String>,

    /// Compilation flags that are used for creating scratches.
    pub default_compile_flags: Option<String>,
    /// Toggle overriding of default flags (above) with database flags. (True by default)
    pub override_compile_flags: Option<bool>,

    /// Preset ID used for categorizing. Requires registering preset with compiler and flags.
    pub preset_id: Option<String>,
}

lazy_static! {
    static ref CONFIG: Config = {
        let toml_path = get_repo_root()
            .expect("failed to get repo root")
            .join("tools/config.toml");
        let raw = std::fs::read_to_string(toml_path.as_path()).expect("failed to read config file");
        toml::from_str(&raw).expect("failed to parse config file")
    };
}

pub fn get_config() -> &'static Config {
    &CONFIG
}

pub fn get_repo_root() -> Result<PathBuf> {
    let current_dir = std::env::current_dir()?;
    let mut dir = current_dir.as_path();

    loop {
        if ["data", "src"].iter().all(|name| dir.join(name).is_dir()) {
            return Ok(dir.to_path_buf());
        }

        match dir.parent() {
            None => {
                bail!("failed to find repo root -- run this program inside the repo");
            }
            Some(parent) => dir = parent,
        };
    }
}

pub fn get_tools_path() -> Result<PathBuf> {
    Ok(get_repo_root()?.join("tools/common"))
}

fn get_version_specific_dir_path(dir_name: &str, version: Option<&str>) -> Result<PathBuf> {
    let dir_name = if let Some(v) = version {
        format!("{dir_name}/{v}")
    } else {
        dir_name.to_string()
    };

    Ok(get_repo_root()?.join(dir_name))
}

pub fn get_data_path(version: Option<&str>) -> Result<PathBuf> {
    get_version_specific_dir_path("data", version)
}

pub fn get_build_path(version: Option<&str>) -> Result<PathBuf> {
    get_version_specific_dir_path("build", version)
}

pub fn get_file_contents_at_git_rev(rev: &str, path: &str) -> Result<String> {
    let output = Command::new("git")
        .current_dir(get_repo_root()?)
        .arg("show")
        .arg(format!("{rev}:{path}"))
        .output()?;
    if !output.status.success() {
        bail!("Failed to get file {path} at rev {rev}");
    }

    Ok(String::from_utf8(output.stdout)?)
}

pub fn get_first_common_ancestor_of_git_revs(rev1: &str, rev2: &str) -> Result<String> {
    let output = Command::new("git")
        .current_dir(get_repo_root()?)
        .arg("merge-base")
        .arg("-a")
        .arg(rev1)
        .arg(rev2)
        .output()?;
    if !output.status.success() {
        bail!("Failed to get common ancestor for revs {rev1} and {rev2}");
    }

    let output = String::from_utf8(output.stdout)?;
    let line = output.lines().next().context("output is empty")?;

    Ok(line.to_string())
}

use clap::Parser;
use lazy_static::lazy_static;
use std::fs;
use std::net::IpAddr;
use std::str::FromStr;

lazy_static! {
    pub static ref ARGS: Args = Args::parse();
}

#[derive(Parser, Debug, Clone)]
#[clap(version, about, long_about = None)]
pub struct Args {
    #[clap(long, env = "MICROBIN_BASIC_AUTH_USERNAME")]
    pub auth_basic_username: Option<SecretArg>,

    #[clap(long, env = "MICROBIN_BASIC_AUTH_PASSWORD")]
    pub auth_basic_password: Option<SecretArg>,

    #[clap(short, long, env = "MICROBIN_PORT", default_value_t = 8080)]
    pub port: u16,

    #[clap(short, long, env="MICROBIN_BIND", default_value_t = IpAddr::from([0, 0, 0, 0]))]
    pub bind: IpAddr,

    #[clap(long, env = "MICROBIN_DATA_DIR", default_value = "microbin_data")]
    pub data_dir: String,

    #[clap(short, long, env = "MICROBIN_THREADS", default_value_t = 1)]
    pub threads: u8,

    #[clap(long, env = "MICROBIN_TITLE", default_value = "microbin")]
    pub title: String,

    #[clap(long, env = "MICROBIN_MAX_FILE_SIZE_MB", default_value_t = 2048)]
    pub max_file_size_mb: u64,
}

#[derive(Debug, Clone)]
pub struct SecretArg(pub String);

impl FromStr for SecretArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(path) = s.strip_prefix("file://").or_else(|| s.strip_prefix("file:")) {
            // Read value from file
            fs::read_to_string(path)
                .map(|content| SecretArg(content.trim().to_string()))
                .map_err(|e| format!("Failed to read secret from {}: {}", path, e))
        } else {
            // Use the raw string provided
            Ok(SecretArg(s.to_string()))
        }
    }
}

// Allow it to be treated like a string automatically
impl std::ops::Deref for SecretArg {
    type Target = String;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_defaults() {
        let a = Args::parse_from(["microbin"]);
        assert_eq!(a.max_file_size_mb, 2048);
        assert_eq!(a.title, "microbin");
    }
}

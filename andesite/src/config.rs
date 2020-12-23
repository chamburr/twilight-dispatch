use lazy_static::lazy_static;
use serde::de::DeserializeOwned;
use std::env;

lazy_static! {
    pub static ref CONFIG: Config = {
        Config {
            rust_log: get_env("RUST_LOG"),
            default_queue: get_env_as("DEFAULT_QUEUE"),
            bot_id: get_env_as("BOT_ID"),
            rabbit_host: get_env("RABBIT_HOST"),
            rabbit_port: get_env_as("RABBIT_PORT"),
            rabbit_username: get_env("RABBIT_USERNAME"),
            rabbit_password: get_env("RABBIT_PASSWORD"),
            redis_host: get_env("REDIS_HOST"),
            redis_port: get_env_as("REDIS_PORT"),
            andesite_host: get_env("ANDESITE_HOST"),
            andesite_port: get_env_as("ANDESITE_PORT"),
            andesite_secret: get_env("ANDESITE_SECRET"),
            prometheus_host: get_env("PROMETHEUS_HOST"),
            prometheus_port: get_env_as("PROMETHEUS_PORT"),
        }
    };
}

#[derive(Clone, Debug)]
pub struct Config {
    pub rust_log: String,
    pub default_queue: bool,
    pub bot_id: u64,
    pub rabbit_host: String,
    pub rabbit_port: u64,
    pub rabbit_username: String,
    pub rabbit_password: String,
    pub redis_host: String,
    pub redis_port: u64,
    pub andesite_host: String,
    pub andesite_port: u64,
    pub andesite_secret: String,
    pub prometheus_host: String,
    pub prometheus_port: u64,
}

fn get_env(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("Missing environmental variable: {}", name))
}

fn get_env_as<T: DeserializeOwned>(name: &str) -> T {
    let mut variable = get_env(name);

    simd_json::from_str(variable.as_mut_str())
        .or_else(|_| simd_json::from_str(format!(r#""{}""#, variable).as_mut_str()))
        .unwrap_or_else(|_| panic!("Invalid environmental variable: {}", name))
}

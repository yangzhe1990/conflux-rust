use clap;
use log::LevelFilter;
use std::fs::File;
use std::io::prelude::*;
use toml;

#[derive(Debug, PartialEq, Clone)]
pub struct Configuration {
    pub port: Option<u16>,
    pub jsonrpc_tcp_port: Option<u16>,
    pub jsonrpc_http_port: Option<u16>,
    pub log_file: Option<String>,
    pub log_level: LevelFilter,
}

impl Default for Configuration {
    fn default() -> Self {
        Configuration {
            port: None,
            jsonrpc_tcp_port: None,
            jsonrpc_http_port: None,
            log_file: None,
            log_level: LevelFilter::Info,
        }
    }
}

impl Configuration {
    pub fn parse(matches: &clap::ArgMatches) -> Result<Configuration, String> {
        let mut config = Configuration::default();

        let config_filename = matches.value_of("config");
        if config_filename.is_some() {
            let mut config_file = File::open(config_filename.unwrap())
                .expect("Config file not found");

            let mut config_str = String::new();
            config_file
                .read_to_string(&mut config_str)
                .expect("Error reading config file");

            let config_value = config_str.parse::<toml::Value>().unwrap();
            if let Some(port) = config_value.get("port") {
                config.port = port.as_integer().map(|x| x as u16);
            }
            if let Some(port) = config_value.get("jsonrpc-tcp-port") {
                config.jsonrpc_tcp_port = port.as_integer().map(|x| x as u16);
            }
            if let Some(port) = config_value.get("jsonrpc-http-port") {
                config.jsonrpc_http_port = port.as_integer().map(|x| x as u16);
            }
            if let Some(log_file) = config_value.get("log-file") {
                config.log_file = log_file.as_str().map(|x| x.to_owned());
            }
            if let Some(log_level) = config_value.get("log-level") {
                config.log_level = match log_level.as_str() {
                    Some("error") => LevelFilter::Error,
                    Some("warn") => LevelFilter::Warn,
                    Some("info") => LevelFilter::Info,
                    Some("debug") => LevelFilter::Debug,
                    Some("trace") => LevelFilter::Trace,
                    Some(_) => LevelFilter::Info,
                    None => LevelFilter::Info,
                };
            }
        }

        if let Some(port) = matches.value_of("port") {
            config.port =
                Some(port.parse().map_err(|_| "Invalid port".to_owned())?);
        }
        if let Some(port) = matches.value_of("jsonrpc-tcp-port") {
            config.jsonrpc_tcp_port = Some(
                port.parse()
                    .map_err(|_| "Invalid jsonrpc-tcp-port".to_owned())?,
            );
        }
        if let Some(port) = matches.value_of("jsonrpc-http-port") {
            config.jsonrpc_http_port = Some(
                port.parse()
                    .map_err(|_| "Invalid jsonrpc-http-port".to_owned())?,
            );
        }
        if let Some(log_file) = matches.value_of("log-file") {
            config.log_file = Some(log_file.to_owned());
        }
        config.log_level = match matches.value_of("log-level") {
            Some("error") => LevelFilter::Error,
            Some("warn") => LevelFilter::Warn,
            Some("info") => LevelFilter::Info,
            Some("debug") => LevelFilter::Debug,
            Some("trace") => LevelFilter::Trace,
            Some(_) => config.log_level,
            None => config.log_level,
        };

        Ok(config)
    }
}

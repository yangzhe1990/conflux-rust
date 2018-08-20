use clap;
use simplelog::LevelFilter;
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
            port: Some(32323),
            jsonrpc_tcp_port: Some(32324),
            jsonrpc_http_port: Some(32325),
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
            if let Some(port) = config_value["port"].as_str() {
                config.port =
                    Some(port.parse().map_err(|_| "Invalid port".to_owned())?);
            }
            if let Some(port) = config_value["jsonrpc-tcp-port"].as_str() {
                config.jsonrpc_tcp_port = Some(
                    port.parse()
                        .map_err(|_| "Invalid jsonrpc-tcp-port".to_owned())?,
                );
            }
            if let Some(port) = config_value["jsonrpc-http-port"].as_str() {
                config.jsonrpc_http_port = Some(
                    port.parse()
                        .map_err(|_| "Invalid jsonrpc-http-port".to_owned())?,
                );
            }
            if let Some(log_file) = config_value["log-file"].as_str() {
                config.log_file = Some(log_file.to_owned());
            }
            config.log_level = match matches.value_of("log-level") {
                Some("error") => LevelFilter::Error,
                Some("warn") => LevelFilter::Warn,
                Some("info") => LevelFilter::Info,
                Some("debug") => LevelFilter::Debug,
                Some("trace") => LevelFilter::Trace,
                Some(_) => LevelFilter::Info,
                None => LevelFilter::Info,
            };
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

use cache_config::CacheConfig;
use clap;
use log::LevelFilter;
use network::{node_table::validate_node_url, ErrorKind, NetworkConfiguration};
use std::{fs::File, io::prelude::*, net::ToSocketAddrs};
use toml;

#[derive(Debug, PartialEq, Clone)]
pub struct Configuration {
    pub port: Option<u16>,
    pub udp_port: Option<u16>,
    pub jsonrpc_tcp_port: Option<u16>,
    pub jsonrpc_http_port: Option<u16>,
    pub log_file: Option<String>,
    pub log_level: LevelFilter,
    pub bootnodes: Option<String>,
    pub netconf_dir: Option<String>,
    pub public_address: Option<String>,
    pub ledger_cache_size: Option<usize>,
    pub test_mode: bool,
}

impl Default for Configuration {
    fn default() -> Self {
        Configuration {
            port: None,
            udp_port: None,
            jsonrpc_tcp_port: None,
            jsonrpc_http_port: None,
            log_file: None,
            log_level: LevelFilter::Info,
            bootnodes: None,
            netconf_dir: None,
            public_address: None,
            ledger_cache_size: None,
            test_mode: false,
        }
    }
}

impl Configuration {
    // First parse arguments from config file,
    // and then parse them from commandline.
    // Replace the ones from config file with the ones
    // from commandline if duplicates.
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
            if let Some(port) = config_value.get("udp-port") {
                config.udp_port = port.as_integer().map(|x| x as u16);
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
            if let Some(bootnodes) = config_value.get("bootnodes") {
                config.bootnodes = bootnodes.as_str().map(|x| x.to_owned());
            }
            if let Some(netconf) = config_value.get("netconf-dir") {
                config.netconf_dir = netconf.as_str().map(|x| x.to_owned());
            }
            if let Some(public_address) = config_value.get("public-address") {
                config.public_address =
                    public_address.as_str().map(|x| x.to_owned());
            }
            if let Some(cache_size) = config_value.get("ledger-cache-size") {
                config.ledger_cache_size =
                    cache_size.as_integer().map(|x| x as usize);
            }
            if let Some(test_mode) = config_value.get("test-mode") {
                config.test_mode =
                    test_mode.as_bool().map_or(false, |x| x as bool);
            }
        }

        if let Some(port) = matches.value_of("port") {
            config.port =
                Some(port.parse().map_err(|_| "Invalid port".to_owned())?);
        }
        if let Some(port) = matches.value_of("udp-port") {
            config.udp_port =
                Some(port.parse().map_err(|_| "Invalid udp-port".to_owned())?);
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
        if let Some(bootnodes) = matches.value_of("bootnodes") {
            config.bootnodes = Some(bootnodes.to_owned());
        }
        if let Some(cache_size) = matches.value_of("ledger-cache-size") {
            config.ledger_cache_size = Some(
                cache_size.parse().map_err(|_| "Invalid port".to_owned())?,
            );
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
        if let Some(netconf) = matches.value_of("netconf-dir") {
            config.netconf_dir = Some(netconf.to_owned());
        }
        if let Some(public_address) = matches.value_of("public-address") {
            config.public_address = Some(public_address.to_owned());
        }
        if let Some(test_mode) = matches.value_of("test-mode") {
            config.test_mode = test_mode
                .parse()
                .map_err(|_| "test-mode not boolean".to_owned())?;
        }
        Ok(config)
    }

    pub fn net_config(&self) -> NetworkConfiguration {
        let mut network_config = match self.port {
            Some(port) => NetworkConfiguration::new_with_port(port),
            None => NetworkConfiguration::default(),
        };

        network_config.boot_nodes =
            to_bootnodes(&self.bootnodes).expect("Error parsing bootnodes!");
        if self.netconf_dir.is_some() {
            network_config.config_path = self.netconf_dir.clone();
        }
        if let Some(addr) = self.public_address.clone() {
            network_config.public_address = match addr
                .to_socket_addrs()
                .map(|mut i| i.next())
            {
                Ok(sock_addr) => sock_addr,
                Err(_e) => {
                    debug!(target: "network", "public_address in config is invalid");
                    None
                }
            };
        }
        network_config.test_mode = self.test_mode;
        network_config
    }

    pub fn cache_config(&self) -> CacheConfig {
        let cache_config = match self.ledger_cache_size {
            Some(cache_size) => CacheConfig::new(cache_size),
            None => CacheConfig::default(),
        };

        cache_config
    }
}

/// Validates and formats bootnodes option.
pub fn to_bootnodes(bootnodes: &Option<String>) -> Result<Vec<String>, String> {
    match *bootnodes {
        Some(ref x) if !x.is_empty() => x
            .split(',')
            .map(|s| match validate_node_url(s).map(Into::into) {
                None => Ok(s.to_owned()),
                Some(ErrorKind::AddressResolve(_)) => Err(format!(
                    "Failed to resolve hostname of a boot node: {}",
                    s
                )),
                Some(_) => Err(format!(
                    "Invalid node address format given for a boot node: {}",
                    s
                )),
            })
            .collect(),
        Some(_) => Ok(vec![]),
        None => Ok(vec![]),
    }
}

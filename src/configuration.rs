use clap;
use simplelog::LevelFilter;

const DEFAULT_PORT: u16 = 32323;

#[derive(Debug, PartialEq, Clone)]
pub struct Configuration {
    pub port: Option<u16>,
    pub jsonrpc_tcp_port: Option<u16>,
    pub jsonrpc_http_port: Option<u16>,
    pub log_file: Option<String>,
    pub log_level: LevelFilter,
}

impl Configuration {
    pub fn parse(matches: &clap::ArgMatches) -> Result<Configuration, String> {
        let port = match matches.value_of("port") {
            Some(port) => {
                Some(port.parse().map_err(|_| "Invalid port".to_owned())?)
            }
            None => None,
        };

        let jsonrpc_tcp_port = match matches.value_of("jsonrpc-tcp-port") {
            Some(port) => Some(
                port.parse()
                    .map_err(|_| "Invalid jsonrpc-tcp-port".to_owned())?,
            ),
            None => None,
        };

        let jsonrpc_http_port = match matches.value_of("jsonrpc-http-port") {
            Some(port) => Some(
                port.parse()
                    .map_err(|_| "Invalid jsonrpc-http-port".to_owned())?,
            ),
            None => None,
        };

        let log_file = matches.value_of("log-file");

        let log_level = match matches.value_of("log-level") {
            Some("error") => LevelFilter::Error,
            Some("warn") => LevelFilter::Warn,
            Some("info") => LevelFilter::Info,
            Some("debug") => LevelFilter::Debug,
            Some("trace") => LevelFilter::Trace,
            Some(_) => LevelFilter::Info,
            None => LevelFilter::Info,
        };

        Ok(Configuration {
            port: port,
            jsonrpc_tcp_port: jsonrpc_tcp_port,
            jsonrpc_http_port: jsonrpc_http_port,
            log_file: log_file.map(|s| s.to_string()),
            log_level: log_level,
        })
    }
}

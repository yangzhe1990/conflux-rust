use clap;

const DEFAULT_PORT: u16 = 32323;

#[derive(Debug, PartialEq, Clone)]
pub struct Configuration {
    pub port: Option<u16>,
    pub jsonrpc_tcp_port: Option<u16>,
    pub jsonrpc_http_port: Option<u16>,
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

        Ok(Configuration {
            port: port,
            jsonrpc_tcp_port: jsonrpc_tcp_port,
            jsonrpc_http_port: jsonrpc_http_port,
        })
    }
}

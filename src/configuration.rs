use clap;

const DEFAULT_PORT: u16 = 32323;

#[derive(Debug, PartialEq, Clone)]
pub struct Configuration {
    pub port: Option<u16>,
    pub jsonrpc_port: Option<u16>,
}

impl Configuration {
    pub fn parse(matches: &clap::ArgMatches) -> Result<Configuration, String> {
        let port = match matches.value_of("port") {
            Some(port) => {
                Some(port.parse().map_err(|_| "Invalid port".to_owned())?)
            }
            None => None,
        };

        let jsonrpc_port = match matches.value_of("jsonrpc-port") {
            Some(port) => Some(
                port.parse()
                    .map_err(|_| "Invalid jsonrpc-port".to_owned())?,
            ),
            None => None,
        };

        Ok(Configuration {
            port: port,
            jsonrpc_port: jsonrpc_port,
        })
    }
}

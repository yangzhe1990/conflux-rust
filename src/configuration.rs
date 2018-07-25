use clap;

#[derive(Debug, PartialEq)]
pub struct Configuration {
    pub port: u16,
    pub jsonrpc_port: u16,
}

impl Configuration {
    pub fn parse(matches: &clap::ArgMatches) -> Result<Configuration, String> {
        let port = match matches.value_of("port") {
            Some(port) => port.parse().map_err(|_| "Invalid port".to_owned())?,
            None => 32323,
        };

        let jsonrpc_port = match matches.value_of("jsonrpc-port") {
            Some(port) => port.parse().map_err(|_| "Invalid port".to_owned())?,
            None => 2323,
        };

        Ok(Configuration {
            port: port,
            jsonrpc_port: jsonrpc_port,
        })
    }
}
